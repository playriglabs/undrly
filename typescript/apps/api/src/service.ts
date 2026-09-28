/**
 * Read-only queries behind the API. Every price comes from PostgreSQL
 * (`canonical_quotes` / `market_observations`), written by the Rust
 * collector; no request ever contacts an upstream source.
 */
import { v1 } from "@undrly/contracts";
import type postgres from "postgres";
import { ageMs, type FreshnessClock, policyElapsedMs } from "./market.ts";
import {
  type Caip2,
  type Caip19,
  canonicalTimestamp,
  classShareSymbol,
  type IdentifierScheme,
  normalizeIdentifier,
  type ParsedQuery,
  parseQuery,
} from "./query.ts";

export type Sql = postgres.Sql;

type NodeRef = v1.NodeRefV1;
type Resolution = v1.ResolutionV1;
type Method = v1.ResolveResultV1["method"];

const KIND_ORDER: Record<string, number> = {
  instrument: 0,
  currency: 1,
  entity: 2,
  venue: 3,
  listing: 4,
  chain: 5,
  deployment: 6,
};

// Every node with its display name, in one relation.
const NAMES = `(
  SELECT id, 'entity'::text AS kind, name::text AS name, NULL::text AS class FROM entities
  UNION ALL SELECT id, 'instrument', name, instrument_class FROM instruments
  UNION ALL SELECT id, 'venue', name, NULL FROM venues
  UNION ALL SELECT id, 'currency', name, NULL FROM currencies
  UNION ALL SELECT l.id, 'listing', i.name || ' on ' || v.name, NULL
    FROM listings l JOIN instruments i ON i.id = l.instrument_id JOIN venues v ON v.id = l.venue_id
  UNION ALL SELECT id, 'chain', name, NULL FROM chains
  UNION ALL SELECT d.id, 'deployment', c.name || ' ' || d.asset_namespace || ':' || d.asset_reference, NULL
    FROM deployments d JOIN chains c ON c.id = d.chain_id
)`;

export const tsText = (column: string) =>
  `to_char(${column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US')`;

type NameRow = { id: string; kind: v1.Category; name: string; class: v1.InstrumentClass | null };

function nodeRef(row: NameRow): NodeRef {
  return {
    id: v1.formatCanonicalId(row.kind, row.id) as NodeRef["id"],
    kind: row.kind,
    name: row.name,
    class: row.class,
  };
}

async function nodeRefs(sql: Sql, uuids: string[]): Promise<Map<string, NodeRef>> {
  const out = new Map<string, NodeRef>();
  if (uuids.length === 0) return out;
  const rows = await sql.unsafe<NameRow[]>(
    `SELECT id::text, kind, name, class FROM ${NAMES} n WHERE id = ANY($1::uuid[])`,
    [uuids],
  );
  for (const row of rows) out.set(row.id, nodeRef(row));
  return out;
}

async function nodeRefOf(sql: Sql, uuid: string): Promise<NodeRef | null> {
  return (await nodeRefs(sql, [uuid])).get(uuid) ?? null;
}

// --- price subjects and units -------------------------------------------------

async function currencyCode(sql: Sql, uuid: string): Promise<string | null> {
  const rows = await sql<{ value: string }[]>`
    SELECT value FROM identifiers
    WHERE node_id = ${uuid} AND scheme = 'iso4217' AND valid_during @> now()`;
  return rows[0]?.value ?? null;
}

async function assetCode(sql: Sql, uuid: string): Promise<string | null> {
  const rows = await sql<{ alias: string }[]>`
    SELECT alias FROM aliases WHERE node_id = ${uuid} AND kind = 'symbol' ORDER BY id LIMIT 1`;
  return rows[0]?.alias ?? null;
}

export async function subjectOf(sql: Sql, uuid: string): Promise<v1.PriceSubjectV1> {
  const node = await nodeRefOf(sql, uuid);
  if (node === null) throw new Error(`unknown node ${uuid}`);
  if (node.kind === "currency") {
    return {
      id: node.id as unknown as v1.CurrencyId,
      kind: "currency",
      code: (await currencyCode(sql, uuid)) as v1.CurrencyCode,
      name: node.name,
    };
  }
  if (node.kind !== "instrument" || node.class === null) throw new Error(`not priceable: ${uuid}`);
  const terms = await sql<
    {
      multiplier: string | null;
      uom: v1.UnitOfMeasure | null;
      base: string | null;
      quote: string | null;
    }[]
  >`
    SELECT contract_multiplier::text AS multiplier, unit_of_measure AS uom,
           base_currency_id::text AS base, quote_currency_id::text AS quote
    FROM instruments WHERE id = ${uuid}`;
  const { multiplier, uom, base, quote } = terms[0] ?? {
    multiplier: null,
    uom: null,
    base: null,
    quote: null,
  };
  const currencyRef = async (id: string) => ({
    id: v1.formatCanonicalId("currency", id) as v1.CurrencyId,
    code: (await currencyCode(sql, id)) as v1.CurrencyCode,
  });
  return {
    id: node.id as unknown as v1.InstrumentId,
    kind: "instrument",
    class: node.class,
    name: node.name,
    // Additive fields: absent (not null) when unset, so V1 output is unchanged.
    ...(multiplier === null ? {} : { contractMultiplier: multiplier as v1.DecimalString }),
    ...(uom === null ? {} : { unitOfMeasure: uom }),
    // FX markets: base and quote currency (the quote is the price unit).
    ...(base === null || quote === null
      ? {}
      : { baseCurrency: await currencyRef(base), quoteCurrency: await currencyRef(quote) }),
  };
}

export async function unitOf(sql: Sql, uuid: string, category: string): Promise<v1.PriceUnitV1> {
  if (category === "currency") {
    return {
      id: v1.formatCanonicalId("currency", uuid) as v1.CurrencyId,
      kind: "currency",
      code: (await currencyCode(sql, uuid)) as v1.CurrencyCode,
    };
  }
  return {
    id: v1.formatCanonicalId("instrument", uuid) as v1.InstrumentId,
    kind: "asset",
    code: await assetCode(sql, uuid),
  };
}

// --- resolve ------------------------------------------------------------------

