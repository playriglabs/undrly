/**
 * Read-only queries behind the API. Every price comes from PostgreSQL
 * (`canonical_quotes` / `market_observations`), written by the Rust
 * collector; no request ever contacts an upstream source.
 */
import { v1 } from "@undrly/contracts";
import type postgres from "postgres";
import { ageMs, meanVenueMid } from "./market.ts";
import {
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
  const terms = await sql<{ multiplier: string | null; uom: v1.UnitOfMeasure | null }[]>`
    SELECT contract_multiplier::text AS multiplier, unit_of_measure AS uom
    FROM instruments WHERE id = ${uuid}`;
  const { multiplier, uom } = terms[0] ?? { multiplier: null, uom: null };
  return {
    id: node.id as unknown as v1.InstrumentId,
    kind: "instrument",
    class: node.class,
    name: node.name,
    // Additive fields: absent (not null) when unset, so V1 output is unchanged.
    ...(multiplier === null ? {} : { contractMultiplier: multiplier as v1.DecimalString }),
    ...(uom === null ? {} : { unitOfMeasure: uom }),
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

async function matches(sql: Sql, q: ParsedQuery): Promise<Match[]> {
  switch (q.kind) {
    case "canonical_id":
      return (await categoryOf(sql, q.uuid)) === q.category ? [{ uuid: q.uuid }] : [];
    case "identifier":
      return (await nodesByIdentifier(sql, q.scheme, q.value)).map((uuid) => ({ uuid }));
    case "alias": {
      const ids = new Set([
        ...(await nodesByAlias(sql, q.text, false)),
        ...(await equitiesByClassShare(sql, q.text)),
        ...(await nodesByIdentifier(sql, null, q.text.toUpperCase())),
        ...(await nodesByIdentifier(sql, "cik", normalizeIdentifier("cik", q.text))),
      ]);
      return [...ids].map((uuid) => ({ uuid }));
    }
    case "pair": {
      const [base, quote] = [await pairSide(sql, q.base), await pairSide(sql, q.quote)];
      const out: { subject: string; unit: string; unitCategory: string }[] = [];
      for (const subject of base) {
        for (const unit of quote) {
          if (subject === unit) continue;
          const unitCategory = (await categoryOf(sql, unit)) ?? "";
          out.push({ subject, unit, unitCategory });
        }
      }
      // A symbol shared across asset classes (a crypto asset and a stock):
      // keep only the combinations something actually quotes.
      if (out.length > 1) {
        const quoted: typeof out = [];
        for (const m of out) {
          const rows = await sql`
            SELECT 1 FROM quote_feeds WHERE subject_id = ${m.subject} AND unit_id = ${m.unit} LIMIT 1`;
          if (rows.length > 0) quoted.push(m);
        }
        return quoted;
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
        WHERE s.venue_id = ANY(${venues}::uuid[])
          AND s.symbol = ANY(${[q.symbol, classShareSymbol(q.symbol) ?? q.symbol]}::text[])
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

/**
 * Freshness window of one observation: the method's own for
 * `mean-venue-mid-v1`, else the cadence its feed declares
 * (`quote_feeds.stale_after_seconds`; 300 for every V1 feed), else the
 * API's configured default.
 */
async function windowSeconds(
  sql: Sql,
  method: string,
  row: ObservationRow,
  staleAfterSeconds: number,
): Promise<number> {
  if (method === "mean-venue-mid-v1") return MEAN_VENUE_MID_MAX_AGE_SECONDS;
  const feeds = await sql<{ seconds: number }[]>`
    SELECT stale_after_seconds AS seconds FROM quote_feeds
    WHERE feed_source_id = ${row.source_id} AND subject_id = ${row.subject_id}
      AND unit_id = ${row.unit_id} AND price_type = ${row.price_type}
      AND venue_id IS NOT DISTINCT FROM ${row.venue_id}::uuid
    ORDER BY id LIMIT 1`;
  return feeds[0]?.seconds ?? staleAfterSeconds;
}

function freshness(asOf: string, now: Date, window: number): "fresh" | "stale" {
  return (now.getTime() - Date.parse(asOf)) / 1000 <= window ? "fresh" : "stale";
}

async function venueRef(sql: Sql, uuid: string | null) {
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

/** The feed an observation came from: source, venue and price type. */
const feedKey = (o: { source_id: string; venue_id: string | null; price_type: string }) =>
  `${o.source_id}|${o.venue_id ?? ""}|${o.price_type}`;

/** Price types of continuously traded markets, which a 24-hour change suits. */
const CHANGE_24H_PRICE_TYPES: readonly string[] = ["last", "mid", "mark"];

type BaselineRow = {
  source_id: string;
  venue_id: string | null;
  price_type: string;
  basis: string;
  price: string;
  bid: string | null;
  ask: string | null;
  effective_at: string;
};

/**
 * The 24-hour change of a canonical quote, or `null` when no trustworthy
 * baseline exists. The baseline is the canonical quote the pair's own method
 * would have served as fresh at `τ = asOf - 24 h`, recomputed from stored
 * observations, and only if it comes from exactly the same feeds as the
 * current quote (never a single venue against an aggregate):
 *
 * - `latest-observation-v1`: the pair's latest observation at or before `τ`
 *   must be of the current feed and at most `window` seconds before `τ`;
 * - `mean-venue-mid-v1`: each feed's latest observation at or before `τ`
 *   that is a venue quote with bid and ask, at most 30 s before `τ`; their
 *   mean of mids, if those feeds are exactly the current inputs' feeds.
 *
 * Only for last/mid/mark prices of non-equity subjects: equities (session
 * closes) and reference/average series (daily to monthly) get `null`.
 */
async function change24h(
  sql: Sql,
  pair: Pair,
  quote: { method: string; priceType: string; price: string; asOf: string },
  subject: v1.PriceSubjectV1,
  inputs: ObservationRow[],
  window: number,
): Promise<{ absolute: string; percent: string; from: string; asOf: string } | null> {
  if (!CHANGE_24H_PRICE_TYPES.includes(quote.priceType)) return null;
  if (subject.kind === "instrument" && subject.class === "equity") return null;
  if (inputs.length === 0) return null;
  const rows = await sql.unsafe<BaselineRow[]>(
    `SELECT DISTINCT ON (${quote.method === "mean-venue-mid-v1" ? "source_id, venue_id, price_type" : "subject_id"})
            source_id, venue_id::text, price_type, basis, price::text, bid::text, ask::text,
            ${tsText("COALESCE(observed_at, received_at)")} AS effective_at
     FROM market_observations
     WHERE subject_id = $1 AND unit_id = $2
       AND COALESCE(observed_at, received_at) <= $3::timestamptz - interval '24 hours'
       AND COALESCE(observed_at, received_at)
           >= $3::timestamptz - interval '24 hours' - make_interval(secs => $4)
     ORDER BY ${quote.method === "mean-venue-mid-v1" ? "source_id, venue_id, price_type," : "subject_id,"}
              COALESCE(observed_at, received_at) DESC, received_at DESC, id DESC`,
    [pair.subject, pair.unit, quote.asOf, window],
  );
  const current = new Set(inputs.map(feedKey));
  let from: string | null;
  let asOf: string;
  if (quote.method === "mean-venue-mid-v1") {
    const eligible = rows.filter((r) => r.basis === "venue" && r.bid !== null && r.ask !== null);
    const feeds = new Set(eligible.map(feedKey));
    if (feeds.size !== current.size || [...current].some((k) => !feeds.has(k))) return null;
    from = meanVenueMid(eligible.map((r) => ({ bid: r.bid ?? "", ask: r.ask ?? "" })));
    asOf = eligible.map((r) => r.effective_at).sort()[0] ?? "";
  } else {
    const only = rows[0];
    if (only === undefined || rows.length !== 1 || !current.has(feedKey(only))) return null;
    from = only.price;
    asOf = only.effective_at;
  }
  if (from === null) return null;
  const change = v1.changeOf(quote.price, from);
  return change === null ? null : { ...change, from, asOf: canonicalTimestamp(asOf) };
}

/**
 * The canonical quote of a pair, from `canonical_quotes` and exactly its
 * inputs. The inputs (observations, venues, sources, raw records) stay in
 * storage for audit and are not served. `latest-observation-v1` quotes are
 * their single input observation (flagged fresh/stale). A
 * `mean-venue-mid-v1` aggregate carries the mean bid and mean ask of its
 * inputs, as computed by the aggregator; one older than its window is not
 * served at all. `spread`, `ageMs` and `change24h` are computed per
 * response, from `now`.
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
  const priced = async (
    subject: v1.PriceSubjectV1,
    q: { priceType: v1.PriceType; price: string; bid: string | null; ask: string | null },
    asOf: string,
    window: number,
  ) => ({
    ...v1.spreadOf(q.price, q.bid, q.ask),
    ageMs: ageMs(asOf, now),
    freshness: freshness(asOf, now, window),
    change24h: await change24h(
      sql,
      pair,
      { method: row.method, priceType: q.priceType, price: q.price, asOf },
      subject,
      inputRows,
      window,
    ),
  });

  if (row.method === "latest-observation-v1") {
    const only = inputRows[0];
    if (only === undefined) return { kind: "none" };
    const fields = await observationFields(sql, only);
    const asOf = fields.observedAt ?? fields.receivedAt;
    const window = await windowSeconds(sql, row.method, only, staleAfterSeconds);
    const derived = await priced(fields.subject, fields, asOf, window);
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
      change24h: derived.change24h,
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

  const asOf = canonicalTimestamp(row.as_of);
  if (freshness(asOf, now, MEAN_VENUE_MID_MAX_AGE_SECONDS) === "stale") {
    return { kind: "stale", asOf };
  }
  const latestReceipt = inputRows
    .map((r) => canonicalTimestamp(r.received_at))
    .sort((a, b) => Date.parse(a) - Date.parse(b))
    .at(-1);
  const subject = await subjectOf(sql, pair.subject);
  // The mean bid and mean ask of the same inputs (not a best bid/offer).
  const q = { priceType: row.price_type, price: row.price, bid: row.bid, ask: row.ask };
  const derived = await priced(subject, q, asOf, MEAN_VENUE_MID_MAX_AGE_SECONDS);
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
      change24h: derived.change24h,
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
    const window = await windowSeconds(sql, method, row, staleAfterSeconds);
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
