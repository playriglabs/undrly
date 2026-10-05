/**
 * API against a real PostgreSQL database: migrations applied from
 * `database/migrations/`, two BTC/USD venue observations (Kraken, Coinbase)
 * and their `mean-venue-mid-v1` aggregate seeded directly. `fetch` is stubbed
 * to fail: the API must never contact an upstream provider.
 *
 * Skipped without DATABASE_URL (the role needs CREATEDB), like the Rust
 * database tests.
 */
import { readdirSync, readFileSync } from "node:fs";
import { v1 } from "@undrly/contracts";
import postgres from "postgres";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";

const url = process.env["DATABASE_URL"];
const ROOT = new URL("../../../../", import.meta.url);
const universe = JSON.parse(readFileSync(new URL("data/demo/universe.json", ROOT), "utf8")) as {
  currencies: { key: string; id: string }[];
  venues: { key: string; id: string }[];
  instruments: { key: string; id: string }[];
};
const uuid = (key: string) => {
  const all = [...universe.currencies, ...universe.venues, ...universe.instruments];
  const id = all.find((o) => o.key === key)?.id;
  const u = id === undefined ? null : v1.canonicalIdUuid(id);
  if (u === null) throw new Error(`no curated key ${key}`);
  return u;
};
const fixture = (path: string) => readFileSync(new URL(`tests/fixtures/sources/${path}`, ROOT));