export type Resolved = {
  status: "resolved" | "ambiguous" | "not_found";
  method: Method;
  resolutions: Resolution[];
  /** (subject uuid, unit uuid) for pair resolutions, parallel to `resolutions`. */
  pairs: ({ subject: string; unit: string; unitCategory: string } | null)[];
};

type Evidence = v1.MatchV1;
type Match = ({ uuid: string } | { subject: string; unit: string; unitCategory: string }) & {
  /** Why it matched; filled only when explaining (`explain = true`). */
  evidence: Evidence[];
};

const evidence = (
  rule: Evidence["rule"],
  value: string,
  extra: Partial<Omit<Evidence, "rule" | "value">> = {},
): Evidence => ({
  rule,
  side: extra.side ?? null,
  namespace: extra.namespace ?? null,
  value,
  venue: extra.venue ?? null,
  source: extra.source ?? null,
});

async function nodesByAlias(sql: Sql, text: string, symbolsOnly: boolean): Promise<string[]> {
  const rows = await sql<{ id: string }[]>`
    SELECT DISTINCT node_id::text AS id FROM aliases
    WHERE alias_key = lower(${text}) AND (${!symbolsOnly} OR kind = 'symbol')`;
  return rows.map((r) => r.id);
}

/** The alias rows that made `nodesByAlias` return `uuid`. */
async function aliasEvidence(
  sql: Sql,
  uuid: string,
  text: string,
  symbolsOnly: boolean,
  side: Evidence["side"],
): Promise<Evidence[]> {
  const rows = await sql<{ alias: string; kind: string; source_id: string }[]>`
    SELECT alias::text, kind, source_id FROM aliases
    WHERE node_id = ${uuid} AND alias_key = lower(${text}) AND (${!symbolsOnly} OR kind = 'symbol')
    ORDER BY id`;
  return rows.map((r) =>
    evidence("alias", r.alias, {
      side,
      namespace: r.kind,
      source: { id: r.source_id as v1.SourceId },
    }),
  );
}

/** Equity instruments listed under the class-share spelling of `token`. */
async function equitiesByClassShare(sql: Sql, token: string): Promise<string[]> {
  const symbol = classShareSymbol(token);
  if (symbol === null) return [];
  const rows = await sql<{ id: string }[]>`
    SELECT DISTINCT a.node_id::text AS id FROM aliases a JOIN instruments i ON i.id = a.node_id
    WHERE a.alias_key = lower(${symbol}) AND a.kind = 'symbol' AND i.instrument_class = 'equity'`;
  return rows.map((r) => r.id);
}

async function nodesByIdentifier(
  sql: Sql,
  scheme: IdentifierScheme | null,
  value: string,
): Promise<string[]> {
  const rows = await sql<{ id: string }[]>`
    SELECT DISTINCT node_id::text AS id FROM identifiers
    WHERE (${scheme}::text IS NULL OR scheme = ${scheme}) AND value = ${value}
      AND valid_during @> now()`;
  return rows.map((r) => r.id);
}

/** The identifier rows that made `nodesByIdentifier` return `uuid`. */
async function identifierEvidence(
  sql: Sql,
  uuid: string,
  scheme: IdentifierScheme | null,
  value: string,
  side: Evidence["side"] = null,
): Promise<Evidence[]> {
  const rows = await sql<{ scheme: string; value: string }[]>`
    SELECT scheme, value FROM identifiers
    WHERE node_id = ${uuid} AND (${scheme}::text IS NULL OR scheme = ${scheme}) AND value = ${value}
      AND valid_during @> now()
    ORDER BY scheme, id`;
  return rows.map((r) => evidence("identifier", r.value, { side, namespace: r.scheme }));
}

async function categoryOf(sql: Sql, uuid: string): Promise<string | null> {
  const rows = await sql<{ category: string }[]>`SELECT category FROM nodes WHERE id = ${uuid}`;
  return rows[0]?.category ?? null;
}

/** One side of a pair: exactly one instrument or currency, by symbol or ISO code. */
async function pairSide(sql: Sql, token: string): Promise<string[]> {
  const ids = new Set([
    ...(await nodesByAlias(sql, token, true)),
    ...(await equitiesByClassShare(sql, token)),
    ...(await nodesByIdentifier(sql, "iso4217", token.toUpperCase())),
  ]);
  const out: string[] = [];
  for (const id of ids) {
    const category = await categoryOf(sql, id);
    if (category === "instrument" || category === "currency") out.push(id);
  }
  return out;
}

/** Why `uuid` is a candidate for one side of a pair (the rules of `pairSide`). */
async function sideEvidence(
  sql: Sql,
  uuid: string,
  token: string,
  side: "base" | "quote",
): Promise<Evidence[]> {
  const shares = (await equitiesByClassShare(sql, token)).includes(uuid)
    ? [
        evidence("class_share_symbol", classShareSymbol(token) ?? token, {
          side,
          namespace: "symbol",
        }),
      ]
    : [];
  return [
    ...(await aliasEvidence(sql, uuid, token, true, side)),
    ...shares,
    ...(await identifierEvidence(sql, uuid, "iso4217", token.toUpperCase(), side)),
  ];
}

/** A chain by its CAIP-2 id (exact, case-sensitive). */
async function chainsByCaip2(sql: Sql, caip2: Caip2): Promise<string[]> {
  const rows = await sql<{ id: string }[]>`
    SELECT id::text FROM chains
    WHERE caip2_namespace = ${caip2.namespace} AND caip2_reference = ${caip2.reference}`;
  return rows.map((r) => r.id);
}

/** A deployment by its chain's CAIP-2 id and its asset (never by address alone). */
async function deploymentsByCaip19(sql: Sql, caip19: Caip19): Promise<string[]> {
  const rows = await sql<{ id: string }[]>`
    SELECT d.id::text FROM deployments d JOIN chains c ON c.id = d.chain_id
    WHERE c.caip2_namespace = ${caip19.chain.namespace}
      AND c.caip2_reference = ${caip19.chain.reference}
      AND d.asset_namespace = ${caip19.assetNamespace}
      AND d.asset_reference = ${caip19.assetReference}`;
  return rows.map((r) => r.id);
}

type Matched = { found: Match[]; quotedPairsOnly: boolean };

