/**
 * V1.9 derived FX (docs/v1.9-live-fx.md) against a real PostgreSQL
 * database: USD/IDR declared as the cross USDT/IDR ÷ USDT/USD, its canonical
 * quote and legs seeded as the collector writes them, and 25 hourly bars on
 * each leg. `fetch` is stubbed to fail: no upstream is contacted.
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
  const hex = Date.now().toString(16).padStart(12, "0");
  const rand = crypto.randomUUID().replaceAll("-", "");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${rand.slice(0, 3)}-8${rand.slice(3, 6)}-${rand.slice(6, 18)}`;
};
const NOW = "2026-10-01T05:00:30Z";

describe.skipIf(url === undefined)("derived FX with a database", () => {
  const name = `undrly_derived_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  const id = {
    usd: uuid(),
    idr: uuid(),
    usdt: uuid(),
    usdIdr: uuid(),
    binance: uuid(),
    kraken: uuid(),
  };
  // Leg closes over 25 hours: USDT/IDR rises 17900 → 17924, USDT/USD flat 1.0000.
  const idrClose = (h: number) => `${17900 + h}`;

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
    const at = "2026-10-01T00:00:00Z";
    await sql`INSERT INTO sources (id, name) VALUES ('undrly-curated', 'Curated'),
      ('binance', 'Binance'), ('kraken', 'Kraken')`;
    const record = async (source: string, key: string, receivedAt: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${key}, ${Buffer.from("raw")}, ${receivedAt}) RETURNING id::text`
      )[0]?.id ?? "";
    const curated = await record("undrly-curated", "data/reference/stablecoin-fx.json", at);
    for (const [cid, code, cname] of [
      [id.usd, "USD", "US Dollar"],
      [id.idr, "IDR", "Indonesian Rupiah"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${cid}, 'currency')`;
      await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${cid}, ${cname}, ${curated})`;
      await sql`INSERT INTO identifiers
        (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
        VALUES ('iso4217', ${code}, ${cid}, 'currency', 'undrly-curated', ${at}, ${curated})`;
    }
    await sql`INSERT INTO nodes (id, category) VALUES (${id.usdt}, 'instrument'), (${id.usdIdr}, 'instrument')`;
    await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id)
      VALUES (${id.usdt}, 'crypto_asset', 'Tether', ${curated})`;
    await sql`INSERT INTO instruments
      (id, instrument_class, name, base_currency_id, quote_currency_id, source_record_id)
      VALUES (${id.usdIdr}, 'fx', 'USD/IDR', ${id.usd}, ${id.idr}, ${curated})`;
    await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
      VALUES (${id.usdIdr}, 'instrument', 'USDIDR', 'symbol', 'undrly-curated', ${at}, ${curated}),
             (${id.usdt}, 'instrument', 'USDT', 'symbol', 'undrly-curated', ${at}, ${curated})`;
    for (const [vid, vname] of [
      [id.binance, "Binance"],
      [id.kraken, "Kraken"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${vid}, 'venue')`;
      await sql`INSERT INTO venues (id, name, source_record_id) VALUES (${vid}, ${vname}, ${curated})`;
    }
    await sql`INSERT INTO quote_derivations
      (subject_id, subject_category, unit_id, unit_category, method,
       numerator_subject_id, numerator_subject_category, numerator_unit_id, numerator_unit_category,
       denominator_subject_id, denominator_subject_category, denominator_unit_id,
       denominator_unit_category, source_id, received_at, source_record_id)
      VALUES (${id.usdIdr}, 'instrument', ${id.idr}, 'currency', 'cross-via-stablecoin-v1',
              ${id.usdt}, 'instrument', ${id.idr}, 'currency',
              ${id.usdt}, 'instrument', ${id.usd}, 'currency', 'undrly-curated', ${at}, ${curated})`;

    // The legs' latest observations, as the collector stores them.
    const observe = async (
      source: string,
      unit: string,
      venue: string,
      price: string,
      received: string,
    ) => {
      const rec = await record(source, `${source}:${unit}`, received);
      return (
        (
          await sql<{ id: string }[]>`
          INSERT INTO market_observations
            (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask, unit_id,
             unit_category, source_id, observed_at, received_at, source_record_id)
          VALUES (${id.usdt}, 'instrument', 'venue', ${venue}, 'mid', ${price}::numeric, NULL, NULL,
                  ${unit}, 'currency', ${source}, NULL, ${received}, ${rec})
          RETURNING id::text`
        )[0]?.id ?? ""
      );
    };
    const idrObs = await observe("binance", id.idr, id.binance, "17924.5", "2026-10-01T05:00:10Z");
    const usdObs = await observe("kraken", id.usd, id.kraken, "1.00005", "2026-10-01T05:00:12Z");
    // The cross, as `cross_quote` computes it: 17924.5 / 1.00005 at 6 digits.
    await sql`INSERT INTO canonical_quotes
      (subject_id, subject_category, unit_id, unit_category, method, price, price_type, basis,
       as_of, eligible_count, computed_at)
      VALUES (${id.usdIdr}, 'instrument', ${id.idr}, 'currency', 'cross-via-stablecoin-v1',
              17923.6, 'mid', 'derived', '2026-10-01T05:00:10Z', 2, '2026-10-01T05:00:13Z')`;
    await sql`INSERT INTO canonical_quote_legs
      (subject_id, unit_id, observation_id, leg_subject_id, leg_unit_id, input_price)
      VALUES (${id.usdIdr}, ${id.idr}, ${idrObs}, ${id.usdt}, ${id.idr}, 17924.5),
             (${id.usdIdr}, ${id.idr}, ${usdObs}, ${id.usdt}, ${id.usd}, 1.00005)`;

    // 25 hourly bars per leg, ending with the hour that opened at 04:00.
    // Received after the last bar closed: every bar is complete.
    const barsAt = "2026-10-01T05:00:20Z";
    const barRecord = await record("binance", "bars", barsAt);
    const krakenBars = await record("kraken", "bars", barsAt);
    for (let h = 0; h < 25; h++) {
      const open = new Date(Date.parse("2026-09-30T04:00:00Z") + h * 3_600_000);
      const close = new Date(open.getTime() + 3_600_000);
      for (const [unit, venue, source, rec, price] of [
        [id.idr, id.binance, "binance", barRecord, idrClose(h)],
        [id.usd, id.kraken, "kraken", krakenBars, "1.0000"],
      ] as const) {
        await sql`INSERT INTO market_bars
          (subject_id, subject_category, unit_id, unit_category, source_id, venue_id, bar_interval,
           open_time, close_time, open, high, low, close, volume, received_at, source_record_id)
          VALUES (${id.usdt}, 'instrument', ${unit}, 'currency', ${source}, ${venue}, '1h',
                  ${open}, ${close}, ${price}::numeric, ${price}::numeric, ${price}::numeric,
                  ${price}::numeric, 1, ${barsAt}, ${rec})`;
      }
    }

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

  const get = async (path: string, now = NOW) => {
    const res = await createApp(sql, { staleAfterSeconds: 300, now: () => new Date(now) }).request(
      path,
    );
    return { status: res.status, body: (await res.json()) as unknown };
  };

  it("USD/IDR is a derived mid, the cross of its legs, fresh while they are", async () => {
    const q = v1.QuoteV1.parse((await get("/v1/quote/USD/IDR")).body);
    expect(q).toMatchObject({
      basis: "derived",
      priceType: "mid",
      price: "17923.6",
      asOf: "2026-10-01T05:00:10Z",
      receivedAt: "2026-10-01T05:00:12Z",
      freshness: "fresh",
      aggregation: { method: "cross-via-stablecoin-v1", eligibleObservations: 2 },
    });
    expect(q.price).toBe(v1.crossRate("17924.5", "1.00005"));
    expect(q).not.toHaveProperty("venue");
    const stale = v1.QuoteV1.parse((await get("/v1/quote/USD/IDR", "2026-10-01T05:10:00Z")).body);
    expect(stale.freshness).toBe("stale");
  });

  it("24h statistics and the sparkline come from the legs' hourly closes", async () => {
    const m = v1.MarketV1.parse((await get("/v1/market/USD/IDR")).body);
    expect(m.marketStatus).toBe("continuous");
    expect(m.statistics).toMatchObject({
      window: "rolling_24h_closes",
      open: "17900",
      close: "17924",
      high: "17924",
      low: "17900",
      previousClose: null,
      volume: null,
      from: "2026-09-30T05:00:00Z",
    });
    expect(m.statistics?.changePercent).toBe(v1.changeOf("17924", "17900")?.percent);
    const all = v1.MarketsV1.parse((await get("/v1/markets?class=fx")).body);
    const row = all.markets.find((r) => r.subject.name === "USD/IDR");
    expect(row?.sparkline).toHaveLength(24);
    expect(row?.sparkline.at(-1)).toBe("17924");
  });

  it("history serves the cross's closes; candles point there", async () => {
    const h = v1.HistoryV1.parse((await get("/v1/history/USD/IDR?interval=1h")).body);
    expect(h).toMatchObject({ priceType: "mid", basis: "derived" });
    expect(h.observations).toHaveLength(25);
    expect(h.observations.at(-1)).toEqual({ asOf: "2026-10-01T05:00:00Z", price: "17924" });
    const c = await get("/v1/candles/USD/IDR?interval=1h");
    expect(c.status).toBe(404);
    expect(v1.ErrorV1.parse(c.body).error.message).toContain("/v1/history");
    const bad = await get("/v1/history/USD/IDR?series=nope");
    expect(bad.status).toBe(400);
    expect(fetchCalls).toBe(0);
  });
});