describe.skipIf(url === undefined)("API with a database", () => {
  const name = `undrly_api_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  const ids = { kraken: "", coinbase: "", krakenRecord: "", coinbaseRecord: "" };

  beforeAll(async () => {
    admin = postgres(url as string, { max: 1, onnotice: () => {} });
    await admin.unsafe(`CREATE DATABASE "${name}"`);
    const target = new URL(url as string);
    target.pathname = `/${name}`;
    sql = postgres(target.toString(), { max: 2, onnotice: () => {} });
    const dir = new URL("database/migrations/", ROOT);
    for (const file of readdirSync(dir).sort()) {
      await sql.unsafe(readFileSync(new URL(file, dir), "utf8")).simple();
    }

    const [btc, usd, kraken, coinbase] = [
      uuid("btc"),
      uuid("usd"),
      uuid("kraken"),
      uuid("coinbase"),
    ];
    await sql`INSERT INTO sources (id, name) VALUES
      ('undrly-curated', 'Curated'), ('kraken', 'Kraken'), ('coinbase', 'Coinbase Exchange')`;
    const record = async (source: string, key: string, payload: Buffer, at: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${key}, ${payload}, ${at}) RETURNING id::text`
      )[0]?.id ?? "";
    const curated = await record(
      "undrly-curated",
      "data/demo/universe.json",
      Buffer.from("{}"),
      "2026-09-25T00:00:00Z",
    );
    await sql`INSERT INTO nodes (id, category) VALUES
      (${btc}, 'instrument'), (${usd}, 'currency'), (${kraken}, 'venue'), (${coinbase}, 'venue')`;
    await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id)
      VALUES (${btc}, 'crypto_asset', 'Bitcoin', ${curated})`;
    await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${usd}, 'US Dollar', ${curated})`;
    await sql`INSERT INTO venues (id, name, source_record_id) VALUES
      (${kraken}, 'Kraken', ${curated}), (${coinbase}, 'Coinbase Exchange', ${curated})`;
    await sql`INSERT INTO identifiers
      (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
      VALUES ('iso4217', 'USD', ${usd}, 'currency', 'undrly-curated', '2026-09-25T00:00:00Z', ${curated})`;
    await sql`INSERT INTO aliases
      (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
      VALUES (${btc}, 'instrument', 'BTC', 'symbol', 'undrly-curated', '2026-09-25T00:00:00Z', ${curated})`;
    await sql`INSERT INTO quote_aggregations
      (subject_id, subject_category, unit_id, unit_category, method, source_id, received_at, source_record_id)
      VALUES (${btc}, 'instrument', ${usd}, 'currency', 'mean-venue-mid-v1', 'undrly-curated',
              '2026-09-25T00:00:00Z', ${curated})`;

    ids.krakenRecord = await record(
      "kraken",
      "https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD",
      fixture("kraken/ticker.json"),
      "2026-09-25T07:28:07Z",
    );
    ids.coinbaseRecord = await record(
      "coinbase",
      "https://api.exchange.coinbase.com/products/BTC-USD/book?level=1",
      fixture("coinbase/book-BTC-USD-level1.json"),
      "2026-09-25T07:28:07.1Z",
    );
    const observe = async (
      source: string,
      venue: string,
      type: string,
      price: string,
      bid: string,
      ask: string,
      observedAt: string | null,
      receivedAt: string,
      rec: string,
    ) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO market_observations
            (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask, unit_id,
             unit_category, source_id, observed_at, received_at, source_record_id)
          VALUES (${btc}, 'instrument', 'venue', ${venue}, ${type}, ${price}::numeric,
                  ${bid}::numeric, ${ask}::numeric, ${usd}, 'currency', ${source}, ${observedAt}::text::timestamptz,
                  ${receivedAt}, ${rec})
          RETURNING id::text`
      )[0]?.id ?? "";
    ids.kraken = await observe(
      "kraken",
      kraken,
      "last",
      "84143.10000",
      "84145.90000",
      "84146.00000",
      null,
      "2026-09-25T07:28:07Z",
      ids.krakenRecord,
    );
    ids.coinbase = await observe(
      "coinbase",
      coinbase,
      "mid",
      "84006.455",
      "84006.45",
      "84006.46",
      "2026-09-25T07:28:06.386017Z",
      "2026-09-25T07:28:07.1Z",
      ids.coinbaseRecord,
    );
    await sql`INSERT INTO canonical_quotes
      (subject_id, subject_category, unit_id, unit_category, method, price, price_type, basis,
       as_of, eligible_count, computed_at, bid, ask)
      VALUES (${btc}, 'instrument', ${usd}, 'currency', 'mean-venue-mid-v1', 84076.2025000, 'mid',
              'aggregated', '2026-09-25T07:28:06.386017Z', 2, '2026-09-25T07:28:10Z',
              84076.1750000, 84076.2300000)`;
    await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
      VALUES (${btc}, ${usd}, ${ids.kraken}, 84145.950000), (${btc}, ${usd}, ${ids.coinbase}, 84006.455)`;

    globalThis.fetch = (async () => {
      fetchCalls++;
      throw new Error("the API must not contact upstream providers");
    }) as unknown as typeof fetch;
  });

  afterAll(async () => {
    globalThis.fetch = realFetch;
    await sql?.end();
    await admin?.unsafe(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`);
    await admin?.end();
  });

  const app = (now: string) => createApp(sql, { staleAfterSeconds: 300, now: () => new Date(now) });

  it("/v1/quotes/BTC/USD exposes each venue's observation with provenance", async () => {
    const res = await app("2026-09-25T07:28:10Z").request("/v1/quotes/BTC/USD");
    expect(res.status).toBe(200);
    const body = v1.ObservationsV1.parse(await res.json());
    const bySource = Object.fromEntries(body.observations.map((o) => [o.source.id, o]));
    expect(Object.keys(bySource).sort()).toStrictEqual(["coinbase", "kraken"]);
    expect(bySource["kraken"]).toMatchObject({
      basis: "venue",
      venue: { name: "Kraken" },
      priceType: "last",
      price: "84143.10000",
      bid: "84145.90000",
      ask: "84146.00000",
      observedAt: null,
      observationId: ids.kraken,
      sourceRecord: { id: ids.krakenRecord },
      freshness: "fresh",
    });
    expect(bySource["coinbase"]).toMatchObject({
      venue: { name: "Coinbase Exchange" },
      priceType: "mid",
      observedAt: "2026-09-25T07:28:06.386017Z",
      sourceRecord: { id: ids.coinbaseRecord },
      freshness: "fresh",
    });
  });

  it("/v1/quote/BTC/USD is one aggregate attributed to no venue", async () => {
    const res = await app("2026-09-25T07:28:10Z").request("/v1/quote/BTC/USD");
    expect(res.status).toBe(200);
    const q = v1.QuoteV1.parse(await res.json());
    for (const key of ["venue", "source", "observedAt"]) expect(q).not.toHaveProperty(key);
    expect(q).toMatchObject({
      basis: "aggregated",
      priceType: "mid",
      price: "84076.2025000",
      bid: "84076.1750000",
      ask: "84076.2300000",
      asOf: "2026-09-25T07:28:06.386017Z",
      receivedAt: "2026-09-25T07:28:07.100Z",
      freshness: "fresh",
    });
    expect(q.aggregation.method).toBe("mean-venue-mid-v1");
    expect(q.aggregation.eligibleObservations).toBe(2);
    expect(q.aggregation.computedAt).toBe("2026-09-25T07:28:10Z");
  });

  it("/v1/quote exposes no per-input venue, source, observation or record", async () => {
    const res = await app("2026-09-25T07:28:10Z").request("/v1/quote/BTC/USD");
    const body = (await res.json()) as Record<string, unknown>;
    expect(Object.keys(body["aggregation"] as object).sort()).toStrictEqual([
      "computedAt",
      "eligibleObservations",
      "method",
    ]);
    const text = JSON.stringify(body);
    for (const leak of [
      "Kraken",
      "Coinbase",
      "kraken",
      "coinbase",
      ids.kraken,
      ids.coinbase,
      ids.krakenRecord,
      ids.coinbaseRecord,
    ]) {
      expect(text).not.toContain(`"${leak}"`);
    }
  });

  it("the aggregate's inputs stay stored with full provenance", async () => {
    const rows = await sql<
      { observation: string; input_price: string; source_id: string; record: string }[]
    >`SELECT i.observation_id::text AS observation, i.input_price::text, o.source_id,
             o.source_record_id::text AS record
      FROM canonical_quote_inputs i JOIN market_observations o ON o.id = i.observation_id
      WHERE i.subject_id = ${uuid("btc")} AND i.unit_id = ${uuid("usd")} ORDER BY o.id`;
    expect(rows.map((r) => [r.observation, r.source_id, r.input_price, r.record])).toStrictEqual([
      [ids.kraken, "kraken", "84145.950000", ids.krakenRecord],
      [ids.coinbase, "coinbase", "84006.455", ids.coinbaseRecord],
    ]);
  });

  it("an aggregate's bid and ask are the means of its inputs' own bids and asks", async () => {
    const at = app("2026-09-25T07:28:10Z");
    const q = v1.QuoteV1.parse(await (await at.request("/v1/quote/BTC/USD")).json());
    const obs = v1.ObservationsV1.parse(await (await at.request("/v1/quotes/BTC/USD")).json());
    const stored = await sql<{ id: string }[]>`
      SELECT observation_id::text AS id FROM canonical_quote_inputs
      WHERE subject_id = ${uuid("btc")} AND unit_id = ${uuid("usd")}`;
    const used = obs.observations.filter((o) => stored.some((i) => i.id === o.observationId));
    expect(used).toHaveLength(2);
    // Each venue observation keeps its own bid and ask.
    const own = used.map((o) => [o.source.id, o.bid, o.ask]).sort();
    expect(own).toStrictEqual([
      ["coinbase", "84006.45", "84006.46"],
      ["kraken", "84145.90000", "84146.00000"],
    ]);
    // (84145.90000 + 84006.45) / 2 and (84146.00000 + 84006.46) / 2.
    expect(v1.decimalCompare(q.bid ?? "", "84076.175")).toBe(0);
    expect(v1.decimalCompare(q.ask ?? "", "84076.23")).toBe(0);
    expect(v1.decimalCompare(q.bid ?? "", q.price)).toBeLessThanOrEqual(0);
    expect(v1.decimalCompare(q.price, q.ask ?? "")).toBeLessThanOrEqual(0);
  });

  it("an aggregate older than its window is not served; observations stay visible", async () => {
    // Within one venue sweep (120 s, V1.10) the aggregate is still served.
    const sweep = app("2026-09-25T07:29:00Z"); // as_of + 53.6 s
    const fresh = v1.QuoteV1.parse(await (await sweep.request("/v1/quote/BTC/USD")).json());
    expect(fresh.freshness).toBe("fresh");
    const later = app("2026-09-25T07:30:30Z"); // as_of + 143.6 s > 120 s
    const res = await later.request("/v1/quote/BTC/USD");
    expect(res.status).toBe(404);
    expect(v1.ErrorV1.parse(await res.json()).error.code).toBe("no_quote");
    const obs = v1.ObservationsV1.parse(await (await later.request("/v1/quotes/BTC/USD")).json());
    expect(obs.observations.map((o) => o.freshness)).toStrictEqual(["stale", "stale"]);
  });

  it("never contacted an upstream provider", () => {
    expect(fetchCalls).toBe(0);
  });
});

describe("API source", () => {
  it("contains no network client and no upstream host", () => {
    const dir = new URL("../src/", import.meta.url);
    for (const file of readdirSync(dir)) {
      const text = readFileSync(new URL(file, dir), "utf8");
      expect(text, file).not.toMatch(/\bfetch\(/);
      for (const host of [
        "kraken.com",
        "coinbase.com",
        "hyperliquid.xyz",
        "gold-api.com",
        "alpaca.markets",
      ]) {
        expect(text, file).not.toContain(host);
      }
    }
  });
});