/**
 * The resolver's rules. `explain` only adds the evidence of each match (with
 * the same predicates, restricted to the matched node); it never changes
 * which nodes match or their order.
 */
async function matches(sql: Sql, q: ParsedQuery, explain: boolean): Promise<Matched> {
  const plain = (found: Match[]): Matched => ({ found, quotedPairsOnly: false });
  switch (q.kind) {
    case "canonical_id":
      return plain(
        (await categoryOf(sql, q.uuid)) === q.category
          ? [
              {
                uuid: q.uuid,
                evidence: explain
                  ? [evidence("canonical_id", v1.formatCanonicalId(q.category, q.uuid))]
                  : [],
              },
            ]
          : [],
      );
    case "identifier": {
      const out: Match[] = [];
      for (const uuid of await nodesByIdentifier(sql, q.scheme, q.value)) {
        out.push({
          uuid,
          evidence: explain ? await identifierEvidence(sql, uuid, q.scheme, q.value) : [],
        });
      }
      return plain(out);
    }
    case "chain": {
      const text = `${q.caip2.namespace}:${q.caip2.reference}`;
      return plain(
        (await chainsByCaip2(sql, q.caip2)).map((uuid) => ({
          uuid,
          evidence: explain ? [evidence("identifier", text, { namespace: "caip2" })] : [],
        })),
      );
    }
    case "deployment": {
      const { chain, assetNamespace, assetReference } = q.caip19;
      const text = `${chain.namespace}:${chain.reference}/${assetNamespace}:${assetReference}`;
      return plain(
        (await deploymentsByCaip19(sql, q.caip19)).map((uuid) => ({
          uuid,
          evidence: explain ? [evidence("identifier", text, { namespace: "caip19" })] : [],
        })),
      );
    }
    case "alias": {
      const [byAlias, byShare, byIdentifier, byCik] = [
        await nodesByAlias(sql, q.text, false),
        await equitiesByClassShare(sql, q.text),
        await nodesByIdentifier(sql, null, q.text.toUpperCase()),
        await nodesByIdentifier(sql, "cik", normalizeIdentifier("cik", q.text)),
      ];
      const ids = new Set([...byAlias, ...byShare, ...byIdentifier, ...byCik]);
      const out: Match[] = [];
      for (const uuid of ids) {
        const found: Evidence[] = [];
        if (explain) {
          if (byAlias.includes(uuid))
            found.push(...(await aliasEvidence(sql, uuid, q.text, false, null)));
          if (byShare.includes(uuid)) {
            found.push(
              evidence("class_share_symbol", classShareSymbol(q.text) ?? q.text, {
                namespace: "symbol",
              }),
            );
          }
          if (byIdentifier.includes(uuid)) {
            found.push(...(await identifierEvidence(sql, uuid, null, q.text.toUpperCase())));
          }
          if (byCik.includes(uuid) && !byIdentifier.includes(uuid)) {
            found.push(
              ...(await identifierEvidence(sql, uuid, "cik", normalizeIdentifier("cik", q.text))),
            );
          }
        }
        out.push({ uuid, evidence: found });
      }
      return plain(out);
    }
    case "pair": {
      const [base, quote] = [await pairSide(sql, q.base), await pairSide(sql, q.quote)];
      const out: Match[] = [];
      for (const subject of base) {
        for (const unit of quote) {
          if (subject === unit) continue;
          const unitCategory = (await categoryOf(sql, unit)) ?? "";
          const sides = explain
            ? [
                ...(await sideEvidence(sql, subject, q.base, "base")),
                ...(await sideEvidence(sql, unit, q.quote, "quote")),
              ]
            : [];
          // Two currencies name an FX market (`EUR/USD`: base EUR, quote
          // USD), in exactly that orientation, never its inverse.
          if (unitCategory === "currency" && (await categoryOf(sql, subject)) === "currency") {
            const fx = await sql<{ id: string; name: string }[]>`
              SELECT id::text, name::text FROM instruments
              WHERE base_currency_id = ${subject} AND quote_currency_id = ${unit}`;
            for (const m of fx) {
              out.push({
                subject: m.id,
                unit,
                unitCategory,
                evidence: explain ? [...sides, evidence("fx_pair", m.name)] : [],
              });
            }
            continue;
          }
          out.push({ subject, unit, unitCategory, evidence: sides });
        }
      }
      // A symbol shared across asset classes (a crypto asset and a stock):
      // keep only the combinations something actually quotes.
      if (out.length > 1) {
        const quoted: typeof out = [];
        for (const m of out) {
          if (!("subject" in m)) continue;
          const rows = await sql`
            SELECT 1 FROM quote_feeds WHERE subject_id = ${m.subject} AND unit_id = ${m.unit} LIMIT 1`;
          if (rows.length > 0) quoted.push(m);
        }
        return { found: quoted, quotedPairsOnly: true };
      }
      return plain(out);
    }
    case "venue_symbol": {
      // Venues named by symbol alias or MIC; feed sources named by source id.
      const venueRows = await sql<{ id: string }[]>`
        SELECT DISTINCT n.id::text AS id FROM nodes n
        WHERE n.category = 'venue' AND (
          n.id IN (SELECT node_id FROM aliases WHERE alias_key = lower(${q.venue}) AND kind = 'symbol')
          OR n.id IN (SELECT node_id FROM identifiers WHERE scheme = 'mic'
                      AND value = ${q.venue.toUpperCase()} AND valid_during @> now()))`;
      const venues = venueRows.map((r) => r.id);
      const symbols = [q.symbol, classShareSymbol(q.symbol) ?? q.symbol];
      const listingRows = await sql<{ id: string }[]>`
        SELECT DISTINCT l.instrument_id::text AS id
        FROM listing_symbols s JOIN listings l ON l.id = s.listing_id
        WHERE s.venue_id = ANY(${venues}::uuid[])
          AND s.symbol = ANY(${symbols}::text[])
          AND s.valid_during @> now()`;
      const feedRows = await sql<{ subject: string; unit: string; unit_category: string }[]>`
        SELECT DISTINCT subject_id::text AS subject, unit_id::text AS unit, unit_category
        FROM quote_feeds
        WHERE symbol = ${q.symbol}
          AND (venue_id = ANY(${venues}::uuid[]) OR feed_source_id = ${q.venue.toLowerCase()})`;
      const out: Match[] = [];
      for (const r of listingRows) {
        const found: Evidence[] = [];
        if (explain) {
          const rows = await sql<{ symbol: string; venue: string }[]>`
            SELECT s.symbol, s.venue_id::text AS venue
            FROM listing_symbols s JOIN listings l ON l.id = s.listing_id
            WHERE l.instrument_id = ${r.id} AND s.venue_id = ANY(${venues}::uuid[])
              AND s.symbol = ANY(${symbols}::text[]) AND s.valid_during @> now()
            ORDER BY s.id`;
          for (const row of rows) {
            found.push(
              evidence("listing_symbol", row.symbol, { venue: await nodeRefOf(sql, row.venue) }),
            );
          }
        }
        out.push({ uuid: r.id, evidence: found });
      }
      for (const r of feedRows) {
        const found: Evidence[] = [];
        if (explain) {
          const rows = await sql<{ symbol: string; venue: string | null; source: string }[]>`
            SELECT symbol, venue_id::text AS venue, feed_source_id AS source FROM quote_feeds
            WHERE subject_id = ${r.subject} AND unit_id = ${r.unit} AND symbol = ${q.symbol}
              AND (venue_id = ANY(${venues}::uuid[]) OR feed_source_id = ${q.venue.toLowerCase()})
            ORDER BY id`;
          for (const row of rows) {
            found.push(
              evidence("feed_symbol", row.symbol, {
                venue: row.venue === null ? null : await nodeRefOf(sql, row.venue),
                source: { id: row.source as v1.SourceId },
              }),
            );
          }
        }
        out.push({
          subject: r.subject,
          unit: r.unit,
          unitCategory: r.unit_category,
          evidence: found,
        });
      }
      return plain(out);
    }
  }
}

