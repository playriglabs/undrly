/**
 * Read-only queries behind the API. Every price comes from PostgreSQL
 * (`canonical_quotes` / `market_observations`), written by the Rust
 * collector; no request ever contacts an upstream source.
 */
import { v1 } from "@undrly/contracts";
import type postgres from "postgres";
import {
  canonicalTimestamp,
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
};

// Every node with its display name, in one relation.
const NAMES = `(
  SELECT id, 'entity'::text AS kind, name::text AS name, NULL::text AS class FROM entities
  UNION ALL SELECT id, 'instrument', name, instrument_class FROM instruments
  UNION ALL SELECT id, 'venue', name, NULL FROM venues
  UNION ALL SELECT id, 'currency', name, NULL FROM currencies
  UNION ALL SELECT l.id, 'listing', i.name || ' on ' || v.name, NULL
    FROM listings l JOIN instruments i ON i.id = l.instrument_id JOIN venues v ON v.id = l.venue_id
)`;

const tsText = (column: string) =>
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

async function subjectOf(sql: Sql, uuid: string): Promise<v1.PriceSubjectV1> {
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
  return {
    id: node.id as unknown as v1.InstrumentId,
    kind: "instrument",
    class: node.class,
    name: node.name,
  };
}

async function unitOf(sql: Sql, uuid: string, category: string): Promise<v1.PriceUnitV1> {
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

type Match = { uuid: string } | { subject: string; unit: string; unitCategory: string };

async function nodesByAlias(sql: Sql, text: string, symbolsOnly: boolean): Promise<string[]> {
  const rows = await sql<{ id: string }[]>`
    SELECT DISTINCT node_id::text AS id FROM aliases
    WHERE alias_key = lower(${text}) AND (${!symbolsOnly} OR kind = 'symbol')`;
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

async function categoryOf(sql: Sql, uuid: string): Promise<string | null> {
  const rows = await sql<{ category: string }[]>`SELECT category FROM nodes WHERE id = ${uuid}`;
  return rows[0]?.category ?? null;
}

/** One side of a pair: exactly one instrument or currency, by symbol or ISO code. */
async function pairSide(sql: Sql, token: string): Promise<string[]> {
  const ids = new Set([
    ...(await nodesByAlias(sql, token, true)),
    ...(await nodesByIdentifier(sql, "iso4217", token.toUpperCase())),
  ]);
  const out: string[] = [];
  for (const id of ids) {
    const category = await categoryOf(sql, id);
    if (category === "instrument" || category === "currency") out.push(id);
  }
  return out;
}

async function matches(sql: Sql, q: ParsedQuery): Promise<Match[]> {
  switch (q.kind) {
    case "canonical_id":
      return (await categoryOf(sql, q.uuid)) === q.category ? [{ uuid: q.uuid }] : [];
    case "identifier":
      return (await nodesByIdentifier(sql, q.scheme, q.value)).map((uuid) => ({ uuid }));
    case "alias": {
      const ids = new Set([
        ...(await nodesByAlias(sql, q.text, false)),
        ...(await nodesByIdentifier(sql, null, q.text.toUpperCase())),
        ...(await nodesByIdentifier(sql, "cik", normalizeIdentifier("cik", q.text))),
      ]);
      return [...ids].map((uuid) => ({ uuid }));
    }
    case "pair": {
      const [base, quote] = [await pairSide(sql, q.base), await pairSide(sql, q.quote)];
      const out: Match[] = [];
      for (const subject of base) {
        for (const unit of quote) {
          if (subject === unit) continue;
          const unitCategory = (await categoryOf(sql, unit)) ?? "";
          out.push({ subject, unit, unitCategory });
        }
      }
      return out;
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
      const listingRows = await sql<{ id: string }[]>`
        SELECT DISTINCT l.instrument_id::text AS id
        FROM listing_symbols s JOIN listings l ON l.id = s.listing_id
        WHERE s.venue_id = ANY(${venues}::uuid[]) AND s.symbol = ${q.symbol}
          AND s.valid_during @> now()`;
      const feedRows = await sql<{ subject: string; unit: string; unit_category: string }[]>`
        SELECT DISTINCT subject_id::text AS subject, unit_id::text AS unit, unit_category
        FROM quote_feeds
        WHERE symbol = ${q.symbol}
          AND (venue_id = ANY(${venues}::uuid[]) OR feed_source_id = ${q.venue.toLowerCase()})`;
      return [
        ...listingRows.map((r) => ({ uuid: r.id })),
        ...feedRows.map((r) => ({
          subject: r.subject,
          unit: r.unit,
          unitCategory: r.unit_category,
        })),
      ];
    }
  }
}

const METHOD: Record<ParsedQuery["kind"], Method> = {
  canonical_id: "canonical_id",
  identifier: "identifier",
  venue_symbol: "venue_symbol",
  pair: "pair",
  alias: "alias",
};

export async function resolveQuery(sql: Sql, raw: string): Promise<Resolved | null> {
  const q = parseQuery(raw);
  if (q === null) return null;
  const found = await matches(sql, q);
  const resolutions: Resolution[] = [];
  const pairs: Resolved["pairs"] = [];
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
      pairs.push(m);
    }
  }
  const status =
    resolutions.length === 0 ? "not_found" : resolutions.length === 1 ? "resolved" : "ambiguous";
  return { status, method: status === "not_found" ? null : method, resolutions, pairs };
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

// --- quotes -------------------------------------------------------------------

type Pair = { subject: string; unit: string; unitCategory: string };

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
};

