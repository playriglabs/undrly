/**
 * The public canonical quote against a real PostgreSQL database (synthetic
 * data): basis-specific shape, `spread` / `spreadBps`, `ageMs` versus
 * `freshness`, and `change24h` recomputed from stored observations. Canonical
 * rows are seeded as the Rust collector writes them. `fetch` is stubbed to
 * fail: the API never contacts an upstream provider.
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

const id = (n: number) => `0192f100-0000-7000-8000-${n.toString().padStart(12, "0")}`;
const U = {
  usd: id(1),
  btc: id(2),
  sui: id(3),
  eth: id(4),
  nvda: id(5),
  perp: id(6),
  xau: id(7),
  wti: id(8),
  zro: id(9),
  kraken: id(10),
  coinbase: id(11),
  iex: id(12),
  hyperliquid: id(13),
};

type Obs = {
  source: string;
  subject: string;
  venue: string | null;
  type: string;
  price: string;
  bid?: string;
  ask?: string;
  observed: string | null;
  received: string;
};

describe.skipIf(url === undefined)("canonical quote contract with a database", () => {
  const name = `undrly_api_quote_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  const obs: Record<string, string> = {};

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
    const sources = [
      "undrly-curated",
      "kraken",
      "coinbase",
      "alpaca",
      "hyperliquid",
      "gold-api",
      "eia",
    ];
    for (const s of sources) await sql`INSERT INTO sources (id, name) VALUES (${s}, ${s})`;
    let n = 0;
    const record = async (source: string, at: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${`${source}-${++n}`}, ${Buffer.from("{}")}, ${at}) RETURNING id::text`
      )[0]?.id ?? "";
    const at = "2026-09-01T00:00:00Z";
    const curated = await record("undrly-curated", at);
    const instruments: [string, string, string, string, string | null][] = [
      [U.btc, "crypto_asset", "Bitcoin", "BTC", null],
      [U.sui, "crypto_asset", "Sui", "SUI", null],
      [U.eth, "crypto_asset", "Ethereum", "ETH", null],
      [U.nvda, "equity", "NVIDIA Corporation", "NVDA", null],
      [U.perp, "perpetual_future", "BTC Perpetual", "BTC-PERP", null],
      [U.xau, "commodity", "Gold (troy ounce)", "XAU", "troy_ounce"],
      [U.wti, "commodity", "WTI crude oil", "WTI", "barrel"],
      [U.zro, "crypto_asset", "Zero Coin", "ZRO", null],
    ];
    await sql`INSERT INTO nodes (id, category) VALUES (${U.usd}, 'currency')`;
    await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${U.usd}, 'US Dollar', ${curated})`;
    await sql`INSERT INTO identifiers (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
      VALUES ('iso4217', 'USD', ${U.usd}, 'currency', 'undrly-curated', ${at}, ${curated})`;
    for (const [node, cls, label, alias, unit] of instruments) {
      await sql`INSERT INTO nodes (id, category) VALUES (${node}, 'instrument')`;
      await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id, unit_of_measure)
        VALUES (${node}, ${cls}, ${label}, ${curated}, ${unit})`;
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${node}, 'instrument', ${alias}, 'symbol', 'undrly-curated', ${at}, ${curated})`;
    }
    for (const [node, label] of [
      [U.kraken, "Kraken"],
      [U.coinbase, "Coinbase Exchange"],
      [U.iex, "IEX"],
      [U.hyperliquid, "Hyperliquid"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${node}, 'venue')`;
      await sql`INSERT INTO venues (id, name, source_record_id) VALUES (${node}, ${label}, ${curated})`;
    }
    await sql`INSERT INTO quote_feeds (feed_source_id, symbol, subject_id, subject_category, unit_id,
        unit_category, basis, price_type, stale_after_seconds, source_id, received_at, source_record_id)
      VALUES ('eia', 'RWTC', ${U.wti}, 'instrument', ${U.usd}, 'currency', 'aggregated', 'reference',
              1209600, 'undrly-curated', ${at}, ${curated})`;

    const observe = async (key: string, o: Obs) => {
      const rec = await record(o.source, o.received);
      obs[key] =
        (
          await sql<{ id: string }[]>`
            INSERT INTO market_observations
              (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask, unit_id,
               unit_category, source_id, observed_at, received_at, source_record_id)
            VALUES (${o.subject}, 'instrument', ${o.venue === null ? "aggregated" : "venue"},
                    ${o.venue}, ${o.type}, ${o.price}::numeric, ${o.bid ?? null}::numeric,
                    ${o.ask ?? null}::numeric, ${U.usd}, 'currency', ${o.source},
                    ${o.observed}::text::timestamptz, ${o.received}, ${rec})
            RETURNING id::text`
        )[0]?.id ?? "";
    };
    /** A canonical row exactly as the collector writes it, with its inputs. */
    const canonical = async (
      subject: string,
      q: {
        method: string;
        price: string;
        type: string;
        basis: string;
        venue?: string;
        bid?: string;
        ask?: string;
        asOf: string;
        computed: string;
      },
      inputs: [string, string][],
    ) => {
      await sql`INSERT INTO canonical_quotes (subject_id, subject_category, unit_id, unit_category,
          method, price, price_type, basis, venue_id, as_of, eligible_count, computed_at, bid, ask)
        VALUES (${subject}, 'instrument', ${U.usd}, 'currency', ${q.method}, ${q.price}::numeric,
                ${q.type}, ${q.basis}, ${q.venue ?? null}, ${q.asOf}::text::timestamptz, ${inputs.length}, ${q.computed}::text::timestamptz,
                ${q.bid ?? null}::numeric, ${q.ask ?? null}::numeric)`;
      for (const [key, price] of inputs) {
        await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
          VALUES (${subject}, ${U.usd}, ${obs[key] ?? ""}, ${price}::numeric)`;
      }
    };
    const kraken = (subject: string, bid: string, ask: string, received: string, price = ask) => ({
      source: "kraken",
      subject,
      venue: U.kraken,
      type: "last",
      price,
      bid,
      ask,
      observed: null,
      received,
    });
    const coinbase = (
      subject: string,
      bid: string,
      ask: string,
      mid: string,
      observed: string,
    ) => ({
      source: "coinbase",
      subject,
      venue: U.coinbase,
      type: "mid",
      price: mid,
      bid,
      ask,
      observed,
      received: observed.replace(/Z$/, "").slice(0, 19).concat(".900Z"),
    });

    // BTC/USD: two venues now, and both 24 h earlier (τ = 2026-09-24T07:28:06.386017Z).
    await observe(
      "btcKraken",
      kraken(U.btc, "84145.90000", "84146.00000", "2026-09-25T07:28:07Z", "84143.10000"),
    );
    await observe(
      "btcCoinbase",
      coinbase(U.btc, "84006.45", "84006.46", "84006.455", "2026-09-25T07:28:06.386017Z"),
    );
    await observe(
      "btcCoinbaseOld",
      coinbase(U.btc, "81000.00", "81000.02", "81000.010", "2026-09-24T07:20:00Z"),
    );
    await observe(
      "btcCoinbase24h",
      coinbase(U.btc, "82900.12", "82900.14", "82900.130", "2026-09-24T07:27:45.5Z"),
    );
    await observe(
      "btcKraken24h",
      kraken(U.btc, "83000.00000", "83000.20000", "2026-09-24T07:27:50Z"),
    );
    // After τ: never a baseline.
    await observe(
      "btcKrakenAfter",
      kraken(U.btc, "99999.00000", "99999.10000", "2026-09-24T07:28:10Z"),
    );
    await canonical(
      U.btc,
      {
        method: "mean-venue-mid-v1",
        price: "84076.2025000",
        type: "mid",
        basis: "aggregated",
        bid: "84076.1750000",
        ask: "84076.2300000",
        asOf: "2026-09-25T07:28:06.386017Z",
        computed: "2026-09-25T07:28:10Z",
      },
      [
        ["btcKraken", "84145.950000"],
        ["btcCoinbase", "84006.455"],
      ],
    );

    // SUI/USD: two venues now; 24 h earlier only Kraken was fresh → no baseline.
    await observe(
      "suiCoinbase",
      coinbase(U.sui, "1.1819", "1.1821", "1.18200", "2026-09-25T07:28:05Z"),
    );
    await observe("suiKraken", kraken(U.sui, "1.1803", "1.1805", "2026-09-25T07:28:07Z"));
    await observe("suiKraken24h", kraken(U.sui, "1.2000", "1.2002", "2026-09-24T07:27:50Z"));
    await observe(
      "suiCoinbaseOld",
      coinbase(U.sui, "1.1900", "1.1902", "1.19010", "2026-09-24T07:20:00Z"),
    );
    await canonical(
      U.sui,
      {
        method: "mean-venue-mid-v1",
        price: "1.181200",
        type: "mid",
        basis: "aggregated",
        bid: "1.181100",
        ask: "1.181300",
        asOf: "2026-09-25T07:28:05Z",
        computed: "2026-09-25T07:28:10Z",
      },
      [
        ["suiCoinbase", "1.18200"],
        ["suiKraken", "1.18040"],
      ],
    );

    // ETH/USD: one fresh venue now (Coinbase stale), and that venue alone 24 h earlier.
    await observe("ethKraken", kraken(U.eth, "2689.29000", "2689.30000", "2026-09-25T07:28:07Z"));
    await observe(
      "ethCoinbaseStale",
      coinbase(U.eth, "2600.00", "2600.02", "2600.010", "2026-09-25T07:00:00Z"),
    );
    await observe(
      "ethKraken24h",
      kraken(U.eth, "2800.00000", "2800.01000", "2026-09-24T07:28:00Z"),
    );
    await canonical(
      U.eth,
      {
        method: "mean-venue-mid-v1",
        price: "2689.2950000",
        type: "mid",
        basis: "aggregated",
        bid: "2689.2900000",
        ask: "2689.3000000",
        asOf: "2026-09-25T07:28:07Z",
        computed: "2026-09-25T07:28:10Z",
      },
      [["ethKraken", "2689.295000"]],
    );

    // NVDA: an IEX venue quote, with a price exactly 24 h earlier (equity → no change24h).
    const iex = (price: string, bid: string, ask: string, observed: string, received: string) => ({
      source: "alpaca",
      subject: U.nvda,
      venue: U.iex,
      type: "last",
      price,
      bid,
      ask,
      observed,
      received,
    });
    await observe(
      "nvda",
      iex("225.05", "225.00", "225.10", "2026-09-25T19:59:59.5Z", "2026-09-25T20:00:00Z"),
    );
    await observe(
      "nvda24h",
      iex("220.00", "219.95", "220.05", "2026-09-24T19:59:50Z", "2026-09-24T20:00:00Z"),
    );
    await canonical(
      U.nvda,
      {
        method: "latest-observation-v1",
        price: "225.05",
        type: "last",
        basis: "venue",
        venue: U.iex,
        bid: "225.00",
        ask: "225.10",
        asOf: "2026-09-25T19:59:59.5Z",
        computed: "2026-09-25T20:00:01Z",
      },
      [["nvda", "225.05"]],
    );

    // BTC-PERP: a Hyperliquid mark without bid/ask; unchanged over 24 h.
    const mark = (price: string, received: string) => ({
      source: "hyperliquid",
      subject: U.perp,
      venue: U.hyperliquid,
      type: "mark",
      price,
      observed: null,
      received,
    });
    await observe("perp", mark("83924.0", "2026-09-25T07:28:08Z"));
    await observe("perp24h", mark("83924.0", "2026-09-24T07:25:00Z"));
    await canonical(
      U.perp,
      {
        method: "latest-observation-v1",
        price: "83924.0",
        type: "mark",
        basis: "venue",
        venue: U.hyperliquid,
        asOf: "2026-09-25T07:28:08Z",
        computed: "2026-09-25T07:28:09Z",
      },
      [["perp", "83924.0"]],
    );

    // XAU: a reference price published without a venue or bid/ask.
    const gold = (price: string, observed: string) => ({
      source: "gold-api",
      subject: U.xau,
      venue: null,
      type: "reference",
      price,
      observed,
      received: observed.replace(/Z$/, ".500Z"),
    });
    await observe("xau", gold("4286.200195", "2026-09-25T07:27:54Z"));
    await observe("xau24h", gold("4200.000000", "2026-09-24T07:27:30Z"));
    await canonical(
      U.xau,
      {
        method: "latest-observation-v1",
        price: "4286.200195",
        type: "reference",
        basis: "aggregated",
        asOf: "2026-09-25T07:27:54Z",
        computed: "2026-09-25T07:27:55Z",
      },
      [["xau", "4286.200195"]],
    );

    // WTI: a daily EIA reference (14-day cadence).
    await observe("wti", {
      source: "eia",
      subject: U.wti,
      venue: null,
      type: "reference",
      price: "65.12",
      observed: "2026-09-22T00:00:00Z",
      received: "2026-09-22T06:00:00Z",
    });
    await canonical(
      U.wti,
      {
        method: "latest-observation-v1",
        price: "65.12",
        type: "reference",
        basis: "aggregated",
        asOf: "2026-09-22T00:00:00Z",
        computed: "2026-09-22T06:00:01Z",
      },
      [["wti", "65.12"]],
    );

    // ZRO: a venue last price whose 24 h baseline is zero.
    await observe("zro", kraken(U.zro, "0.49", "0.51", "2026-09-25T07:28:07Z", "0.50"));
    await observe("zro24h", kraken(U.zro, "0", "0.01", "2026-09-24T07:28:00Z", "0"));
    await canonical(
      U.zro,
      {
        method: "latest-observation-v1",
        price: "0.50",
        type: "last",
        basis: "venue",
        venue: U.kraken,
        bid: "0.49",
        ask: "0.51",
        asOf: "2026-09-25T07:28:07Z",
        computed: "2026-09-25T07:28:08Z",
      },
      [["zro", "0.50"]],
    );

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

  const NOW = "2026-09-25T07:28:10Z";
  const raw = async (query: string, now = NOW) => {
    const res = await createApp(sql, { staleAfterSeconds: 300, now: () => new Date(now) }).request(
      `/v1/quote/${query}`,
    );
    expect(res.status, query).toBe(200);
    return (await res.json()) as Record<string, unknown>;
  };
  const quote = async (query: string, now = NOW) => v1.QuoteV1.parse(await raw(query, now));

  it("BTC/USD: a two-input aggregate with its mean bid/ask, spread and 24 h change", async () => {
    const body = await raw("BTC");
    expect(Object.keys(body)).toStrictEqual([
      "schemaVersion",
      "subject",
      "unit",
      "priceType",
      "price",
      "bid",
      "ask",
      "spread",
      "spreadBps",
      "basis",
      "receivedAt",
      "asOf",
      "ageMs",
      "freshness",
      "change24h",
      "aggregation",
    ]);
    const q = v1.QuoteV1.parse(body);
    expect(q).toMatchObject({
      basis: "aggregated",
      priceType: "mid",
      price: "84076.2025000",
      bid: "84076.1750000",
      ask: "84076.2300000",
      spread: "0.0550000",
      spreadBps: "0.0065",
      asOf: "2026-09-25T07:28:06.386017Z",
      freshness: "fresh",
      aggregation: {
        method: "mean-venue-mid-v1",
        eligibleObservations: 2,
        computedAt: "2026-09-25T07:28:10Z",
      },
    });
    expect(v1.decimalCompare(q.bid ?? "", q.price)).toBeLessThanOrEqual(0);
    expect(v1.decimalCompare(q.price, q.ask ?? "")).toBeLessThanOrEqual(0);
    // Baseline: mean of the same two feeds' mids at τ = asOf - 24 h:
    // (83000.100000 + 82900.130) / 2 at scale 7, as of the older input.
    // Not Kraken's later 99999.05 (after τ), not Coinbase's older 81000.010.
    expect(q.change24h).toStrictEqual({
      absolute: "1126.0875000",
      percent: "1.3575",
      from: "82950.1150000",
      asOf: "2026-09-24T07:27:45.500Z",
    });
  });

  it("SUI/USD: aggregated bid/ask are the input means; no baseline mixing venues", async () => {
    const q = await quote("SUI");
    expect(q).toMatchObject({
      price: "1.181200",
      bid: "1.181100",
      ask: "1.181300",
      spread: "0.000200",
      spreadBps: "1.6932",
      aggregation: { eligibleObservations: 2 },
    });
    // 24 h earlier only Kraken was fresh: a single-venue price is not the
    // baseline of a two-venue aggregate.
    expect(q.change24h).toBeNull();
  });

  it("one-input aggregate: that venue's mid, bid and ask; a negative 24 h change", async () => {
    const q = await quote("ETH");
    expect(q).toMatchObject({
      basis: "aggregated",
      price: "2689.2950000",
      bid: "2689.2900000",
      ask: "2689.3000000",
      spread: "0.0100000",
      spreadBps: "0.0372",
      aggregation: { method: "mean-venue-mid-v1", eligibleObservations: 1 },
    });
    expect(q.change24h).toStrictEqual({
      absolute: "-110.7100000",
      percent: "-3.9539",
      from: "2800.0050000",
      asOf: "2026-09-24T07:28:00Z",
    });
  });

  it("aggregated quotes serialize no venue, source or observedAt", async () => {
    for (const query of ["BTC", "SUI", "ETH", "XAU", "WTI"]) {
      const body = await raw(query);
      for (const key of ["venue", "source", "observedAt"]) {
        expect(body, `${query} ${key}`).not.toHaveProperty(key);
      }
      const text = JSON.stringify(body);
      for (const leak of ["kraken", "coinbase", "gold-api", "eia", "Kraken", "Coinbase"]) {
        expect(text, `${query} names ${leak}`).not.toContain(`"${leak}"`);
      }
    }
  });

  it("equity venue quote: venue and observedAt, no source; spread; no 24 h change", async () => {
    const body = await raw("NVDA", "2026-09-25T20:00:05Z");
    expect(body).not.toHaveProperty("source");
    expect(JSON.stringify(body)).not.toContain("alpaca");
    const q = v1.QuoteV1.parse(body);
    expect(q).toMatchObject({
      basis: "venue",
      venue: { name: "IEX" },
      observedAt: "2026-09-25T19:59:59.500Z",
      price: "225.05",
      bid: "225.00",
      ask: "225.10",
      spread: "0.10",
      spreadBps: "4.4435",
      ageMs: 5500,
    });
    // A price exactly 24 h earlier is stored, but equities trade in sessions.
    expect(q.change24h).toBeNull();
  });

  it("venue mark without bid/ask: null spread; a zero 24 h change", async () => {
    const q = await quote("BTC-PERP");
    expect(q).toMatchObject({
      basis: "venue",
      venue: { name: "Hyperliquid" },
      observedAt: null,
      priceType: "mark",
      bid: null,
      ask: null,
      spread: null,
      spreadBps: null,
    });
    expect(q.change24h).toStrictEqual({
      absolute: "0.0",
      percent: "0.0000",
      from: "83924.0",
      asOf: "2026-09-24T07:25:00Z",
    });
  });

  it("commodity reference quote: no bid/ask, no spread, no 24 h change", async () => {
    const q = await quote("XAU");
    expect(q).toMatchObject({
      basis: "aggregated",
      priceType: "reference",
      price: "4286.200195",
      bid: null,
      ask: null,
      spread: null,
      spreadBps: null,
      change24h: null,
      ageMs: 16_000,
      freshness: "fresh",
    });
  });

  it("a zero 24 h baseline yields no change", async () => {
    const q = await quote("ZRO");
    expect(q).toMatchObject({ basis: "venue", price: "0.50", change24h: null });
  });

  it("ageMs is elapsed time since asOf (not receivedAt), floored, never negative", async () => {
    // asOf 07:28:06.386017, receivedAt 07:28:07.100: 3613.983 ms → 3613.
    expect((await quote("BTC")).ageMs).toBe(3613);
    // A clock behind asOf gives 0, never a negative age.
    expect((await quote("BTC", "2026-09-25T07:28:06Z")).ageMs).toBe(0);
    expect((await quote("BTC", "2026-09-25T07:28:06.386Z")).ageMs).toBe(0);
    expect((await quote("BTC", "2026-09-25T07:28:06.388Z")).ageMs).toBe(1);
  });

  it("ageMs is computed per response and not stored", async () => {
    const [a, b] = [await quote("XAU", NOW), await quote("XAU", "2026-09-25T07:29:10Z")];
    expect(b.ageMs - a.ageMs).toBe(60_000);
    expect({ ...a, ageMs: 0, freshness: "" }).toStrictEqual({ ...b, ageMs: 0, freshness: "" });
    const columns = await sql<{ column_name: string }[]>`
      SELECT column_name FROM information_schema.columns WHERE table_name = 'canonical_quotes'`;
    const names = columns.map((c) => c.column_name);
    for (const derived of ["age", "age_ms", "spread", "spread_bps", "change24h"]) {
      expect(names).not.toContain(derived);
    }
  });

  it("freshness and ageMs are independent", async () => {
    // WTI: over three days old, still fresh on its 14-day cadence.
    const wti = await quote("WTI");
    expect(wti.ageMs).toBe(3 * 86_400_000 + 7 * 3_600_000 + 28 * 60_000 + 10_000);
    expect(wti.freshness).toBe("fresh");
    expect(wti.change24h).toBeNull();
    // BTC-PERP: six minutes old, stale on its 300 s window.
    const perp = await quote("BTC-PERP", "2026-09-25T07:34:09Z");
    expect(perp.ageMs).toBe(361_000);
    expect(perp.freshness).toBe("stale");
    expect(perp.ageMs).toBeLessThan(wti.ageMs);
  });

  it("change24h depends on asOf, not on the response time", async () => {
    const [a, b] = [await quote("ETH"), await quote("ETH", "2026-09-25T07:28:30Z")];
    expect(a.change24h).toStrictEqual(b.change24h);
  });

  it("/v1/quotes still exposes each venue observation's own bid and ask", async () => {
    const res = await createApp(sql, { staleAfterSeconds: 300, now: () => new Date(NOW) }).request(
      "/v1/quotes/SUI",
    );
    const body = v1.ObservationsV1.parse(await res.json());
    expect(body.observations.map((o) => [o.venue?.name, o.bid, o.ask]).sort()).toStrictEqual([
      ["Coinbase Exchange", "1.1819", "1.1821"],
      ["Kraken", "1.1803", "1.1805"],
    ]);
  });

  it("never contacted an upstream provider", () => {
    expect(fetchCalls).toBe(0);
  });
});