const METHOD: Record<ParsedQuery["kind"], Method> = {
  canonical_id: "canonical_id",
  identifier: "identifier",
  chain: "identifier",
  deployment: "identifier",
  venue_symbol: "venue_symbol",
  pair: "pair",
  alias: "alias",
};

type Assembled = Resolved & { evidence: Evidence[][]; quotedPairsOnly: boolean };

async function assemble(sql: Sql, q: ParsedQuery, explain: boolean): Promise<Assembled> {
  const { found, quotedPairsOnly } = await matches(sql, q, explain);
  const resolutions: Resolution[] = [];
  const pairs: Resolved["pairs"] = [];
  const evidenceOf: Evidence[][] = [];
  let method = METHOD[q.kind];
  for (const m of found) {
    if ("uuid" in m) {
      const node = await nodeRefOf(sql, m.uuid);
      if (node === null) continue;
      resolutions.push({ kind: "node", node });
      pairs.push(null);
    } else {
      if (q.kind === "venue_symbol") method = "feed_symbol";
      resolutions.push({
        kind: "pair",
        subject: await subjectOf(sql, m.subject),
        unit: await unitOf(sql, m.unit, m.unitCategory),
      });
      pairs.push({ subject: m.subject, unit: m.unit, unitCategory: m.unitCategory });
    }
    evidenceOf.push(m.evidence);
  }
  const status =
    resolutions.length === 0 ? "not_found" : resolutions.length === 1 ? "resolved" : "ambiguous";
  return {
    status,
    method: status === "not_found" ? null : method,
    resolutions,
    pairs,
    evidence: evidenceOf,
    quotedPairsOnly,
  };
}

export async function resolveQuery(sql: Sql, raw: string): Promise<Resolved | null> {
  const q = parseQuery(raw);
  if (q === null) return null;
  const { status, method, resolutions, pairs } = await assemble(sql, q, false);
  return { status, method, resolutions, pairs };
}

export function resolveResult(query: string, r: Resolved): v1.ResolveResultV1 {
  return v1.ResolveResultV1.parse({
    schemaVersion: 1,
    query,
    status: r.status,
    method: r.method,
    match: r.status === "resolved" ? (r.resolutions[0] ?? null) : null,
    candidates: r.status === "ambiguous" ? r.resolutions : [],
  });
}

// --- explain ------------------------------------------------------------------

/** A node's current external identifiers, including chain and asset ids. */
async function identifiersOf(sql: Sql, uuid: string) {
  const rows = await sql<{ namespace: string; value: string }[]>`
    SELECT scheme AS namespace, value FROM identifiers
    WHERE node_id = ${uuid} AND valid_during @> now()
    UNION ALL
    SELECT 'caip2', caip2_namespace || ':' || caip2_reference FROM chains WHERE id = ${uuid}
    UNION ALL
    SELECT 'caip19', c.caip2_namespace || ':' || c.caip2_reference || '/'
                     || d.asset_namespace || ':' || d.asset_reference
    FROM deployments d JOIN chains c ON c.id = d.chain_id WHERE d.id = ${uuid}
    ORDER BY namespace, value`;
  return rows.map((r) => ({ namespace: r.namespace, value: r.value }));
}

/**
 * A node's outgoing edges (canonical direction, with provenance), then the
 * projections: `LISTED_ON` for an instrument's listings, `DEPLOYED_ON` for a
 * deployment's chain, and `TRACKED_BY` for each instrument that TRACKS the
 * node (the stored edge read from its object's side, with its provenance).
 */
async function relationshipsOf(sql: Sql, uuid: string) {
  const rows = await sql.unsafe<
    { type: string; object: string; projected: boolean; source_id: string; received_at: string }[]
  >(
    `SELECT type, object, projected, source_id, ${tsText("received_at")} AS received_at FROM (
       SELECT 0 AS part, id AS ord, relationship_type AS type, object_id::text AS object,
              false AS projected, source_id, received_at
       FROM graph_edges WHERE subject_id = $1
       UNION ALL
       SELECT 1, row_number() OVER (ORDER BY id), 'LISTED_ON', venue_id::text, true,
              source_id, received_at
       FROM listings WHERE instrument_id = $1
       UNION ALL
       SELECT 2, 0, 'DEPLOYED_ON', d.chain_id::text, true, r.source_id, r.received_at
       FROM deployments d JOIN source_records r ON r.id = d.source_record_id WHERE d.id = $1
       UNION ALL
       SELECT 3, id, 'TRACKED_BY', subject_id::text, true, source_id, received_at
       FROM graph_edges WHERE object_id = $1 AND relationship_type = 'TRACKS'
     ) x ORDER BY part, ord`,
    [uuid],
  );
  const refs = await nodeRefs(
    sql,
    rows.map((r) => r.object),
  );
  return rows.map((r) => {
    const object = refs.get(r.object);
    if (object === undefined) throw new Error(`unknown node ${r.object}`);
    return {
      relationshipType: r.type,
      object,
      projected: r.projected,
      provenance: { sourceId: r.source_id, receivedAt: canonicalTimestamp(r.received_at) },
    };
  });
}

