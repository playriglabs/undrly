/**
 * The four World's Fair agent journeys through MCP (docs/v1.8-mcp.md §11):
 * an MCP client → the Undrly MCP server → the API routes in process → a
 * fresh PostgreSQL database seeded with fixtures (public identifiers, never
 * production data), with a fixed clock. Also freezes the semantic
 * invariants of §12. `fetch` is stubbed to fail: nothing contacts upstream.
 *
 * Skipped without DATABASE_URL (the role needs CREATEDB).
 */
import { readdirSync, readFileSync } from "node:fs";
import { v1 } from "@undrly/contracts";
import postgres from "postgres";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { inProcessApi } from "../src/api.ts";
import { connect } from "./connect.ts";

const url = process.env["DATABASE_URL"];
const ROOT = new URL("../../../../", import.meta.url);

const SOLANA = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const SOLANA_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const RH_CHAIN = "eip155:4663";
const RH_TOKEN = "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";
const RH_ISIN = "JE00BX9C6J83";
const NVDA_ISIN = "US67066G1040";
const NVDA_FIGI = "BBG000BBJQV0";
const TEMPO = "eip155:4217";
const PATH_USD = "0x20c0000000000000000000000000000000000000";
const AT = "2026-09-28T00:00:00Z";
const NOW = new Date("2026-09-28T13:32:20Z");

type Call = Awaited<ReturnType<typeof connect>>["call"];

