/**
 * V1.3 market data endpoints against a real PostgreSQL database, seeded
 * directly: Kraken 1h bars for BTC/USD (26 hours, one missing hour, the last
 * in progress), IEX daily bars and a calendar for NVDA, a Hyperliquid
 * perpetual context, and a reference series. `fetch` is
 * stubbed to fail: no upstream is contacted.
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
let seq = 0;
const uuid = () => {
  const hex = (Date.now() + seq++).toString(16).padStart(12, "0");
  const rand = crypto.randomUUID().replaceAll("-", "");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${rand.slice(0, 3)}-8${rand.slice(3, 6)}-${rand.slice(6, 18)}`;
};
const HOUR = 3_600_000;
const iso = (t: number) => new Date(t).toISOString().replace(".000Z", "Z");

describe.skipIf(url === undefined)("market data with a database", () => {
  const name = `undrly_md_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let fetchCalls = 0;
  const realFetch = globalThis.fetch;
  const id = {
    usd: uuid(),
    usdc: uuid(),
    idr: uuid(),
    btc: uuid(),
    nvda: uuid(),
    perp: uuid(),
    usdIdr: uuid(),
    kraken: uuid(),
    iex: uuid(),
    hl: uuid(),
  };
  // BTC/USD hourly bars: 2026-09-25T00:00Z … 2026-09-26T02:00Z (27 hours), the
  // 2026-09-25T05:00Z hour missing; the last bar in progress when collected.
  const T0 = Date.parse("2026-09-25T00:00:00Z");
  const NOW = "2026-09-26T02:30:00Z";

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
      ('kraken', 'Kraken'), ('alpaca', 'Alpaca'), ('hyperliquid', 'Hyperliquid'),
      ('bank-indonesia', 'Bank Indonesia'), ('fred', 'FRED'), ('finnhub', 'Finnhub')`;
    const record = async (source: string, key: string, receivedAt: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES (${source}, ${key}, ${Buffer.from(key)}, ${receivedAt}) RETURNING id::text`
      )[0]?.id ?? "";
    const curated = await record("undrly-curated", "curated", at);
    for (const [cid, code, cname] of [
      [id.usd, "USD", "US Dollar"],
      [id.idr, "IDR", "Indonesian Rupiah"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${cid}, 'currency')`;
      await sql`INSERT INTO currencies (id, name, source_record_id) VALUES (${cid}, ${cname}, ${curated})`;
      await sql`INSERT INTO identifiers (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
        VALUES ('iso4217', ${code}, ${cid}, 'currency', 'undrly-curated', ${at}, ${curated})`;
    }
    const instrument = async (iid: string, cls: string, iname: string, symbol: string) => {
      await sql`INSERT INTO nodes (id, category) VALUES (${iid}, 'instrument')`;
      await sql`INSERT INTO instruments (id, instrument_class, name, source_record_id)
        VALUES (${iid}, ${cls}, ${iname}, ${curated})`;
      await sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
        VALUES (${iid}, 'instrument', ${symbol}, 'symbol', 'undrly-curated', ${at}, ${curated})`;
    };
    await instrument(id.btc, "crypto_asset", "Bitcoin", "BTC");
    await instrument(id.usdc, "crypto_asset", "USD Coin", "USDC");
    await instrument(id.nvda, "equity", "NVIDIA Corporation Common Stock", "NVDA");
    await instrument(id.perp, "perpetual_future", "BTC Perpetual (Hyperliquid)", "BTC-PERP");
    await sql`INSERT INTO nodes (id, category) VALUES (${id.usdIdr}, 'instrument')`;
    await sql`INSERT INTO instruments (id, instrument_class, name, base_currency_id, quote_currency_id, source_record_id)
      VALUES (${id.usdIdr}, 'fx', 'USD/IDR', ${id.usd}, ${id.idr}, ${curated})`;
    for (const [vid, vname] of [
      [id.kraken, "Kraken"],
      [id.iex, "IEX"],
      [id.hl, "Hyperliquid"],
    ] as const) {
      await sql`INSERT INTO nodes (id, category) VALUES (${vid}, 'venue')`;
      await sql`INSERT INTO venues (id, name, source_record_id) VALUES (${vid}, ${vname}, ${curated})`;
    }
    const feed = async (
      source: string,
      symbol: string,
      subject: string,
      unit: string,
      unitCategory: string,
      venue: string | null,
      priceType: string,
    ) =>
      sql`INSERT INTO quote_feeds (feed_source_id, symbol, subject_id, subject_category, unit_id,
          unit_category, basis, venue_id, price_type, source_id, received_at, source_record_id)
        VALUES (${source}, ${symbol}, ${subject}, 'instrument', ${unit}, ${unitCategory},
          ${venue === null ? "aggregated" : "venue"}, ${venue}, ${priceType}, 'undrly-curated', ${at}, ${curated})`;
    await feed("kraken", "XXBTZUSD", id.btc, id.usd, "currency", id.kraken, "last");
    await feed("alpaca", "NVDA", id.nvda, id.usd, "currency", id.iex, "last");
    await feed("hyperliquid", "BTC", id.perp, id.usdc, "instrument", id.hl, "mark");
    await feed("bank-indonesia", "JISDOR-USD", id.usdIdr, id.idr, "currency", null, "reference");

    // Canonical quotes: one observation each (latest-observation-v1).
    const quote = async (
      source: string,
      subject: string,
      unit: string,
      unitCategory: string,
      venue: string | null,
      priceType: string,
      price: string,
      observedAt: string | null,
      receivedAt: string,
    ) => {
      const rec = await record(source, `${source}:${subject}:${price}:${receivedAt}`, receivedAt);
      const oid =
        (
          await sql<{ id: string }[]>`
          INSERT INTO market_observations (subject_id, subject_category, basis, venue_id, price_type,
            price, unit_id, unit_category, source_id, observed_at, received_at, source_record_id)
          VALUES (${subject}, 'instrument', ${venue === null ? "aggregated" : "venue"}, ${venue},
            ${priceType}, ${price}::numeric, ${unit}, ${unitCategory}, ${source},
            ${observedAt}::text::timestamptz, ${receivedAt}, ${rec}) RETURNING id::text`
        )[0]?.id ?? "";
      return oid;
    };
    const canonical = async (
      subject: string,
      unit: string,
      unitCategory: string,
      oid: string,
      o: {
        priceType: string;
        price: string;
        venue: string | null;
        asOf: string;
      },
    ) => {
      await sql`INSERT INTO canonical_quotes (subject_id, subject_category, unit_id, unit_category,
          method, price, price_type, basis, venue_id, as_of, eligible_count, computed_at)
        VALUES (${subject}, 'instrument', ${unit}, ${unitCategory}, 'latest-observation-v1',
          ${o.price}::numeric, ${o.priceType}, ${o.venue === null ? "aggregated" : "venue"}, ${o.venue},
          ${o.asOf}, 1, ${o.asOf})`;
      await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
        VALUES (${subject}, ${unit}, ${oid}, ${o.price}::numeric)`;
    };
    const btcQ = await quote(
      "kraken",
      id.btc,
      id.usd,
      "currency",
      id.kraken,
      "last",
      "84010",
      null,
      "2026-09-26T02:29:50Z",
    );
    await canonical(id.btc, id.usd, "currency", btcQ, {
      priceType: "last",
      price: "84010",
      venue: id.kraken,
      asOf: "2026-09-26T02:29:50Z",
    });
    const nvdaQ = await quote(
      "alpaca",
      id.nvda,
      id.usd,
      "currency",
      id.iex,
      "last",
      "225.05",
      "2026-09-25T19:59:59Z",
      "2026-09-26T02:00:00Z",
    );
    await canonical(id.nvda, id.usd, "currency", nvdaQ, {
      priceType: "last",
      price: "225.05",
      venue: id.iex,
      asOf: "2026-09-25T19:59:59Z",
    });
    const perpQ = await quote(
      "hyperliquid",
      id.perp,
      id.usdc,
      "instrument",
      id.hl,
      "mark",
      "83990.0",
      null,
      "2026-09-26T02:29:55Z",
    );
    await canonical(id.perp, id.usdc, "instrument", perpQ, {
      priceType: "mark",
      price: "83990.0",
      venue: id.hl,
      asOf: "2026-09-26T02:29:55Z",
    });
    for (const [day, price] of [
      ["2026-09-22", "17800.00"],
      ["2026-09-23", "17803.00"],
      ["2026-09-24", "17898.00"],
      ["2026-09-25", "17917.00"],
    ] as const) {
      const t = new Date(Date.parse(`${day}T00:00:00+07:00`)).toISOString();
      const oid = await quote(
        "bank-indonesia",
        id.usdIdr,
        id.idr,
        "currency",
        null,
        "reference",
        price,
        t,
        "2026-09-25T04:00:00Z",
      );
      if (day === "2026-09-25") {
        await canonical(id.usdIdr, id.idr, "currency", oid, {
          priceType: "reference",
          price,
          venue: null,
          asOf: t,
        });
      }
    }

    // Bars.
    const krakenBars = await record("kraken", "ohlc", "2026-09-26T02:29:50Z");
    for (let h = 0; h < 27; h++) {
      if (h === 5) continue;
      const open = 84000 + h;
      await sql`INSERT INTO market_bars (subject_id, subject_category, unit_id, unit_category, source_id,
          venue_id, bar_interval, open_time, close_time, open, high, low, close, volume, received_at, source_record_id)
        VALUES (${id.btc}, 'instrument', ${id.usd}, 'currency', 'kraken', ${id.kraken}, '1h',
          ${iso(T0 + h * HOUR)}, ${iso(T0 + (h + 1) * HOUR)}, ${`${open}.0`}::numeric, ${`${open + 5}.0`}::numeric,
          ${`${open - 5}.0`}::numeric, ${`${open + 1}.0`}::numeric, '1.5'::numeric, '2026-09-26T02:29:50Z', ${krakenBars})`;
    }
    const iexBars = await record("alpaca", "bars", "2026-09-26T02:00:00Z");
    for (const [day, o, h, l, c, next] of [
      ["2026-09-24T04:00:00Z", "220.00", "224.00", "219.00", "223.71", "2026-09-25T04:00:00Z"],
      ["2026-09-25T04:00:00Z", "224.00", "226.50", "223.10", "225.05", "2026-09-26T04:00:00Z"],
    ] as const) {
      await sql`INSERT INTO market_bars (subject_id, subject_category, unit_id, unit_category, source_id,
          venue_id, bar_interval, open_time, close_time, open, high, low, close, volume, received_at, source_record_id)
        VALUES (${id.nvda}, 'instrument', ${id.usd}, 'currency', 'alpaca', ${id.iex}, '1d', ${day}, ${next},
          ${o}::numeric, ${h}::numeric, ${l}::numeric, ${c}::numeric, 1000::numeric, '2026-09-26T02:00:00Z', ${iexBars})`;
    }
    // IEX calendar: Friday 2026-09-25 and Monday 2026-09-28 (EDT).
    const cal = await record("alpaca", "calendar", "2026-09-25T00:00:00Z");
    for (const d of ["2026-09-25", "2026-09-28"]) {
      await sql`INSERT INTO trading_sessions (venue_id, session_date, pre_open_at, open_at, close_at,
          post_close_at, source_id, received_at, source_record_id)
        VALUES (${id.iex}, ${d}, ${`${d}T08:00:00Z`}, ${`${d}T13:30:00Z`}, ${`${d}T20:00:00Z`},
          ${`${d}T24:00:00Z`}, 'alpaca', '2026-09-25T00:00:00Z', ${cal})`;
    }
    // An early close (13:00 EDT).
    await sql`INSERT INTO trading_sessions (venue_id, session_date, pre_open_at, open_at, close_at,
        post_close_at, source_id, received_at, source_record_id)
      VALUES (${id.iex}, '2026-10-30', '2026-10-30T08:00:00Z', '2026-10-30T13:30:00Z',
        '2026-10-30T17:00:00Z', '2026-10-30T21:00:00Z', 'alpaca', '2026-09-25T00:00:00Z', ${cal})`;
    // Corporate actions: a dividend in the window, a split after it.
    await sql`INSERT INTO corporate_actions (instrument_id, source_id, source_action_id, action_type, role,
        ex_date, record_date, payable_date, process_date, cash_amount, special, received_at, source_record_id)
      VALUES (${id.nvda}, 'alpaca', 'div-1', 'cash_dividend', 'subject', '2026-09-28', '2026-09-28',
        '2026-10-01', '2026-10-01', 0.01, false, '2026-09-25T00:00:00Z', ${cal})`;
    await sql`INSERT INTO corporate_actions (instrument_id, source_id, source_action_id, action_type, role,
        ex_date, old_rate, new_rate, received_at, source_record_id)
      VALUES (${id.nvda}, 'alpaca', 'split-1', 'forward_split', 'subject', '2026-10-15', 1, 10,
        '2026-09-25T00:00:00Z', ${cal})`;
    // Earnings: one in the window (after the close, with estimates), one later.
    const fh = await record("finnhub", "earnings", "2026-09-25T00:00:00Z");
    await sql`INSERT INTO earnings_events (instrument_id, source_id, fiscal_year, fiscal_quarter,
        report_date, report_time, eps_estimate, revenue_estimate, received_at, source_record_id)
      VALUES (${id.nvda}, 'finnhub', 2027, 2, '2026-09-29', 'after_close', 1.2501, 54000000000,
        '2026-09-25T00:00:00Z', ${fh})`;
    await sql`INSERT INTO earnings_events (instrument_id, source_id, fiscal_year, fiscal_quarter,
        report_date, received_at, source_record_id)
      VALUES (${id.nvda}, 'finnhub', 2027, 3, '2026-11-18', '2026-09-25T00:00:00Z', ${fh})`;
    // Economic releases: CPI fetched twice; the newer schedule moved 10-14 to 10-15.
    const fred1 = await record("fred", "cpi-1", "2026-09-20T00:00:00Z");
    const fred2 = await record("fred", "cpi-2", "2026-09-25T00:00:00Z");
    const jobs = await record("fred", "jobs", "2026-09-25T00:00:00Z");
    await sql`INSERT INTO economic_calendar_windows (source_record_id, source_id, received_at,
        release_key, category, window_from, window_to) VALUES
      (${fred1}, 'fred', '2026-09-20T00:00:00Z', '10', 'inflation', '2026-09-01', '2026-12-31'),
      (${fred2}, 'fred', '2026-09-25T00:00:00Z', '10', 'inflation', '2026-09-01', '2026-12-31'),
      (${jobs}, 'fred', '2026-09-25T00:00:00Z', '50', 'labor', '2026-09-01', '2026-12-31')`;
    await sql`INSERT INTO economic_release_dates (source_record_id, release_key, release_name, release_date) VALUES
      (${fred1}, '10', 'Consumer Price Index', '2026-10-14'),
      (${fred2}, '10', 'Consumer Price Index', '2026-10-15'),
      (${jobs}, '50', 'Employment Situation', '2026-10-02')`;
    await sql`INSERT INTO trading_calendar_ranges (venue_id, first_date, last_date, source_id, received_at, source_record_id)
      VALUES (${id.iex}, '2026-09-20', '2026-10-31', 'alpaca', '2026-09-25T00:00:00Z', ${cal})`;
    // Perpetual context and funding.
    const ctx = await record("hyperliquid", "ctx", "2026-09-26T02:29:55Z");
    await sql`INSERT INTO perp_contexts (subject_id, unit_id, unit_category, source_id, venue_id, mark_price,
        oracle_price, mid_price, funding_rate, funding_interval_hours, open_interest, volume_24h_base,
        volume_24h_notional, price_24h_ago, received_at, source_record_id)
      VALUES (${id.perp}, ${id.usdc}, 'instrument', 'hyperliquid', ${id.hl}, 83990.0, 84005.0, 83992.5,
        0.0000125, 1, 39202.60446, 45831.55481, 3851413609.4609913826, 83894.0, '2026-09-26T02:29:55Z', ${ctx})`;

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
  const errorCode = (body: unknown) => v1.ErrorV1.parse(body).error.code;

  it("1h candles: venue bars, oldest first, the in-progress bar incomplete", async () => {
    const r = await get("/v1/candles/BTC/USD?interval=1h&limit=3");
    expect(r.status).toBe(200);
    const c = v1.CandlesV1.parse(r.body);
    expect(c).toMatchObject({ interval: "1h", priceType: "last", derived: false });
    for (const key of ["venue", "basis", "source"]) expect(c).not.toHaveProperty(key);
    expect(c.candles.map((x) => x.openTime)).toStrictEqual([
      "2026-09-26T00:00:00Z",
      "2026-09-26T01:00:00Z",
      "2026-09-26T02:00:00Z",
    ]);
    expect(c.candles.map((x) => x.complete)).toStrictEqual([true, true, false]);
    expect(c.candles[0]).toMatchObject({
      open: "84024.0",
      high: "84029.0",
      low: "84019.0",
      close: "84025.0",
      volume: "1.5",
    });
    expect(JSON.stringify(c)).not.toMatch(/kraken"|sourceRecord|observationId/);
  });

  it("4h candles are derived from four consecutive hours; a gap omits the period", async () => {
    const c = v1.CandlesV1.parse((await get("/v1/candles/BTC/USD?interval=4h&limit=10")).body);
    expect(c.derived).toBe(true);
    // 00-04, 08-12, 12-16, 16-20, 20-24 on 09-25, 00-04 on 09-26 (partial: 3 hours) → omitted;
    // 04-08 misses 05:00 → omitted.
    expect(c.candles.map((x) => x.openTime)).toStrictEqual([
      "2026-09-25T00:00:00Z",
      "2026-09-25T08:00:00Z",
      "2026-09-25T12:00:00Z",
      "2026-09-25T16:00:00Z",
      "2026-09-25T20:00:00Z",
    ]);
    expect(c.candles[0]).toMatchObject({
      open: "84000.0",
      high: "84008.0",
      low: "83995.0",
      close: "84004.0",
      volume: "6.0",
      closeTime: "2026-09-25T04:00:00Z",
      complete: true,
    });
  });

  it("start/end bound candles; invalid parameters are bad_request", async () => {
    const c = v1.CandlesV1.parse(
      (
        await get(
          "/v1/candles/BTC/USD?interval=1h&start=2026-09-25T10:00:00Z&end=2026-09-25T12:00:00Z",
        )
      ).body,
    );
    expect(c.candles.map((x) => x.openTime)).toStrictEqual([
      "2026-09-25T10:00:00Z",
      "2026-09-25T11:00:00Z",
    ]);
    for (const q of [
      "interval=2h",
      "",
      "interval=1h&limit=0",
      "interval=1h&limit=1001",
      "interval=1h&limit=abc",
      "interval=1h&start=yesterday",
      "interval=1h&start=2026-09-25T12:00:00Z&end=2026-09-25T10:00:00Z",
    ]) {
      const r = await get(`/v1/candles/BTC/USD?${q}`);
      expect(r.status, q).toBe(400);
      expect(errorCode(r.body)).toBe("bad_request");
    }
  });

  it("markets without bars are no_data; unknown markets not_found", async () => {
    for (const q of ["USD/IDR", "BTC-PERP"]) {
      const r = await get(`/v1/candles/${q}?interval=1h`);
      expect(r.status, q).toBe(404);
      expect(errorCode(r.body), q).toBe("no_data");
    }
    const r = await get("/v1/candles/NOPE?interval=1h");
    expect(errorCode(r.body)).toBe("not_found");
  });

  it("history is the published reference series, never candles", async () => {
    const h = v1.HistoryV1.parse((await get("/v1/history/USD/IDR?limit=3")).body);
    expect(h).toMatchObject({ priceType: "reference", basis: "aggregated" });
    expect(h.observations).toStrictEqual([
      { asOf: "2026-09-22T17:00:00Z", price: "17803.00" },
      { asOf: "2026-09-23T17:00:00Z", price: "17898.00" },
      { asOf: "2026-09-24T17:00:00Z", price: "17917.00" },
    ]);
    const r = await get("/v1/history/BTC/USD");
    expect(errorCode(r.body)).toBe("no_data");
  });

  it("market: continuous crypto with rolling 24h statistics from the venue's bars", async () => {
    const m = v1.MarketV1.parse((await get("/v1/market/BTC/USD")).body);
    expect(m.marketStatus).toBe("continuous");
    // The latest 24 hours (2026-09-25T03:00Z … 09-26T02:00Z) miss 05:00: no statistics.
    expect(m.statistics).toBeNull();
    expect(m).toMatchObject({
      price: "84010",
      priceType: "last",
      basis: "venue",
      freshness: "fresh",
    });
  });

  it("market: equity session statistics and calendar status", async () => {
    const m = v1.MarketV1.parse((await get("/v1/market/NVDA")).body);
    // Saturday 02:30 UTC: no session, the calendar covers the date.
    expect(m.marketStatus).toBe("closed");
    expect(m.freshness).toBe("stale");
    expect(m.statistics).toMatchObject({
      window: "session",
      open: "224.00",
      high: "226.50",
      low: "223.10",
      close: "225.05",
      previousClose: "223.71",
      change: "1.34",
      changePercent: "0.5990",
      volume: "1000",
      from: "2026-09-25T04:00:00Z",
    });
    // Statistics carry no venue or provider identity.
    for (const key of ["venue", "basis", "source"]) expect(m.statistics).not.toHaveProperty(key);
    const status = async (now: string) =>
      v1.MarketV1.parse((await get("/v1/market/NVDA", now)).body).marketStatus;
    expect(await status("2026-09-28T12:00:00Z")).toBe("pre_market");
    expect(await status("2026-09-28T15:00:00Z")).toBe("open");
    expect(await status("2026-09-28T21:00:00Z")).toBe("after_hours");
    expect(await status("2026-11-15T15:00:00Z")).toBe("unknown");
  });

  it("market: a reference rate is not a traded market", async () => {
    const m = v1.MarketV1.parse((await get("/v1/market/USD/IDR")).body);
    expect(m).toMatchObject({ marketStatus: null, statistics: null, priceType: "reference" });
  });

  it("calendar: sessions, early closes and closed weekdays of a session market", async () => {
    const r = await get("/v1/calendar/NVDA?from=2026-09-24&to=2026-09-30");
    expect(r.status).toBe(200);
    const c = v1.CalendarV1.parse(r.body);
    expect(c).toMatchObject({
      timezone: "America/New_York",
      marketStatus: "closed",
      from: "2026-09-24",
      to: "2026-09-30",
      coverage: { from: "2026-09-20", to: "2026-10-31" },
    });
    expect(c.sessions.map((s) => s.date)).toStrictEqual(["2026-09-25", "2026-09-28"]);
    expect(c.sessions[0]).toMatchObject({
      open: "2026-09-25T13:30:00Z",
      close: "2026-09-25T20:00:00Z",
      earlyClose: false,
    });
    // Weekdays in the loaded calendar without a session; the weekend is not listed.
    expect(c.closedWeekdays).toStrictEqual(["2026-09-24", "2026-09-29", "2026-09-30"]);
    for (const key of ["venue", "source"]) expect(c).not.toHaveProperty(key);
    // Corporate actions in the window, as announced; the October split is outside it.
    expect(c.events).toStrictEqual([
      {
        type: "cash_dividend",
        role: "subject",
        date: "2026-09-28",
        exDate: "2026-09-28",
        recordDate: "2026-09-28",
        payableDate: "2026-10-01",
        effectiveDate: null,
        cashAmount: "0.01",
        stockRate: null,
        ratio: null,
        special: false,
        otherSymbol: null,
      },
    ]);
    expect(c.earnings).toStrictEqual([
      {
        date: "2026-09-29",
        time: "after_close",
        fiscalYear: 2027,
        fiscalQuarter: 2,
        epsEstimate: "1.2501",
        epsActual: null,
        revenueEstimate: "54000000000",
        revenueActual: null,
      },
    ]);
    const october = v1.CalendarV1.parse(
      (await get("/v1/calendar/NVDA?from=2026-10-01&to=2026-10-31")).body,
    );
    expect(october.events.map((e) => [e.type, e.ratio])).toStrictEqual([
      ["forward_split", { old: "1", new: "10" }],
    ]);
    const early = v1.CalendarV1.parse(
      (await get("/v1/calendar/NVDA?from=2026-10-30&to=2026-10-30")).body,
    );
    expect(early.sessions[0]?.earlyClose).toBe(true);
    // Outside the loaded calendar nothing is claimed closed.
    const beyond = v1.CalendarV1.parse(
      (await get("/v1/calendar/NVDA?from=2026-11-02&to=2026-11-06")).body,
    );
    expect(beyond).toMatchObject({ sessions: [], closedWeekdays: [], events: [], earnings: [] });
    // Default window: today (New York) and the next 14 days.
    const d = v1.CalendarV1.parse((await get("/v1/calendar/NVDA")).body);
    expect(d).toMatchObject({ from: "2026-09-25", to: "2026-10-09" });
  });

  it("calendar: invalid dates are bad_request; markets without a calendar no_data", async () => {
    for (const q of [
      "from=2026-9-24",
      "from=2026-02-30",
      "from=2026-13-01",
      "from=2026-09-30&to=2026-09-24",
      "from=2026-01-01&to=2027-01-02",
    ]) {
      const r = await get(`/v1/calendar/NVDA?${q}`);
      expect(r.status, q).toBe(400);
      expect(errorCode(r.body), q).toBe("bad_request");
    }
    for (const q of ["BTC/USD", "BTC-PERP", "USD/IDR"]) {
      const r = await get(`/v1/calendar/${q}`);
      expect(r.status, q).toBe(404);
      expect(errorCode(r.body), q).toBe("no_data");
    }
  });

  it("economic calendar: the newest schedule, dates only, with the required notice", async () => {
    const r = await get("/v1/economic-calendar?from=2026-10-01&to=2026-10-31");
    expect(r.status).toBe(200);
    const e = v1.EconomicCalendarV1.parse(r.body);
    // CPI moved from 10-14 to 10-15 in the newer fetch: only 10-15 remains.
    expect(e.releases).toStrictEqual([
      { date: "2026-10-02", name: "Employment Situation", category: "labor" },
      { date: "2026-10-15", name: "Consumer Price Index", category: "inflation" },
    ]);
    expect(e.notice).toContain(
      "not endorsed or certified by the Federal Reserve Bank of St. Louis",
    );
    const inflation = v1.EconomicCalendarV1.parse(
      (await get("/v1/economic-calendar?from=2026-10-01&to=2026-10-31&category=inflation")).body,
    );
    expect(inflation.releases.map((x) => x.name)).toStrictEqual(["Consumer Price Index"]);
    // Default window: today (New York) + 30 days.
    const d = v1.EconomicCalendarV1.parse((await get("/v1/economic-calendar")).body);
    expect(d).toMatchObject({ from: "2026-09-25", to: "2026-10-25" });
    for (const q of ["category=weather", "from=2026-13-01", "from=2026-10-31&to=2026-10-01"]) {
      const bad = await get(`/v1/economic-calendar?${q}`);
      expect(bad.status, q).toBe(400);
    }
  });

  it("derivatives: a perpetual's venue context with explicit units", async () => {
    const d = v1.DerivativesV1.parse((await get("/v1/derivatives/BTC-PERP")).body);
    expect(d).toMatchObject({
      unit: { kind: "asset", code: "USDC" },
      venue: { name: "Hyperliquid" },
      markPrice: "83990.0",
      indexPrice: "84005.0",
      midPrice: "83992.5",
      fundingRate: "0.0000125",
      fundingIntervalHours: 1,
      openInterest: "39202.60446",
      volume24h: "45831.55481",
      volume24hNotional: "3851413609.4609913826",
      price24hAgo: "83894.0",
      asOf: "2026-09-26T02:29:55Z",
      ageMs: 5000,
      freshness: "fresh",
    });
    // 40 minutes later the context is stale by the mark feed's 300 s window.
    const later = v1.DerivativesV1.parse(
      (await get("/v1/derivatives/BTC-PERP", "2026-09-26T03:10:00Z")).body,
    );
    expect(later).toMatchObject({ freshness: "stale", ageMs: 2_405_000 });
    for (const q of ["BTC/USD", "NVDA"]) {
      const r = await get(`/v1/derivatives/${q}`);
      expect(errorCode(r.body), q).toBe("no_data");
    }
    expect(fetchCalls).toBe(0);
  });
});