/**
 * `/v1/explain`: the resolver's own result for `raw`, with the evidence of
 * every candidate and a compact description of what each candidate is.
 * `null` for an invalid query (as `/v1/resolve`).
 */
export async function explain(sql: Sql, raw: string): Promise<v1.ExplainV1 | null> {
  const q = parseQuery(raw);
  if (q === null) return null;
  const a = await assemble(sql, q, true);
  const candidates = [];
  for (const [i, resolution] of a.resolutions.entries()) {
    const pair = a.pairs[i] ?? null;
    const node =
      pair === null && resolution.kind === "node"
        ? v1.canonicalIdUuid(resolution.node.id)
        : (pair?.subject ?? null);
    if (node === null) throw new Error("candidate without a node");
    const quoted =
      pair === null
        ? null
        : (
            await sql`
            SELECT 1 FROM quote_feeds WHERE subject_id = ${pair.subject} AND unit_id = ${pair.unit}
            LIMIT 1`
          ).length > 0;
    candidates.push({
      resolution,
      matches: a.evidence[i] ?? [],
      identifiers: await identifiersOf(sql, node),
      relationships: await relationshipsOf(sql, node),
      quoted,
    });
  }
  const parsedAs = q.kind === "chain" || q.kind === "deployment" ? ("identifier" as const) : q.kind;
  return v1.ExplainV1.parse({
    schemaVersion: 1,
    query: raw,
    parsedAs,
    status: a.status,
    method: a.method,
    quotedPairsOnly: a.quotedPairsOnly,
    candidates,
  });
}

// --- quotes -------------------------------------------------------------------

export type Pair = { subject: string; unit: string; unitCategory: string };

/**
 * The priced (subject, unit) pairs a resolution refers to: the pair itself,
 * or every pair with a canonical quote whose subject is a resolved node.
 */
export async function pricedPairs(sql: Sql, r: Resolved): Promise<Pair[]> {
  const out: Pair[] = [];
  const nodes: string[] = [];
  r.resolutions.forEach((res, i) => {
    const pair = r.pairs[i];
    if (res.kind === "pair" && pair) out.push(pair);
    if (res.kind === "node") {
      const uuid = v1.canonicalIdUuid(res.node.id);
      if (uuid) nodes.push(uuid);
    }
  });
  if (nodes.length > 0) {
    const rows = await sql<{ subject: string; unit: string; unit_category: string }[]>`
      SELECT subject_id::text AS subject, unit_id::text AS unit, unit_category
      FROM canonical_quotes WHERE subject_id = ANY(${nodes}::uuid[])
      ORDER BY subject_id, unit_id`;
    for (const row of rows) {
      out.push({ subject: row.subject, unit: row.unit, unitCategory: row.unit_category });
    }
  }
  return out;
}

type ObservationRow = {
  id: string;
  subject_id: string;
  unit_id: string;
  unit_category: string;
  basis: v1.ObservationV1["basis"];
  venue_id: string | null;
  price_type: v1.PriceType;
  price: string;
  bid: string | null;
  ask: string | null;
  source_id: string;
  observed_at: string | null;
  received_at: string;
  source_record_id: string;
  record_key: string;
};

const OBSERVATION_COLUMNS = `o.id::text, o.subject_id::text, o.unit_id::text, o.unit_category,
  o.basis, o.venue_id::text, o.price_type, o.price::text, o.bid::text, o.ask::text, o.source_id,
  ${tsText("o.observed_at")} AS observed_at, ${tsText("o.received_at")} AS received_at,
  o.source_record_id::text, r.record_key`;

/** Mirrors `undrly_core::quote::MEAN_VENUE_MID_MAX_AGE_SECONDS`. */
export const MEAN_VENUE_MID_MAX_AGE_SECONDS = 30;

/** The pair's declared aggregation method (default `latest-observation-v1`). */
async function methodOf(sql: Sql, pair: Pair): Promise<string> {
  const rows = await sql<{ method: string }[]>`
    SELECT method FROM quote_aggregations WHERE subject_id = ${pair.subject} AND unit_id = ${pair.unit}`;
  return rows[0]?.method ?? "latest-observation-v1";
}

/** A freshness policy: how long, counted on which clock. */
type Window = { seconds: number; clock: FreshnessClock };

/**
 * Freshness window of one observation: the method's own for
 * `mean-venue-mid-v1`, else the cadence its feed declares
 * (`quote_feeds.stale_after_seconds` on `freshness_clock`; 300 s continuous
 * for every V1 feed), else the API's configured default.
 */
async function windowOf(
  sql: Sql,
  method: string,
  row: ObservationRow,
  staleAfterSeconds: number,
): Promise<Window> {
  if (method === "mean-venue-mid-v1") {
    return { seconds: MEAN_VENUE_MID_MAX_AGE_SECONDS, clock: "continuous" };
  }
  const feeds = await sql<{ seconds: number; clock: FreshnessClock }[]>`
    SELECT stale_after_seconds AS seconds, freshness_clock AS clock FROM quote_feeds
    WHERE feed_source_id = ${row.source_id} AND subject_id = ${row.subject_id}
      AND unit_id = ${row.unit_id} AND price_type = ${row.price_type}
      AND venue_id IS NOT DISTINCT FROM ${row.venue_id}::uuid
    ORDER BY id LIMIT 1`;
  return feeds[0] ?? { seconds: staleAfterSeconds, clock: "continuous" };
}

/**
 * Undrly's policy verdict: `fresh` while the time counted on the window's
 * clock since `asOf` is within it. (`ageMs` is always literal elapsed time.)
 */