describe.skipIf(url === undefined)("V1.8 MCP agent journeys", () => {
  const name = `undrly_mcp_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  let call: Call;
  let close: () => Promise<void>;
  const realFetch = globalThis.fetch;
  let fetchCalls = 0;
  const id: Record<string, string> = {};
  const text = (category: v1.Category, key: string) =>
    v1.formatCanonicalId(category, id[key] ?? "");

  beforeAll(async () => {
    admin = postgres(url as string, { max: 1, onnotice: () => {} });
    await admin.unsafe(`CREATE DATABASE "${name}"`);
    const target = new URL(url as string);
    target.pathname = `/${name}`;
    sql = postgres(target.toString(), { max: 3, onnotice: () => {} });
    const dir = new URL("database/migrations/", ROOT);
    for (const file of readdirSync(dir).sort()) {
      await sql.unsafe(readFileSync(new URL(file, dir), "utf8")).simple();
    }
    let seq = 0;
    const uuid = () => {
      const hex = (Date.now() * 4096 + seq++).toString(16).padStart(15, "0").slice(-15);
      const rand = [...crypto.getRandomValues(new Uint8Array(8))]
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
      return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${hex.slice(12, 15)}-a${rand.slice(0, 3)}-${rand.slice(3, 15)}`;
    };
    const records: Record<string, string> = {};
    for (const source of [
      "undrly-fixture",
      "hyperliquid",
      "circle",
      "rhj-final-terms",
      "rhj-api",
      "tempo-rpc",
    ]) {
      await sql`INSERT INTO sources (id, name) VALUES (${source}, ${source})`;
      records[source] =
        (
          await sql<{ id: string }[]>`
            INSERT INTO source_records (source_id, record_key, payload, received_at)
            VALUES (${source}, ${`journeys.db.test.ts#${source}`}, '\\x7b7d', ${AT}) RETURNING id::text`
        )[0]?.id ?? "";
    }
    const rec = (source = "undrly-fixture") => records[source] ?? "";
    const node = async (key: string, category: string) => {
      id[key] = uuid();
      await sql`INSERT INTO nodes (id, category) VALUES (${id[key] ?? ""}, ${category})`;
      return id[key] ?? "";
    };
    const u = (key: string) => id[key] ?? "";
    const instrument = async (key: string, cls: string, label: string) =>
      sql`INSERT INTO instruments (id, instrument_class, name, source_record_id)
          VALUES (${await node(key, "instrument")}, ${cls}, ${label}, ${rec()})`;
    const venue = async (key: string, label: string) =>
      sql`INSERT INTO venues (id, name, source_record_id)
          VALUES (${await node(key, "venue")}, ${label}, ${rec()})`;
    const entity = async (key: string, label: string) =>
      sql`INSERT INTO entities (id, entity_kind, name, source_record_id)
          VALUES (${await node(key, "entity")}, 'company', ${label}, ${rec()})`;
    const alias = (key: string, category: string, value: string) =>
      sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
          VALUES (${u(key)}, ${category}, ${value}, 'symbol', 'undrly-fixture', ${AT}, ${rec()})`;
    const identifier = (key: string, category: string, scheme: string, value: string) =>
      sql`INSERT INTO identifiers (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
          VALUES (${scheme}, ${value}, ${u(key)}, ${category}, 'undrly-fixture', ${AT}, ${rec()})`;
    const edge = (
      s: string,
      sc: string,
      type: string,
      o: string,
      oc: string,
      source = "undrly-fixture",
    ) =>
      sql`INSERT INTO graph_edges (subject_id, subject_category, relationship_type, object_id,
            object_category, source_id, received_at, source_record_id)
          VALUES (${u(s)}, ${sc}, ${type}, ${u(o)}, ${oc}, ${source}, ${AT}, ${rec(source)})`;
    const chain = async (key: string, label: string, caip2: string, source: string) => {
      const [ns = "", ref = ""] = caip2.split(":");
      await sql`INSERT INTO chains (id, name, caip2_namespace, caip2_reference, source_record_id)
                VALUES (${await node(key, "chain")}, ${label}, ${ns}, ${ref}, ${rec(source)})`;
    };
    const deployment = async (
      key: string,
      chainKey: string,
      ns: string,
      assetNs: string,
      ref: string,
      source: string,
    ) =>
      sql`INSERT INTO deployments (id, chain_id, chain_namespace, asset_namespace, asset_reference, source_record_id)
          VALUES (${await node(key, "deployment")}, ${u(chainKey)}, ${ns}, ${assetNs}, ${ref}, ${rec(source)})`;

    // Currencies and assets. USD is fiat; USDC, USDT and pathUSD are not.
    await sql`INSERT INTO currencies (id, name, source_record_id)
              VALUES (${await node("usd", "currency")}, 'US Dollar', ${rec()})`;
    await identifier("usd", "currency", "iso4217", "USD");
    await instrument("btc", "crypto_asset", "Bitcoin");
    await instrument("usdc", "crypto_asset", "USD Coin");
    await instrument("usdt", "crypto_asset", "Tether");
    await alias("btc", "instrument", "BTC");
    await alias("usdc", "instrument", "USDC");
    await alias("usdt", "instrument", "USDT");

    // A. Hyperliquid: the BTC perpetual, priced in USDT, margined and settled in USDC.
    await instrument("perp", "perpetual_future", "BTC Perpetual (Hyperliquid)");
    await venue("hl", "Hyperliquid");
    await alias("perp", "instrument", "BTC-PERP");
    await edge("perp", "instrument", "DERIVES_FROM", "btc", "instrument");
    await edge("perp", "instrument", "DENOMINATED_IN", "usdt", "instrument");
    await edge("perp", "instrument", "MARGINED_IN", "usdc", "instrument");
    await edge("perp", "instrument", "SETTLES_IN", "usdc", "instrument");
    await edge("perp", "instrument", "TRADES_ON", "hl", "venue");
    await sql`INSERT INTO quote_feeds (feed_source_id, symbol, subject_id, subject_category, unit_id,
        unit_category, basis, venue_id, price_type, source_id, received_at, source_record_id)
      VALUES ('hyperliquid', 'BTC', ${u("perp")}, 'instrument', ${u("usdt")}, 'instrument', 'venue',
        ${u("hl")}, 'mark', 'undrly-fixture', ${AT}, ${rec()})`;
    /** A Hyperliquid source record received at `at` (observations must match it). */
    const record = async (key: string, at: string) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO source_records (source_id, record_key, payload, received_at)
          VALUES ('hyperliquid', ${key}, '\\x7b7d', ${at}) RETURNING id::text`
      )[0]?.id ?? "";
    const obs = async (
      type: string,
      price: string,
      bid: string | null,
      ask: string | null,
      at: string,
    ) =>
      (
        await sql<{ id: string }[]>`
          INSERT INTO market_observations (subject_id, subject_category, basis, venue_id, price_type,
            price, bid, ask, unit_id, unit_category, source_id, observed_at, received_at, source_record_id)
          VALUES (${u("perp")}, 'instrument', 'venue', ${u("hl")}, ${type}, ${price}::numeric,
            ${bid}::numeric, ${ask}::numeric, ${u("usdt")}, 'instrument', 'hyperliquid', NULL, ${at},
            ${await record(`${type}@${at}`, at)})
          RETURNING id::text`
      )[0]?.id ?? "";
    const mark = await obs("mark", "83383.0", null, null, "2026-09-28T13:32:10Z");
    const book = await obs("mid", "83520.50", "83520.0", "83521.0", "2026-09-28T13:32:05Z");
    await sql`INSERT INTO canonical_quotes (subject_id, subject_category, unit_id, unit_category, method,
        price, price_type, basis, venue_id, bid, ask, as_of, eligible_count, computed_at)
      VALUES (${u("perp")}, 'instrument', ${u("usdt")}, 'instrument', 'mark-with-venue-book-v1',
        83383.0, 'mark', 'venue', ${u("hl")}, 83520.0, 83521.0, '2026-09-28T13:32:10Z', 2,
        '2026-09-28T13:32:11Z')`;
    await sql`INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price) VALUES
      (${u("perp")}, ${u("usdt")}, ${mark}, 83383.0), (${u("perp")}, ${u("usdt")}, ${book}, 83520.50)`;
    await sql`INSERT INTO perp_contexts (subject_id, unit_id, unit_category, source_id, venue_id, mark_price,
        oracle_price, mid_price, funding_rate, funding_interval_hours, open_interest, volume_24h_base,
        volume_24h_notional, price_24h_ago, received_at, source_record_id)
      VALUES (${u("perp")}, ${u("usdt")}, 'instrument', 'hyperliquid', ${u("hl")}, 83383.0, 83400.0, 83520.5,
        0.0000125, 1, 37246.5, 30441.0, 2543485928.3, 84401.0, '2026-09-28T13:32:10Z',
        ${await record("ctx", "2026-09-28T13:32:10Z")})`;
    for (let h = 8; h < 12; h++) {
      const open = `2026-09-28T${String(h).padStart(2, "0")}:00:00Z`;
      const closeAt = `2026-09-28T${String(h + 1).padStart(2, "0")}:00:00Z`;
      await sql`INSERT INTO market_bars (subject_id, subject_category, unit_id, unit_category, source_id,
          venue_id, bar_interval, open_time, close_time, open, high, low, close, volume, received_at, source_record_id)
        VALUES (${u("perp")}, 'instrument', ${u("usdt")}, 'instrument', 'hyperliquid', ${u("hl")}, '1h',
          ${open}, ${closeAt}, ${83000 + h}::numeric, ${83100 + h}::numeric, ${82900 + h}::numeric,
          ${83050 + h}::numeric, 10::numeric, ${AT}, ${rec("hyperliquid")})`;
    }

    // B. Solana: Circle's USDC mint, a deployment that REPRESENTS USD Coin.
    await chain("solana", "Solana", SOLANA, "undrly-fixture");
    await deployment("usdcSol", "solana", "solana", "token", SOLANA_MINT, "circle");
    await edge("usdcSol", "deployment", "REPRESENTS", "usdc", "instrument", "circle");

    // C. Robinhood Chain: RHJ's NVIDIA tracker, distinct from the common stock.
    await entity("nvidia", "NVIDIA Corporation");
    await entity("rhj", "Robinhood Assets (Jersey) Limited");
    await instrument("nvda", "equity", "NVIDIA Corporation Common Stock");
    await instrument("tracker", "tokenized_security", "NVIDIA • Robinhood Token (RHJ Series 1)");
    await venue("nasdaq", "Nasdaq");
    await alias("nvda", "instrument", "NVDA");
    await alias("nasdaq", "venue", "NASDAQ");
    await identifier("nvda", "instrument", "isin", NVDA_ISIN);
    await identifier("nvda", "instrument", "figi", NVDA_FIGI);
    await identifier("tracker", "instrument", "isin", RH_ISIN);
    await sql`INSERT INTO listings (id, instrument_id, venue_id, source_id, received_at, source_record_id)
              VALUES (${await node("listing", "listing")}, ${u("nvda")}, ${u("nasdaq")},
                      'undrly-fixture', ${AT}, ${rec()})`;
    await sql`INSERT INTO listing_symbols (listing_id, venue_id, symbol, source_id, received_at, source_record_id)
              VALUES (${u("listing")}, ${u("nasdaq")}, 'NVDA', 'undrly-fixture', ${AT}, ${rec()})`;
    await edge("nvda", "instrument", "ISSUED_BY", "nvidia", "entity");
    await edge("tracker", "instrument", "ISSUED_BY", "rhj", "entity", "rhj-final-terms");
    await edge("tracker", "instrument", "TRACKS", "nvda", "instrument", "rhj-final-terms");
    await chain("robinhood", "Robinhood Chain", RH_CHAIN, "undrly-fixture");
    await deployment("rhToken", "robinhood", "eip155", "erc20", RH_TOKEN, "rhj-api");
    await edge("rhToken", "deployment", "REPRESENTS", "tracker", "instrument", "rhj-api");

    // D. Tempo: pathUSD TRACKS USD; no issuer is stored.
    await instrument("pathusd", "crypto_asset", "pathUSD");
    await alias("pathusd", "instrument", "pathUSD");
    await edge("pathusd", "instrument", "TRACKS", "usd", "currency", "tempo-rpc");
    await chain("tempo", "Tempo", TEMPO, "tempo-rpc");
    await deployment("pathusdTempo", "tempo", "eip155", "erc20", PATH_USD, "tempo-rpc");
    await edge("pathusdTempo", "deployment", "REPRESENTS", "pathusd", "instrument", "tempo-rpc");

    globalThis.fetch = (() => {
      fetchCalls++;
      return Promise.reject(new Error("the MCP server must not call upstream"));
    }) as unknown as typeof fetch;
    const api = inProcessApi(sql, { staleAfterSeconds: 300, now: () => NOW });
    ({ call, close } = await connect(api));
  });

  afterAll(async () => {
    await close?.();
    globalThis.fetch = realFetch;
    await sql?.end();
    await admin?.unsafe(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`);
    await admin?.end();
    expect(fetchCalls).toBe(0);
  });

  const ok = async (name: string, args: Record<string, unknown>) => {
    const r = await call(name, args);
    expect(r.isError, `${name} ${JSON.stringify(args)}: ${r.text}`).toBe(false);
    return r.data;
  };
  const explained = async (query: string) => {
    const e = v1.ExplainV1.parse(await ok("explain_instrument", { query }));
    expect(e.status, query).toBe("resolved");
    const c = e.candidates[0];
    if (c?.resolution.kind !== "node") throw new Error(`${query}: expected a node`);
    const rel = (type: string) => c.relationships.filter((r) => r.relationshipType === type);
    return { e, c, node: c.resolution.node, rel };
  };

  describe("A. Hyperliquid: what is BTC-PERP?", () => {
    it("a perpetual deriving from Bitcoin, priced in Tether, margined and settled in USD Coin", async () => {
      const { node, rel, c } = await explained("BTC-PERP");
      expect(node).toMatchObject({ id: text("instrument", "perp"), class: "perpetual_future" });
      expect(c.matches).toStrictEqual([
        expect.objectContaining({ rule: "alias", value: "BTC-PERP" }),
      ]);
      const names = (t: string) => rel(t).map((r) => r.object.name);
      expect(names("DERIVES_FROM")).toStrictEqual(["Bitcoin"]);
      expect(names("TRADES_ON")).toStrictEqual(["Hyperliquid"]);
      expect(names("DENOMINATED_IN")).toStrictEqual(["Tether"]);
      expect(names("MARGINED_IN")).toStrictEqual(["USD Coin"]);
      expect(names("SETTLES_IN")).toStrictEqual(["USD Coin"]);
    });

    it("get_instrument lists its one market (USDT) and the data available for it", async () => {
      const r = await ok("get_instrument", { id: text("instrument", "perp") });
      expect(r.markets).toStrictEqual([
        {
          unit: { id: text("instrument", "usdt"), kind: "asset", code: "USDT" },
          venues: [expect.objectContaining({ name: "Hyperliquid" })],
          available: {
            quote: true,
            candles: ["1h", "4h"],
            referenceHistory: false,
            derivatives: true,
          },
        },
      ]);
    });

    it("get_quote: the mark, in USDT (never USDC), with its venue book and times", async () => {
      const { quote } = await ok("get_quote", { query: "BTC-PERP" });
      expect(quote).toMatchObject({
        unit: { id: text("instrument", "usdt"), kind: "asset", code: "USDT" },
        priceType: "mark",
        price: "83383.0",
        bid: "83520.0",
        ask: "83521.0",
        basis: "venue",
        venue: { name: "Hyperliquid" },
        asOf: "2026-09-28T13:32:10Z",
        ageMs: 10_000,
        freshness: "fresh",
        aggregation: { method: "mark-with-venue-book-v1", eligibleObservations: 2 },
      });
    });

    it("get_derivatives: mark, oracle, funding and open interest in USDT; margin and settlement stay USDC", async () => {
      const r = await ok("get_derivatives", { query: "BTC-PERP" });
      expect(r.derivatives).toMatchObject({
        unit: { code: "USDT", kind: "asset" },
        markPrice: "83383.0",
        indexPrice: "83400.0",
        fundingRate: "0.0000125",
        fundingIntervalHours: 1,
        openInterest: "37246.5",
      });
      const ids = (k: string) => r.contract[k].map((x: { object: { id: string } }) => x.object.id);
      expect(ids("denominatedIn")).toStrictEqual([r.derivatives.unit.id]);
      expect(ids("marginedIn")).toStrictEqual([text("instrument", "usdc")]);
      expect(ids("settlesIn")).toStrictEqual([text("instrument", "usdc")]);
      expect(ids("marginedIn")).not.toContain(r.derivatives.unit.id);
    });

    it("get_history: bounded candles in USDT", async () => {
      const r = await ok("get_history", { query: "BTC-PERP", interval: "1h", limit: 2 });
      expect(r.candles.unit.code).toBe("USDT");
      expect(r.candles.candles.map((c: { openTime: string }) => c.openTime)).toStrictEqual([
        "2026-09-28T10:00:00Z",
        "2026-09-28T11:00:00Z",
      ]);
    });

    it("get_markets: the market, its feed and the stored source record, apart from the instrument", async () => {
      const r = await ok("get_markets", { id: text("instrument", "perp") });
      expect(r.instrument.id).toBe(text("instrument", "perp"));
      expect(r.listings).toStrictEqual([]);
      const m = r.markets[0];
      expect(m.unit.code).toBe("USDT");
      expect(m.status).toBe("continuous");
      expect(
        m.feeds.map((f: { source: { id: string }; priceType: string }) => [
          f.source.id,
          f.priceType,
        ]),
      ).toStrictEqual([
        ["hyperliquid", "mark"],
        ["hyperliquid", "mid"],
      ]);
      expect(m.feeds[0].sourceRecord.key).toBe("mark@2026-09-28T13:32:10Z");
    });
  });

  describe("B. Solana: the USDC mint by CAIP-19", () => {
    const caip19 = `caip19:${SOLANA}/token:${SOLANA_MINT}`;

    it("resolves to the deployment, which REPRESENTS USD Coin and is DEPLOYED_ON Solana", async () => {
      const { node, rel, c } = await explained(caip19);
      expect(node).toMatchObject({ id: text("deployment", "usdcSol"), kind: "deployment" });
      expect(c.matches[0]).toMatchObject({ rule: "identifier", namespace: "caip19" });
      expect(c.identifiers).toStrictEqual([
        { namespace: "caip19", value: `${SOLANA}/token:${SOLANA_MINT}` },
      ]);
      expect(rel("REPRESENTS").map((r) => [r.object.id, r.provenance.sourceId])).toStrictEqual([
        [text("instrument", "usdc"), "circle"],
      ]);
      expect(rel("DEPLOYED_ON")).toStrictEqual([
        expect.objectContaining({
          object: expect.objectContaining({ name: "Solana" }),
          projected: true,
        }),
      ]);
    });

    it("USDC resolves to USD Coin, not to the deployment; its graph lists the deployment", async () => {
      const r = await ok("resolve_instrument", { query: "USDC" });
      expect(r.match.node).toMatchObject({ id: text("instrument", "usdc"), kind: "instrument" });
      const g = await ok("get_instrument_graph", { id: text("instrument", "usdc") });
      expect(g.deployments).toStrictEqual([
        {
          id: text("deployment", "usdcSol"),
          chain: expect.objectContaining({ name: "Solana", kind: "chain" }),
          caip19: `${SOLANA}/token:${SOLANA_MINT}`,
        },
      ]);
    });

    it("the bare mint resolves to nothing; get_instrument on the deployment keeps its kind", async () => {
      expect((await ok("resolve_instrument", { query: SOLANA_MINT })).status).toBe("not_found");
      const d = await ok("get_instrument", { id: text("deployment", "usdcSol") });
      expect(d.node.kind).toBe("deployment");
      expect(d.markets).toStrictEqual([]);
    });
  });

  describe("C. Robinhood Chain: the NVIDIA token is not NVIDIA stock", () => {
    it("the CAIP-19 deployment REPRESENTS RHJ's tracker and is DEPLOYED_ON Robinhood Chain", async () => {
      const { node, rel } = await explained(`caip19:${RH_CHAIN}/erc20:${RH_TOKEN}`);
      expect(node.kind).toBe("deployment");
      const represents = rel("REPRESENTS");
      expect(represents.map((r) => [r.object.id, r.provenance.sourceId])).toStrictEqual([
        [text("instrument", "tracker"), "rhj-api"],
      ]);
      expect(rel("DEPLOYED_ON").map((r) => r.object.name)).toStrictEqual(["Robinhood Chain"]);
    });

    it("the token ISIN is the tracker: ISSUED_BY RHJ, TRACKS NVIDIA common stock, never the stock", async () => {
      const { node, rel, c } = await explained(`isin:${RH_ISIN}`);
      expect(node).toMatchObject({
        id: text("instrument", "tracker"),
        class: "tokenized_security",
      });
      expect(c.identifiers).toStrictEqual([{ namespace: "isin", value: RH_ISIN }]);
      expect(rel("ISSUED_BY").map((r) => r.object.name)).toStrictEqual([
        "Robinhood Assets (Jersey) Limited",
      ]);
      expect(rel("TRACKS").map((r) => [r.object.id, r.provenance.sourceId])).toStrictEqual([
        [text("instrument", "nvda"), "rhj-final-terms"],
      ]);
      expect(rel("TOKENIZES")).toStrictEqual([]);
      expect(node.id).not.toBe(text("instrument", "nvda"));
    });

    it("NVIDIA common stock keeps its own ISIN, FIGI, listing and issuer", async () => {
      for (const q of ["NVDA", `isin:${NVDA_ISIN}`, `figi:${NVDA_FIGI}`, "NASDAQ:NVDA"]) {
        const { node } = await explained(q);
        expect(node.id, q).toBe(text("instrument", "nvda"));
      }
      const n = await ok("get_instrument", { id: text("instrument", "nvda") });
      expect(n.identifiers).toStrictEqual([
        { namespace: "figi", value: NVDA_FIGI },
        { namespace: "isin", value: NVDA_ISIN },
      ]);
      const g = await ok("get_instrument_graph", { id: text("instrument", "nvda") });
      expect(g.listings).toStrictEqual([
        expect.objectContaining({
          venue: expect.objectContaining({ name: "Nasdaq" }),
          symbols: ["NVDA"],
        }),
      ]);
      const out = g.edges.filter((e: { subject: { id: string } }) => e.subject.id === g.root.id);
      expect(
        out.map((e: { relationshipType: string; object: { name: string } }) => [
          e.relationshipType,
          e.object.name,
        ]),
      ).toStrictEqual([["ISSUED_BY", "NVIDIA Corporation"]]);
      expect(g.edgeCounts.incoming).toStrictEqual({ TRACKS: 1 });
    });
  });

  describe("D. Tempo: pathUSD", () => {
    it("the deployment REPRESENTS pathUSD and is DEPLOYED_ON Tempo", async () => {
      const { node, rel } = await explained(`caip19:${TEMPO}/erc20:${PATH_USD}`);
      expect(node.kind).toBe("deployment");
      expect(rel("REPRESENTS").map((r) => [r.object.name, r.provenance.sourceId])).toStrictEqual([
        ["pathUSD", "tempo-rpc"],
      ]);
      expect(rel("DEPLOYED_ON").map((r) => r.object.name)).toStrictEqual(["Tempo"]);
    });

    it("pathUSD TRACKS the US Dollar and has no issuer; it is not USD, USDC or USDT", async () => {
      const { node, c } = await explained("pathUSD");
      expect(
        c.relationships.map((r) => [r.relationshipType, r.object.kind, r.object.name]),
      ).toStrictEqual([["TRACKS", "currency", "US Dollar"]]);
      const ids = new Set([node.id]);
      for (const q of ["USD", "USDC", "USDT"]) ids.add((await explained(q)).node.id);
      expect(ids.size).toBe(4);
    });
  });

  describe("semantic invariants", () => {
    it("1. a deployment is not the economic asset", async () => {
      const d = (await explained(`caip19:${SOLANA}/token:${SOLANA_MINT}`)).node;
      const a = (await explained("USDC")).node;
      expect([d.kind, a.kind]).toStrictEqual(["deployment", "instrument"]);
      expect(d.id).not.toBe(a.id);
    });

    it("2. a tracker is not the underlying security", async () => {
      const t = (await explained(`isin:${RH_ISIN}`)).node;
      const s = (await explained(`isin:${NVDA_ISIN}`)).node;
      expect([t.class, s.class]).toStrictEqual(["tokenized_security", "equity"]);
      expect(t.id).not.toBe(s.id);
    });

    it("3. a stablecoin or payment asset is not the fiat currency", async () => {
      const usd = (await explained("USD")).node;
      for (const q of ["USDC", "USDT", "pathUSD"]) {
        const n = (await explained(q)).node;
        expect(n.kind, q).toBe("instrument");
        expect(n.id, q).not.toBe(usd.id);
      }
      expect(usd.kind).toBe("currency");
    });

    it("4. price denomination, margin asset and settlement asset stay separate relationships", async () => {
      const { rel } = await explained("BTC-PERP");
      expect(rel("DENOMINATED_IN")[0]?.object.id).not.toBe(rel("MARGINED_IN")[0]?.object.id);
      expect(rel("MARGINED_IN")).toHaveLength(1);
      expect(rel("SETTLES_IN")).toHaveLength(1);
    });

    it("5–6. a feed and a listing are not instruments", async () => {
      const m = await ok("get_markets", { id: text("instrument", "nvda") });
      expect(m.listings[0].id).toMatch(/^undrly:listing:/);
      const listing = await ok("get_instrument", { id: m.listings[0].id });
      expect(listing.node.kind).toBe("listing");
      const p = await ok("get_markets", { id: text("instrument", "perp") });
      for (const f of p.markets[0].feeds) expect(Object.keys(f)).not.toContain("id");
    });

    it("7. equal symbols do not establish identity", async () => {
      const s = await ok("search_instruments", { query: "NVIDIA" });
      const kinds = s.results.map((r: { node: { kind: string; class: string | null } }) => [
        r.node.kind,
        r.node.class,
      ]);
      expect(kinds).toContainEqual(["instrument", "equity"]);
      expect(kinds).toContainEqual(["instrument", "tokenized_security"]);
      expect(kinds).toContainEqual(["entity", null]);
    });

    it("8. a bare blockchain address does not establish a deployment", async () => {
      for (const q of [SOLANA_MINT, RH_TOKEN, PATH_USD]) {
        expect((await ok("resolve_instrument", { query: q })).status, q).toBe("not_found");
      }
    });

    it("9. a missing issuer remains missing", async () => {
      const n = await ok("get_instrument_graph", { id: text("instrument", "pathusd") });
      expect(n.edgeCounts.outgoing).toStrictEqual({ TRACKS: 1 });
    });

    it("10. a missing relationship remains missing", async () => {
      const g = await ok("get_instrument_graph", { id: text("instrument", "btc") });
      expect(g.edgeCounts.outgoing).toStrictEqual({});
      const q = await call("get_quote", { query: "BTC" });
      expect(q.data.error.code).toBe("no_data");
    });
  });

  it("answers are deterministic", async () => {
    for (const [name, args] of [
      ["explain_instrument", { query: "pathUSD" }],
      ["get_instrument", { id: text("instrument", "perp") }],
      ["get_derivatives", { query: "BTC-PERP" }],
    ] as const) {
      const a = await call(name, args);
      const b = await call(name, args);
      expect(a.text).toBe(b.text);
    }
  });
});