const OBSERVATION_COLUMNS = `o.id::text, o.subject_id::text, o.unit_id::text, o.unit_category,
  o.basis, o.venue_id::text, o.price_type, o.price::text, o.bid::text, o.ask::text, o.source_id,
  ${tsText("o.observed_at")} AS observed_at, ${tsText("o.received_at")} AS received_at`;

async function observationFields(sql: Sql, row: ObservationRow) {
  const venue = row.venue_id === null ? null : await nodeRefOf(sql, row.venue_id);
  return {
    schemaVersion: 1 as const,
    subject: await subjectOf(sql, row.subject_id),
    unit: await unitOf(sql, row.unit_id, row.unit_category),
    priceType: row.price_type,
    price: row.price,
    bid: row.bid,
    ask: row.ask,
    basis: row.basis,
    venue: venue === null ? null : { id: venue.id, name: venue.name },
    observedAt: row.observed_at === null ? null : canonicalTimestamp(row.observed_at),
    receivedAt: canonicalTimestamp(row.received_at),
    source: { id: row.source_id },
  };
}

export async function canonicalQuote(
  sql: Sql,
  pair: Pair,
  now: Date,
  staleAfterSeconds: number,
): Promise<v1.QuoteV1 | null> {
  const rows = await sql.unsafe<
    (ObservationRow & { method: string; eligible: number; computed_at: string })[]
  >(
    `SELECT ${OBSERVATION_COLUMNS}, c.method, c.eligible_count AS eligible,
            ${tsText("c.computed_at")} AS computed_at
     FROM canonical_quotes c JOIN market_observations o ON o.id = c.observation_id
     WHERE c.subject_id = $1 AND c.unit_id = $2`,
    [pair.subject, pair.unit],
  );
  const row = rows[0];
  if (row === undefined) return null;
  const fields = await observationFields(sql, row);
  const asOf = fields.observedAt ?? fields.receivedAt;
  const ageSeconds = (now.getTime() - Date.parse(asOf)) / 1000;
  return v1.QuoteV1.parse({
    ...fields,
    asOf,
    freshness: ageSeconds <= staleAfterSeconds ? "fresh" : "stale",
    aggregation: {
      method: row.method,
      eligibleObservations: row.eligible,
      computedAt: canonicalTimestamp(row.computed_at),
    },
  });
}

/** The latest observation of each feed (source, venue, price type) for a pair. */
export async function feedObservations(sql: Sql, pair: Pair): Promise<v1.ObservationV1[]> {
  const rows = await sql.unsafe<ObservationRow[]>(
    `SELECT ${OBSERVATION_COLUMNS} FROM (
       SELECT DISTINCT ON (source_id, venue_id, price_type) *
       FROM market_observations WHERE subject_id = $1 AND unit_id = $2
       ORDER BY source_id, venue_id, price_type,
                COALESCE(observed_at, received_at) DESC, received_at DESC, id DESC
     ) o ORDER BY o.source_id, o.id`,
    [pair.subject, pair.unit],
  );
  const out: v1.ObservationV1[] = [];
  for (const row of rows) out.push(v1.ObservationV1.parse(await observationFields(sql, row)));
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
  const refs = await nodeRefs(sql, [
    ...edges.flatMap((e) => [e.subject, e.object]),
    ...listings.flatMap((l) => [l.id, l.venue]),
  ]);
  const ref = (id: string) => {
    const r = refs.get(id);
    if (r === undefined) throw new Error(`unknown node ${id}`);
    return r;
  };
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
  });
}