function freshness(asOf: string, now: Date, window: Window): "fresh" | "stale" {
  const elapsed = policyElapsedMs(new Date(Date.parse(asOf)), now, window.clock);
  return elapsed / 1000 <= window.seconds ? "fresh" : "stale";
}

const CONTINUOUS_30S: Window = {
  seconds: MEAN_VENUE_MID_MAX_AGE_SECONDS,
  clock: "continuous",
};

export async function venueRef(sql: Sql, uuid: string | null) {
  const venue = uuid === null ? null : await nodeRefOf(sql, uuid);
  return venue === null ? null : { id: venue.id, name: venue.name };
}

async function observationFields(sql: Sql, row: ObservationRow) {
  return {
    schemaVersion: 1 as const,
    subject: await subjectOf(sql, row.subject_id),
    unit: await unitOf(sql, row.unit_id, row.unit_category),
    priceType: row.price_type,
    price: row.price,
    bid: row.bid,
    ask: row.ask,
    basis: row.basis,
    venue: await venueRef(sql, row.venue_id),
    observedAt: row.observed_at === null ? null : canonicalTimestamp(row.observed_at),
    receivedAt: canonicalTimestamp(row.received_at),
  };
}

export type CanonicalResult =
  | { kind: "quote"; quote: v1.QuoteV1 }
  | { kind: "none" }
  /** A multi-source aggregate older than its method's window: not served. */
  | { kind: "stale"; asOf: string };

/**
 * The canonical quote of a pair, from `canonical_quotes` and exactly its
 * inputs. The inputs (observations, venues, sources, raw records) stay in
 * storage for audit and are not served. `latest-observation-v1` quotes are
 * their single input observation (flagged fresh/stale). A
 * `mean-venue-mid-v1` aggregate carries the mean bid and mean ask of its
 * inputs, as computed by the aggregator; one older than its window is not
 * served at all. `spread` and `ageMs` are computed per response, from
 * `now`.
 */
export async function canonicalQuote(
  sql: Sql,
  pair: Pair,
  now: Date,
  staleAfterSeconds: number,
): Promise<CanonicalResult> {
  const rows = await sql.unsafe<
    {
      method: v1.QuoteV1["aggregation"]["method"];
      price: string;
      bid: string | null;
      ask: string | null;
      price_type: v1.PriceType;
      eligible: number;
      as_of: string;
      computed_at: string;
    }[]
  >(
    `SELECT method, price::text, bid::text, ask::text, price_type, eligible_count AS eligible,
            ${tsText("as_of")} AS as_of, ${tsText("computed_at")} AS computed_at
     FROM canonical_quotes WHERE subject_id = $1 AND unit_id = $2`,
    [pair.subject, pair.unit],
  );
  const row = rows[0];
  if (row === undefined) return { kind: "none" };
  const inputRows = await sql.unsafe<ObservationRow[]>(
    `SELECT ${OBSERVATION_COLUMNS}
     FROM canonical_quote_inputs i
     JOIN market_observations o ON o.id = i.observation_id
     JOIN source_records r ON r.id = o.source_record_id
     WHERE i.subject_id = $1 AND i.unit_id = $2 ORDER BY o.id`,
    [pair.subject, pair.unit],
  );
  const aggregation = {
    method: row.method,
    eligibleObservations: row.eligible,
    computedAt: canonicalTimestamp(row.computed_at),
  };
  const priced = (
    q: { price: string; bid: string | null; ask: string | null },
    asOf: string,
    window: Window,
  ) => ({
    ...v1.spreadOf(q.price, q.bid, q.ask),
    ageMs: ageMs(asOf, now),
    freshness: freshness(asOf, now, window),
  });

  if (row.method === "latest-observation-v1") {
    const only = inputRows[0];
    if (only === undefined) return { kind: "none" };
    const fields = await observationFields(sql, only);
    const asOf = fields.observedAt ?? fields.receivedAt;
    const window = await windowOf(sql, row.method, only, staleAfterSeconds);
    const derived = priced(fields, asOf, window);
    const common = {
      schemaVersion: 1,
      subject: fields.subject,
      unit: fields.unit,
      priceType: fields.priceType,
      price: fields.price,
      bid: fields.bid,
      ask: fields.ask,
      spread: derived.spread,
      spreadBps: derived.spreadBps,
    };
    const timing = {
      receivedAt: fields.receivedAt,
      asOf,
      ageMs: derived.ageMs,
      freshness: derived.freshness,
      aggregation,
    };
    const quote =
      fields.basis === "venue"
        ? {
            ...common,
            basis: "venue",
            venue: fields.venue,
            observedAt: fields.observedAt,
            ...timing,
          }
        : { ...common, basis: fields.basis, ...timing };
    return { kind: "quote", quote: v1.QuoteV1.parse(quote) };
  }

  if (row.method === "mark-with-venue-book-v1") {
    // The mark is the price; the book (if used) only gave bid and ask.
    const markRow = inputRows.find((r) => r.price_type === "mark");
    if (markRow === undefined) return { kind: "none" };
    const bookRow = inputRows.find((r) => r.price_type === "mid");
    const bookTime =
      row.bid === null || bookRow === undefined
        ? {}
        : { bidAskAsOf: canonicalTimestamp(bookRow.observed_at ?? bookRow.received_at) };
    const mark = await observationFields(sql, markRow);
    const asOf = canonicalTimestamp(row.as_of);
    const window = await windowOf(sql, row.method, markRow, staleAfterSeconds);
    const q = { price: row.price, bid: row.bid, ask: row.ask };
    const derived = priced(q, asOf, window);
    const latestReceipt = inputRows
      .map((r) => canonicalTimestamp(r.received_at))
      .sort((a, b) => Date.parse(a) - Date.parse(b))
      .at(-1);
    return {
      kind: "quote",
      quote: v1.QuoteV1.parse({
        schemaVersion: 1,
        subject: mark.subject,
        unit: mark.unit,
        priceType: row.price_type,
        ...q,
        spread: derived.spread,
        spreadBps: derived.spreadBps,
        basis: "venue",
        venue: mark.venue,
        observedAt: mark.observedAt,
        ...bookTime,
        receivedAt: latestReceipt,
        asOf,
        ageMs: derived.ageMs,
        freshness: derived.freshness,
        aggregation,
      }),
    };
  }

  const asOf = canonicalTimestamp(row.as_of);
  if (freshness(asOf, now, CONTINUOUS_30S) === "stale") {
    return { kind: "stale", asOf };
  }
  const latestReceipt = inputRows
    .map((r) => canonicalTimestamp(r.received_at))
    .sort((a, b) => Date.parse(a) - Date.parse(b))
    .at(-1);
  const subject = await subjectOf(sql, pair.subject);
  // The mean bid and mean ask of the same inputs (not a best bid/offer).
  const q = { priceType: row.price_type, price: row.price, bid: row.bid, ask: row.ask };
  const derived = priced(q, asOf, CONTINUOUS_30S);
  return {
    kind: "quote",
    quote: v1.QuoteV1.parse({
      schemaVersion: 1,
      subject,
      unit: await unitOf(sql, pair.unit, pair.unitCategory),
      ...q,
      spread: derived.spread,
      spreadBps: derived.spreadBps,
      basis: "aggregated",
      receivedAt: latestReceipt,
      asOf,
      ageMs: derived.ageMs,
      freshness: derived.freshness,
      aggregation,
    }),
  };
}

