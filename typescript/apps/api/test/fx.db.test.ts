/**
 * V1.2 FX (docs/v1.2-fx.md) against a real PostgreSQL database: FX markets
 * are instruments (class `fx`) between currency nodes, seeded directly with
 * a two-venue EUR/USD aggregate and a USD/IDR reference rate on a weekday
 * freshness clock. `fetch` is stubbed to fail: no upstream is contacted.
 *
 * Skipped without DATABASE_URL (the role needs CREATEDB).
 */
import { readdirSync, readFileSync } from "node:fs";
import { v1 } from "@undrly/contracts";
import postgres from "postgres";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createApp } from "../src/app.ts";

const url = process.env["DATABASE_URL"];
const ROOT = new URL("../../../../", import.meta.url);
const uuid = () => {
  // UUIDv7 from the clock plus randomness (the nodes table requires v7).
  const hex = Date.now().toString(16).padStart(12, "0");
  const rand = crypto.randomUUID().replaceAll("-", "");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${rand.slice(0, 3)}-8${rand.slice(3, 6)}-${rand.slice(6, 18)}`;
};

describe.skipIf(url === undefined)("FX with a database", () => {
  const name = `undrly_fx_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  const id = {
    usd: uuid(),
    eur: uuid(),
    idr: uuid(),
    jpy: uuid(),
    eurUsd: uuid(),
    usdIdr: uuid(),
    usdJpy: uuid(),
    kraken: uuid(),
    bitstamp: uuid(),
    idrToken: uuid(),
  };

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
    const at = "2026-09-25T00:00:00Z";
    await sql`INSERT INTO sources (id, name) VALUES ('undrly-curated', 'Curated'),
      ('kraken', 'Kraken'), ('bitstamp', 'Bitstamp'), ('bank-indonesia', 'Bank Indonesia')`;
    const record = async (source: string, key: string, payload: string, receivedAt: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${key}, ${Buffer.from(payload)}, ${receivedAt}) RETURNING id::text`
      )[0]?.id ?? "";
    const curated = await record("undrly-curated", "data/reference/fx.json", "{}", at);
    const currencies: [string, string, string][] = [
      [id.usd, "USD", "US Dollar"],
      [id.eur, "EUR", "Euro"],
      [id.idr, "IDR", "Indonesian Rupiah"],
      [id.jpy, "JPY", "Japanese Yen"],
    ];
    for (const [cid, code, cname] of currencies) {
      await sql`INSERT INTO nodes (id, category) VALUES (${cid}, 'currency')`;
      await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${cid}, ${cname}, ${curated})`;
      await sql`INSERT INTO identifiers
        (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
        VALUES ('iso4217', ${code}, ${cid}, 'currency', 'undrly-curated', ${at}, ${curated})`;
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${cid}, 'currency', ${cname}, 'name', 'undrly-curated', ${at}, ${curated})`;
    }
    const markets: [string, string, string, string][] = [
      [id.eurUsd, "EUR/USD", id.eur, id.usd],
      [id.usdIdr, "USD/IDR", id.usd, id.idr],
      [id.usdJpy, "USD/JPY", id.usd, id.jpy],
    ];
    for (const [mid, mname, base, quote] of markets) {
      await sql`INSERT INTO nodes (id, category) VALUES (${mid}, 'instrument')`;
      await sql`INSERT INTO instruments
        (id, instrument_class, name, base_currency_id, quote_currency_id, source_record_id)
        VALUES (${mid}, 'fx', ${mname}, ${base}, ${quote}, ${curated})`;
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${mid}, 'instrument', ${mname.replace("/", "")}, 'symbol', 'undrly-curated', ${at}, ${curated})`;
    }
    // A crypto token whose symbol is also an ISO code: the code must not
    // steal its quote.
    await sql`INSERT INTO nodes (id, category) VALUES (${id.idrToken}, 'instrument')`;
    await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id)
      VALUES (${id.idrToken}, 'crypto_asset', 'IDR Token', ${curated})`;
    await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
      VALUES (${id.idrToken}, 'instrument', 'IDR', 'symbol', 'undrly-curated', ${at}, ${curated})`;
    for (const [vid, vname] of [
      [id.kraken, "Kraken"],
      [id.bitstamp, "Bitstamp"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${vid}, 'venue')`;
      await sql`INSERT INTO venues (id, name, source_record_id) VALUES (${vid}, ${vname}, ${curated})`;
    }
    const feed = async (
      source: string,
      symbol: string,
      subject: string,
      unit: string,
      venue: string | null,
      priceType: string,
      staleAfter: number,
      clock: string,
    ) =>
      sql`INSERT INTO quote_feeds
        (feed_source_id, symbol, subject_id, subject_category, unit_id, unit_category, basis,
         venue_id, price_type, source_id, received_at, source_record_id, stale_after_seconds,
         freshness_clock)
        VALUES (${source}, ${symbol}, ${subject}, 'instrument', ${unit}, 'currency',
                ${venue === null ? "aggregated" : "venue"}, ${venue}, ${priceType}, 'undrly-curated',
                ${at}, ${curated}, ${staleAfter}, ${clock})`;
    await feed("kraken", "ZEURZUSD", id.eurUsd, id.usd, id.kraken, "last", 300, "continuous");
    await feed("bitstamp", "eurusd", id.eurUsd, id.usd, id.bitstamp, "mid", 300, "continuous");
    await feed(
      "bank-indonesia",
      "JISDOR-USD",
      id.usdIdr,
      id.idr,
      null,
      "reference",
      259200,
      "weekdays",
    );
    await sql`INSERT INTO quote_aggregations
      (subject_id, subject_category, unit_id, unit_category, method, source_id, received_at, source_record_id)
      VALUES (${id.eurUsd}, 'instrument', ${id.usd}, 'currency', 'mean-venue-mid-v1',
              'undrly-curated', ${at}, ${curated})`;

    const observe = async (o: {
      source: string;
      subject: string;
      unit: string;
      venue: string | null;
      type: string;
      price: string;
      bid: string | null;
      ask: string | null;
      observedAt: string | null;
      receivedAt: string;
    }) => {
      const rec = await record(o.source, `${o.source}:${o.subject}`, "raw", o.receivedAt);
      return (
        (
          await sql<{ id: string }[]>`
          INSERT INTO market_observations
            (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask, unit_id,
             unit_category, source_id, observed_at, received_at, source_record_id)
          VALUES (${o.subject}, 'instrument', ${o.venue === null ? "aggregated" : "venue"}, ${o.venue},
                  ${o.type}, ${o.price}::numeric, ${o.bid}::numeric, ${o.ask}::numeric, ${o.unit},
                  'currency', ${o.source}, ${o.observedAt}::text::timestamptz, ${o.receivedAt}, ${rec})
          RETURNING id::text`
        )[0]?.id ?? ""
      );
    };
    const kraken = await observe({
      source: "kraken",
      subject: id.eurUsd,
      unit: id.usd,
      venue: id.kraken,
      type: "last",
      price: "1.13680",
      bid: "1.13679",
      ask: "1.13680",
      observedAt: null,
      receivedAt: "2026-09-25T10:00:05Z",
    });
    const bitstamp = await observe({
      source: "bitstamp",
      subject: id.eurUsd,
      unit: id.usd,
      venue: id.bitstamp,
      type: "mid",
      price: "1.138825",
      bid: "1.13882",
      ask: "1.13883",
      observedAt: "2026-09-25T10:00:00Z",
      receivedAt: "2026-09-25T10:00:01Z",
    });
    await sql`INSERT INTO canonical_quotes
      (subject_id, subject_category, unit_id, unit_category, method, price, price_type, basis,
       as_of, eligible_count, computed_at, bid, ask)
      VALUES (${id.eurUsd}, 'instrument', ${id.usd}, 'currency', 'mean-venue-mid-v1', 1.1378100,
              'mid', 'aggregated', '2026-09-25T10:00:00Z', 2, '2026-09-25T10:00:06Z',
              1.1378050, 1.1378150)`;
    await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
      VALUES (${id.eurUsd}, ${id.usd}, ${kraken}, 1.136795), (${id.eurUsd}, ${id.usd}, ${bitstamp}, 1.138825)`;
    // JISDOR for Friday 2026-09-25 (stated as 00:00 WIB).
    const jisdor = await observe({
      source: "bank-indonesia",
      subject: id.usdIdr,
      unit: id.idr,
      venue: null,
      type: "reference",
      price: "17917.00",
      bid: null,
      ask: null,
      observedAt: "2026-09-24T17:00:00Z",
      receivedAt: "2026-09-25T04:00:00Z",
    });
    await sql`INSERT INTO canonical_quotes
      (subject_id, subject_category, unit_id, unit_category, method, price, price_type, basis,
       as_of, eligible_count, computed_at)
      VALUES (${id.usdIdr}, 'instrument', ${id.idr}, 'currency', 'latest-observation-v1', 17917.00,
              'reference', 'aggregated', '2026-09-24T17:00:00Z', 1, '2026-09-25T04:00:01Z')`;
    await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
      VALUES (${id.usdIdr}, ${id.idr}, ${jisdor}, 17917.00)`;
    // The crypto token's own quote, in USD.
    await sql`INSERT INTO quote_feeds
        (feed_source_id, symbol, subject_id, subject_category, unit_id, unit_category, basis,
         venue_id, price_type, source_id, received_at, source_record_id)
        VALUES ('kraken', 'IDRUSD', ${id.idrToken}, 'instrument', ${id.usd}, 'currency', 'venue',
                ${id.kraken}, 'last', 'undrly-curated', ${at}, ${curated})`;
    const token = await observe({
      source: "kraken",
      subject: id.idrToken,
      unit: id.usd,
      venue: id.kraken,
      type: "last",
      price: "0.25",
      bid: "0.24",
      ask: "0.26",
      observedAt: null,
      receivedAt: "2026-09-25T10:00:05Z",
    });
    await sql`INSERT INTO canonical_quotes
      (subject_id, subject_category, unit_id, unit_category, method, price, price_type, basis,
       venue_id, as_of, eligible_count, computed_at, bid, ask)
      VALUES (${id.idrToken}, 'instrument', ${id.usd}, 'currency', 'latest-observation-v1', 0.25,
              'last', 'venue', ${id.kraken}, '2026-09-25T10:00:05Z', 1, '2026-09-25T10:00:06Z', 0.24, 0.26)`;
    await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
      VALUES (${id.idrToken}, ${id.usd}, ${token}, 0.25)`;

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
  const quote = async (now: string, q: string) => {
    const res = await app(now).request(`/v1/quote/${q}`);
    return { status: res.status, body: (await res.json()) as unknown };
  };

  it("EUR/USD is the FX market, one aggregate of two venue mids", async () => {
    for (const q of ["EUR/USD", "eur/usd", "EURUSD", "eurusd"]) {
      const r = await quote("2026-09-25T10:00:10Z", q);
      expect(r.status, q).toBe(200);
      const body = v1.QuoteV1.parse(r.body);
      expect(body).toMatchObject({
        subject: {
          kind: "instrument",
          class: "fx",
          name: "EUR/USD",
          baseCurrency: { code: "EUR" },
          quoteCurrency: { code: "USD" },
        },
        unit: { kind: "currency", code: "USD" },
        priceType: "mid",
        price: "1.1378100",
        bid: "1.1378050",
        ask: "1.1378150",
        spread: "0.0000100",
        basis: "aggregated",
        asOf: "2026-09-25T10:00:00Z",
        ageMs: 10_000,
        freshness: "fresh",
        aggregation: { method: "mean-venue-mid-v1", eligibleObservations: 2 },
      });
      for (const key of ["venue", "source", "observedAt"]) expect(body).not.toHaveProperty(key);
      expect(JSON.stringify(body)).not.toMatch(/kraken|bitstamp|sourceRecord|observationId/i);
    }
  });

  it("never inverts silently: USD/EUR is not EUR/USD", async () => {
    const r = await quote("2026-09-25T10:00:10Z", "USD/EUR");
    expect(r.status).toBe(404);
    expect(v1.ErrorV1.parse(r.body).error.code).toBe("not_found");
  });

  it("USD/IDR is a reference rate: no bid/ask, fresh over the weekend", async () => {
    // Sunday: 2.5 days after the rate's date, but no weekday has passed
    // beyond Friday; ageMs is still the literal elapsed time.
    for (const q of ["USD/IDR", "usd/idr", "USDIDR"]) {
      const r = await quote("2026-09-27T12:00:00Z", q);
      expect(r.status, q).toBe(200);
      const body = v1.QuoteV1.parse(r.body);
      expect(body).toMatchObject({
        subject: { class: "fx", name: "USD/IDR", baseCurrency: { code: "USD" } },
        unit: { code: "IDR" },
        priceType: "reference",
        price: "17917.00",
        bid: null,
        ask: null,
        spread: null,
        spreadBps: null,
        basis: "aggregated",
        asOf: "2026-09-24T17:00:00Z",
        ageMs: 241_200_000,
        freshness: "fresh",
        aggregation: { method: "latest-observation-v1", eligibleObservations: 1 },
      });
    }
    // Wednesday 18:00Z without a newer rate: 3 days and 1 h of weekdays.
    const late = v1.QuoteV1.parse((await quote("2026-09-30T18:00:00Z", "USD/IDR")).body);
    expect(late.freshness).toBe("stale");
    // The observation behind it keeps its provenance in /v1/quotes.
    const res = await app("2026-09-27T12:00:00Z").request("/v1/quotes/USD/IDR");
    const o = v1.ObservationsV1.parse(await res.json()).observations;
    expect(o).toHaveLength(1);
    expect(o[0]).toMatchObject({ source: { id: "bank-indonesia" }, bid: null, freshness: "fresh" });
  });

  it("a pair without a source resolves but has no quote", async () => {
    const r = await quote("2026-09-25T10:00:10Z", "USD/JPY");
    expect(r.status).toBe(404);
    expect(v1.ErrorV1.parse(r.body).error.code).toBe("no_quote");
  });

  it("currencies are discoverable by code and name; codes steal no quote", async () => {
    for (const q of ["Indonesian Rupiah", "indonesian rupiah"]) {
      const res = await app("2026-09-25T10:00:10Z").request(
        `/v1/resolve?q=${encodeURIComponent(q)}`,
      );
      const r = v1.ResolveResultV1.parse(await res.json());
      expect(r.status, q).toBe("resolved");
      expect(r.match).toMatchObject({
        kind: "node",
        node: { kind: "currency", name: "Indonesian Rupiah" },
      });
    }
    // `IDR` names the currency and a crypto token: ambiguous to resolve...
    const res = await app("2026-09-25T10:00:10Z").request("/v1/resolve?q=IDR");
    expect(v1.ResolveResultV1.parse(await res.json()).status).toBe("ambiguous");
    // ...but only the token is priced, so its quote is served, as before.
    const r = await quote("2026-09-25T10:00:10Z", "IDR");
    expect(r.status).toBe(200);
    expect(v1.QuoteV1.parse(r.body).subject).toMatchObject({ name: "IDR Token" });
    expect(fetchCalls).toBe(0);
  });
});