/**
 * The latest observation of each feed (source, venue, price type) for a
 * pair, each with its raw-record provenance and read-time freshness.
 */
export async function feedObservations(
  sql: Sql,
  pair: Pair,
  now: Date,
  staleAfterSeconds: number,
): Promise<v1.ObservationV1[]> {
  const method = await methodOf(sql, pair);
  const rows = await sql.unsafe<ObservationRow[]>(
    `SELECT ${OBSERVATION_COLUMNS} FROM (
       SELECT DISTINCT ON (source_id, venue_id, price_type) *
       FROM market_observations WHERE subject_id = $1 AND unit_id = $2
       ORDER BY source_id, venue_id, price_type,
                COALESCE(observed_at, received_at) DESC, received_at DESC, id DESC
     ) o JOIN source_records r ON r.id = o.source_record_id
     ORDER BY o.source_id, o.id`,
    [pair.subject, pair.unit],
  );
  const out: v1.ObservationV1[] = [];
  for (const row of rows) {
    const fields = await observationFields(sql, row);
    const window = await windowOf(sql, method, row, staleAfterSeconds);
    out.push(
      v1.ObservationV1.parse({
        ...fields,
        observationId: row.id,
        source: { id: row.source_id },
        sourceRecord: { id: row.source_record_id, key: row.record_key },
        freshness: freshness(fields.observedAt ?? fields.receivedAt, now, window),
      }),
    );
  }
  return out;
}

// --- search -------------------------------------------------------------------

export async function search(sql: Sql, raw: string, limit = 20): Promise<v1.SearchResultV1> {
  const q = raw.trim();
  const escaped = q.replace(/[\\%_]/g, (c) => `\\${c}`).toLowerCase();
  const rows = await sql.unsafe<(NameRow & { matched: string; rank: number })[]>(
    `WITH terms AS (
       SELECT node_id AS id, alias::text AS text, kind FROM aliases
       UNION ALL SELECT id, name, 'name' FROM ${NAMES} n
     ), ranked AS (
       SELECT id, text, CASE
         WHEN lower(text) = $1 AND kind = 'symbol' THEN 0
         WHEN lower(text) = $1 THEN 1
         WHEN lower(text) LIKE $2 || '%' THEN 2
         WHEN lower(text) LIKE '%' || $2 || '%' THEN 3 END AS rank
       FROM terms
     ), best AS (
       SELECT DISTINCT ON (id) id, text, rank FROM ranked WHERE rank IS NOT NULL
       ORDER BY id, rank, text
     )
     SELECT n.id::text, n.kind, n.name, n.class, b.text AS matched, b.rank
     FROM best b JOIN ${NAMES} n ON n.id = b.id`,
    [q.toLowerCase(), escaped],
  );
  rows.sort(
    (a, b) =>
      a.rank - b.rank ||
      (KIND_ORDER[a.kind] ?? 9) - (KIND_ORDER[b.kind] ?? 9) ||
      a.name.localeCompare(b.name),
  );
  return v1.SearchResultV1.parse({
    schemaVersion: 1,
    query: raw,
    results: rows.slice(0, limit).map((r) => ({
      node: nodeRef(r),
      matched: r.matched,
      rank: v1.SEARCH_RANKS[r.rank],
    })),
  });
}

// --- graph --------------------------------------------------------------------

export async function graph(sql: Sql, uuid: string): Promise<v1.GraphV1 | null> {
  const root = await nodeRefOf(sql, uuid);
  if (root === null || root.kind !== "instrument") return null;
  const edges = await sql.unsafe<
    {
      subject: string;
      type: v1.RelationshipV1["relationshipType"];
      object: string;
      source_id: string;
      received_at: string;
    }[]
  >(
    `SELECT subject_id::text AS subject, relationship_type AS type, object_id::text AS object,
            source_id, ${tsText("received_at")} AS received_at
     FROM graph_edges WHERE subject_id = $1 OR object_id = $1 ORDER BY id`,
    [uuid],
  );
  const listings = await sql<{ id: string; venue: string; symbols: string[] }[]>`
    SELECT l.id::text, l.venue_id::text AS venue,
           COALESCE(array_agg(s.symbol ORDER BY s.id) FILTER (WHERE s.id IS NOT NULL), '{}') AS symbols
    FROM listings l LEFT JOIN listing_symbols s
      ON s.listing_id = l.id AND s.valid_during @> now()
    WHERE l.instrument_id = ${uuid} GROUP BY l.id ORDER BY l.id`;
  // V1.4: deployments that represent the root, with chain and CAIP-19 id.
  const deployments = await sql<{ id: string; chain: string; caip19: string }[]>`
    SELECT DISTINCT d.id::text, d.chain_id::text AS chain,
           c.caip2_namespace || ':' || c.caip2_reference || '/'
             || d.asset_namespace || ':' || d.asset_reference AS caip19
    FROM graph_edges e
    JOIN deployments d ON d.id = e.subject_id
    JOIN chains c ON c.id = d.chain_id
    WHERE e.object_id = ${uuid} AND e.relationship_type = 'REPRESENTS'
    ORDER BY d.id::text`;
  // V1.4: the units the root is priced in by a declared feed, with the venues.
  const markets = await sql<{ unit: string; unit_category: string; venues: string[] }[]>`
    SELECT unit_id::text AS unit, unit_category,
           COALESCE(array_agg(DISTINCT venue_id::text) FILTER (WHERE venue_id IS NOT NULL), '{}')
             AS venues
    FROM quote_feeds WHERE subject_id = ${uuid}
    GROUP BY unit_id, unit_category ORDER BY unit_id`;
  const refs = await nodeRefs(sql, [
    ...edges.flatMap((e) => [e.subject, e.object]),
    ...listings.flatMap((l) => [l.id, l.venue]),
    ...deployments.map((d) => d.chain),
    ...markets.flatMap((m) => m.venues),
  ]);
  const ref = (id: string) => {
    const r = refs.get(id);
    if (r === undefined) throw new Error(`unknown node ${id}`);
    return r;
  };
  const marketList = [];
  for (const m of markets) {
    marketList.push({
      unit: await unitOf(sql, m.unit, m.unit_category),
      venues: [...m.venues].sort().map(ref),
    });
  }
  return v1.GraphV1.parse({
    schemaVersion: 1,
    root,
    edges: edges.map((e) => ({
      subject: ref(e.subject),
      relationshipType: e.type,
      object: ref(e.object),
      provenance: { sourceId: e.source_id, receivedAt: canonicalTimestamp(e.received_at) },
    })),
    listings: listings.map((l) => ({ id: ref(l.id).id, venue: ref(l.venue), symbols: l.symbols })),
    // Additive keys are omitted when empty, so earlier documents are unchanged.
    ...(deployments.length === 0
      ? {}
      : {
          deployments: deployments.map((d) => ({
            id: v1.formatCanonicalId("deployment", d.id),
            chain: ref(d.chain),
            caip19: d.caip19,
          })),
        }),
    ...(marketList.length === 0 ? {} : { markets: marketList }),
  });
}

// --- universes ----------------------------------------------------------------

const UNIVERSE_TEXT: Record<v1.UniverseKey, { name: string; description: string }> = {
  "crypto-top100": {
    name: "Crypto top 100 by market cap",
    description:
      "CoinGecko's top 100 assets by market capitalisation. Membership only: prices come from the Kraken and Coinbase feeds mapped through CoinGecko's exchange tickers.",
  },
  sp500: {
    name: "S&P 500 (via SPY holdings)",
    description: "SSGA SPY ETF holdings: a practical proxy, not the official S&P constituent file.",
  },
  nasdaq100: {
    name: "Nasdaq-100 (imported members)",
    description:
      "Nasdaq.com's Nasdaq-100 list, limited to members that are imported S&P 500 securities listed on Nasdaq; the others are skipped and reported by the importer.",
  },
  "hyperliquid-perps": {
    name: "Hyperliquid perpetuals",
    description: "Hyperliquid's live perpetual markets when the snapshot was taken.",
  },
  "fx-major": {
    name: "FX majors and crosses",
    description:
      "Undrly-curated liquid G10 FX pairs. Priced by live venue books (Kraken, Bitstamp) where they are liquid, otherwise by central-bank reference rates (ECB, Bank of Canada, Federal Reserve H.10); pairs without an approved direct source have no quote.",
  },
  "fx-southeast-asia": {
    name: "Southeast Asian FX",
    description:
      "Undrly-curated Southeast Asian FX pairs that have a trustworthy direct source: official reference rates (Bank Indonesia, Bank Negara Malaysia, Central Bank of Myanmar, Federal Reserve H.10, ECB), not executable market quotes.",
  },
};

type SnapshotRow = {
  id: string;
  key: v1.UniverseKey;
  as_of: string;
  source_id: string;
  record_id: string;
  record_key: string;
  members: number;
};

const LATEST_SNAPSHOTS = `
  SELECT DISTINCT ON (s.universe_key) s.id::text, s.universe_key AS key,
         ${tsText("s.as_of")} AS as_of, s.source_id, s.source_record_id::text AS record_id,
         r.record_key, (SELECT count(*)::int FROM universe_members m WHERE m.snapshot_id = s.id) AS members
  FROM universe_snapshots s JOIN source_records r ON r.id = s.source_record_id
  ORDER BY s.universe_key, s.as_of DESC, s.id DESC`;

function summary(row: SnapshotRow) {
  return {
    key: row.key,
    ...UNIVERSE_TEXT[row.key],
    source: { id: row.source_id },
    asOf: canonicalTimestamp(row.as_of),
    memberCount: row.members,
  };
}

/** Every universe with a snapshot, in key order. */
export async function universes(sql: Sql): Promise<v1.UniversesV1> {
  const rows = await sql.unsafe<SnapshotRow[]>(LATEST_SNAPSHOTS);
  return v1.UniversesV1.parse({ schemaVersion: 1, universes: rows.map(summary) });
}

/** The latest snapshot of `key` with its members, or `null`. */
export async function universe(sql: Sql, key: v1.UniverseKey): Promise<v1.UniverseV1 | null> {
  const rows = await sql.unsafe<SnapshotRow[]>(
    `SELECT * FROM (${LATEST_SNAPSHOTS}) latest WHERE key = $1`,
    [key],
  );
  const row = rows[0];
  if (row === undefined) return null;
  const members = await sql<{ node: string; rank: number | null; symbol: string | null }[]>`
    SELECT node_id::text AS node, rank, source_symbol AS symbol FROM universe_members
    WHERE snapshot_id = ${row.id}
    ORDER BY rank NULLS LAST, source_symbol, node_id`;
  const refs = await nodeRefs(
    sql,
    members.map((m) => m.node),
  );
  return v1.UniverseV1.parse({
    schemaVersion: 1,
    ...summary(row),
    sourceRecord: { id: row.record_id, key: row.record_key },
    members: members.map((m) => {
      const node = refs.get(m.node);
      if (node === undefined) throw new Error(`unknown node ${m.node}`);
      return { node, rank: m.rank, sourceSymbol: m.symbol };
    }),
  });
}
