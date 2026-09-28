/**
 * V1.4 cross-ecosystem identity through the API
 * (docs/v1.4-cross-ecosystem-identity.md): resolve, explain and graph over
 * spot markets, perpetuals, currencies, stablecoins, chains, deployments and
 * a tokenized security. Every row is a fixture seeded directly (addresses are
 * CAIP specification examples or generated values), never production data.
 * `fetch` is stubbed to fail: the API never contacts an upstream source.
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

// CAIP-19 / Solana namespace specification examples, and one generated mint.
const EVM_ADDRESS = "0x6b175474e89094c44da98b954eedeac495271d0f";
const SOLANA = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const SPL_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const TOKEN_MINT = "4wBqpZM9xaSheZzJSMawUKKwhdpChKbZ5eu5ky4Vigw";
const AT = "2026-09-28T00:00:00Z";

describe.skipIf(url === undefined)("V1.4 cross-ecosystem identity", () => {
  const name = `undrly_api_test_${crypto.randomUUID().replaceAll("-", "")}`;
  let admin: postgres.Sql;
  let sql: postgres.Sql;
  const realFetch = globalThis.fetch;
  let fetchCalls = 0;
  const id: Record<string, string> = {};

  const app = () => createApp(sql, { staleAfterSeconds: 300 });
  const get = async (path: string) => {
    const res = await app().request(path);
    return { status: res.status, body: (await res.json()) as Record<string, unknown> };
  };
  const u = (key: string) => id[key] ?? "";
  const text = (category: v1.Category, key: string) =>
    v1.formatCanonicalId(category, id[key] ?? "");

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
    // UUIDv7 (PostgreSQL 17 has no uuidv7()): time, version, variant, random.
    let seq = 0;
    const uuid = async () => {
      const hex = (Date.now() * 4096 + seq++).toString(16).padStart(15, "0").slice(-15);
      const rand = [...crypto.getRandomValues(new Uint8Array(8))]
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
      return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${hex.slice(12, 15)}-a${rand.slice(0, 3)}-${rand.slice(3, 15)}`;
    };
    await sql`INSERT INTO sources (id, name) VALUES ('undrly-fixture', 'V1.4 fixture')`;
    const rec = (
      await sql<{ id: string }[]>`
        INSERT INTO source_records (source_id, record_key, payload, received_at)
        VALUES ('undrly-fixture', 'identity.db.test.ts', '\\x7b7d', ${AT}) RETURNING id::text`
    )[0]?.id;
    const node = async (key: string, category: string) => {
      id[key] = await uuid();
      await sql`INSERT INTO nodes (id, category) VALUES (${id[key] ?? ""}, ${category})`;
      return id[key] ?? "";
    };
    const instrument = async (key: string, cls: string, label: string, multiplier?: string) =>
      sql`INSERT INTO instruments (id, instrument_class, name, contract_multiplier, source_record_id)
          VALUES (${await node(key, "instrument")}, ${cls}, ${label},
                  ${multiplier ?? null}::numeric, ${rec ?? ""})`;
    const venue = async (key: string, label: string) =>
      sql`INSERT INTO venues (id, name, source_record_id)
          VALUES (${await node(key, "venue")}, ${label}, ${rec ?? ""})`;
    const alias = (key: string, category: string, value: string, kind = "symbol") =>
      sql`INSERT INTO aliases (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
          VALUES (${id[key] ?? ""}, ${category}, ${value}, ${kind}, 'undrly-fixture', ${AT}, ${rec ?? ""})`;
    const edge = (s: string, sc: string, type: string, o: string, oc: string) =>
      sql`INSERT INTO graph_edges (subject_id, subject_category, relationship_type, object_id,
            object_category, source_id, received_at, source_record_id)
          VALUES (${id[s] ?? ""}, ${sc}, ${type}, ${id[o] ?? ""}, ${oc}, 'undrly-fixture', ${AT}, ${rec ?? ""})`;
    const feed = (
      source: string,
      symbol: string,
      subject: string,
      unit: string,
      unitCategory: string,
      venueKey: string,
      type: string,
    ) =>
      sql`INSERT INTO sources (id, name) VALUES (${source}, ${source}) ON CONFLICT DO NOTHING`.then(
        () => sql`INSERT INTO quote_feeds (feed_source_id, symbol, subject_id, subject_category, unit_id,
              unit_category, basis, venue_id, price_type, source_id, received_at, source_record_id)
            VALUES (${source}, ${symbol}, ${id[subject] ?? ""}, 'instrument', ${id[unit] ?? ""}, ${unitCategory},
                    'venue', ${id[venueKey] ?? ""}, ${type}, 'undrly-fixture', ${AT}, ${rec ?? ""})`,
      );
    const chain = async (key: string, label: string, ns: string, ref: string) =>
      sql`INSERT INTO chains (id, name, caip2_namespace, caip2_reference, source_record_id)
          VALUES (${await node(key, "chain")}, ${label}, ${ns}, ${ref}, ${rec ?? ""})`;
    const deployment = async (
      key: string,
      chainKey: string,
      ns: string,
      assetNs: string,
      ref: string,
    ) =>
      sql`INSERT INTO deployments (id, chain_id, chain_namespace, asset_namespace, asset_reference, source_record_id)
          VALUES (${await node(key, "deployment")}, ${id[chainKey] ?? ""}, ${ns}, ${assetNs}, ${ref}, ${rec ?? ""})`;

    // Currencies: USD is fiat; USDC is not.
    await sql`INSERT INTO currencies (id, name, source_record_id)
              VALUES (${await node("usd", "currency")}, 'US Dollar', ${rec ?? ""})`;
    await sql`INSERT INTO identifiers (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
              VALUES ('iso4217', 'USD', ${u("usd")}, 'currency', 'undrly-fixture', ${AT}, ${rec ?? ""})`;

    // Crypto: Bitcoin spot (USD at Kraken and Coinbase) and its perpetual,
    // as Hyperliquid specifies it: priced in USDT, margined and paid in USDC.
    await instrument("btc", "crypto_asset", "Bitcoin");
    await instrument("usdc", "crypto_asset", "USD Coin");
    await instrument("usdt", "crypto_asset", "Tether");
    await instrument("pepe", "crypto_asset", "Pepe");
    await instrument("perp", "perpetual_future", "BTC Perpetual (Hyperliquid)");
    await instrument(
      "kpepe",
      "perpetual_future",
      "kPEPE Perpetual (Hyperliquid, 1000 PEPE per contract)",
      "1000",
    );
    await venue("kraken", "Kraken");
    await venue("coinbase", "Coinbase Exchange");
    await venue("hyperliquid", "Hyperliquid");
    await alias("btc", "instrument", "BTC");
    await alias("usdc", "instrument", "USDC");
    await alias("usdt", "instrument", "USDT");
    await alias("perp", "instrument", "BTC-PERP");
    await alias("kpepe", "instrument", "kPEPE-PERP");
    await alias("kraken", "venue", "KRAKEN");
    await alias("hyperliquid", "venue", "HYPERLIQUID");
    await edge("perp", "instrument", "DERIVES_FROM", "btc", "instrument");
    await edge("perp", "instrument", "DENOMINATED_IN", "usdt", "instrument");
    await edge("perp", "instrument", "SETTLES_IN", "usdc", "instrument");
    await edge("perp", "instrument", "MARGINED_IN", "usdc", "instrument");
    await edge("perp", "instrument", "TRADES_ON", "hyperliquid", "venue");
    await edge("kpepe", "instrument", "DERIVES_FROM", "pepe", "instrument");
    await edge("btc", "instrument", "TRADES_ON", "kraken", "venue");
    await edge("btc", "instrument", "TRADES_ON", "coinbase", "venue");
    await feed("kraken", "XXBTZUSD", "btc", "usd", "currency", "kraken", "last");
    await feed("coinbase", "BTC-USD", "btc", "usd", "currency", "coinbase", "mid");
    await feed("hyperliquid", "BTC", "perp", "usdt", "instrument", "hyperliquid", "mark");
    await feed("hyperliquid", "kPEPE", "kpepe", "usdt", "instrument", "hyperliquid", "mark");

    // Chains and deployments. The same EVM address on Ethereum and Base; on
    // Base it is a bridged token (its own instrument) that tokenizes USDC.
    await chain("ethereum", "Ethereum", "eip155", "1");
    await chain("base", "Base", "eip155", "8453");
    await chain("solana", "Solana", "solana", SOLANA);
    await instrument("bridged", "crypto_asset", "Bridged USDC (fixture)");
    await alias("bridged", "instrument", "USDC");
    await edge("bridged", "instrument", "TOKENIZES", "usdc", "instrument");
    await deployment("usdcEth", "ethereum", "eip155", "erc20", EVM_ADDRESS);
    await deployment("usdcSol", "solana", "solana", "token", SPL_MINT);
    await deployment("bridgedBase", "base", "eip155", "erc20", EVM_ADDRESS);
    await edge("usdcEth", "deployment", "REPRESENTS", "usdc", "instrument");
    await edge("usdcSol", "deployment", "REPRESENTS", "usdc", "instrument");
    await edge("bridgedBase", "deployment", "REPRESENTS", "bridged", "instrument");

    // A common stock with its listing and ISIN, and a token of it that shares
    // its ticker as an alias but nothing else.
    await sql`INSERT INTO entities (id, entity_kind, name, source_record_id)
              VALUES (${await node("issuer", "entity")}, 'company', 'Example Issuer (fixture)', ${rec ?? ""})`;
    await instrument("stock", "equity", "Example common stock (fixture)");
    await instrument("token", "tokenized_security", "Example share token (fixture)");
    await venue("nasdaq", "Nasdaq");
    await alias("nasdaq", "venue", "NASDAQ");
    await sql`INSERT INTO identifiers (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
              VALUES ('isin', 'US67066G1040', ${u("stock")}, 'instrument', 'undrly-fixture', ${AT}, ${rec ?? ""}),
                     ('mic', 'XNAS', ${u("nasdaq")}, 'venue', 'undrly-fixture', ${AT}, ${rec ?? ""})`;
    await sql`INSERT INTO listings (id, instrument_id, venue_id, source_id, received_at, source_record_id)
              VALUES (${await node("listing", "listing")}, ${u("stock")}, ${u("nasdaq")},
                      'undrly-fixture', ${AT}, ${rec ?? ""})`;
    await sql`INSERT INTO listing_symbols (listing_id, venue_id, symbol, source_id, received_at, source_record_id)
              VALUES (${u("listing")}, ${u("nasdaq")}, 'EXMP', 'undrly-fixture', ${AT}, ${rec ?? ""})`;
    await alias("stock", "instrument", "EXMP");
    await alias("token", "instrument", "EXMP");
    await edge("stock", "instrument", "ISSUED_BY", "issuer", "entity");
    await edge("token", "instrument", "TOKENIZES", "stock", "instrument");
    await deployment("tokenSol", "solana", "solana", "token", TOKEN_MINT);
    await edge("tokenSol", "deployment", "REPRESENTS", "token", "instrument");

    globalThis.fetch = (() => {
      fetchCalls++;
      return Promise.reject(new Error("the API must not call upstream"));
    }) as unknown as typeof fetch;
  });

  afterAll(async () => {
    globalThis.fetch = realFetch;
    await sql?.end();
    await admin?.unsafe(`DROP DATABASE IF EXISTS "${name}" WITH (FORCE)`);
    await admin?.end();
    expect(fetchCalls).toBe(0);
  });

  type Resolve = v1.ResolveResultV1;
  const resolve = async (q: string) =>
    (await get(`/v1/resolve?q=${encodeURIComponent(q)}`)).body as unknown as Resolve;
  const explainOf = async (q: string) => {
    const { status, body } = await get(`/v1/explain?q=${encodeURIComponent(q)}`);
    expect(status).toBe(200);
    return v1.ExplainV1.parse(body);
  };
  const graphOf = async (key: string) => {
    const { status, body } = await get(`/v1/instruments/${text("instrument", key)}/graph`);
    expect(status).toBe(200);
    return v1.GraphV1.parse(body);
  };
  const nodeOf = (r: Resolve) => (r.match?.kind === "node" ? r.match.node : null);

  it("a perpetual and its spot underlying are distinct instruments", async () => {
    const perp = nodeOf(await resolve("BTC-PERP"));
    const btc = nodeOf(await resolve("BTC"));
    expect(perp?.class).toBe("perpetual_future");
    expect(btc?.class).toBe("crypto_asset");
    expect(perp?.id).not.toBe(btc?.id);
  });

  it("the perpetual derives from the underlying; the spot market is (Bitcoin, USD)", async () => {
    const perp = await graphOf("perp");
    const edges = perp.edges.map((e) => [e.subject.id, e.relationshipType, e.object.id]);
    expect(edges).toStrictEqual([
      [text("instrument", "perp"), "DERIVES_FROM", text("instrument", "btc")],
      [text("instrument", "perp"), "DENOMINATED_IN", text("instrument", "usdt")],
      [text("instrument", "perp"), "SETTLES_IN", text("instrument", "usdc")],
      [text("instrument", "perp"), "MARGINED_IN", text("instrument", "usdc")],
      [text("instrument", "perp"), "TRADES_ON", text("venue", "hyperliquid")],
    ]);
    // Priced in its USDT denomination at Hyperliquid: not USD, not its USDC margin.
    expect(perp.markets).toStrictEqual([
      {
        unit: { id: text("instrument", "usdt"), kind: "asset", code: "USDT" },
        venues: [expect.objectContaining({ id: text("venue", "hyperliquid") })],
      },
    ]);
    // From the underlying: its USD market at two venues, and the perpetual
    // that derives from it (the inverse of DERIVES_FROM, i.e. UNDERLYING_OF).
    const btc = await graphOf("btc");
    expect(
      btc.markets?.map((m) => [m.unit.kind, m.unit.id, m.venues.map((v) => v.name).sort()]),
    ).toStrictEqual([["currency", text("currency", "usd"), ["Coinbase Exchange", "Kraken"]]]);
    expect(
      btc.edges.filter((e) => e.relationshipType === "DERIVES_FROM").map((e) => e.subject.id),
    ).toStrictEqual([text("instrument", "perp")]);
  });

  it("BTC/USD and BTC/USDC are different markets; USD, USDC and USDT are different nodes", async () => {
    // The spot market is priced in the USD currency, the perpetual in the
    // USDT asset (margined in USDC): three units, never interchangeable.
    const spot = (await resolve("BTC/USD")).match;
    const perp = (await resolve("HYPERLIQUID:BTC")).match;
    if (spot?.kind !== "pair" || perp?.kind !== "pair") throw new Error("expected pairs");
    expect(spot.subject.id).toBe(text("instrument", "btc"));
    expect(perp.subject.id).toBe(text("instrument", "perp"));
    expect(spot.unit).toStrictEqual({ id: text("currency", "usd"), kind: "currency", code: "USD" });
    expect(perp.unit).toStrictEqual({
      id: text("instrument", "usdt"),
      kind: "asset",
      code: "USDT",
    });
    const three = [
      nodeOf(await resolve("USD")),
      nodeOf(await resolve("USDT")),
      nodeOf(await resolve(text("instrument", "usdc"))),
    ];
    expect(three.map((n) => n?.kind)).toStrictEqual(["currency", "instrument", "instrument"]);
    expect(new Set(three.map((n) => n?.id)).size).toBe(3);
    // No stablecoin is an alias or identifier of another, or of USD.
    expect(nodeOf(await resolve("USDT"))?.id).toBe(text("instrument", "usdt"));
    expect((await resolve("USDT/USD")).match).toMatchObject({
      subject: { id: text("instrument", "usdt") },
      unit: { id: text("currency", "usd") },
    });
    expect((await explainOf("BTC/USD")).candidates[0]?.quoted).toBe(true);

    const usd = nodeOf(await resolve("USD"));
    expect(usd).toMatchObject({ kind: "currency", id: text("currency", "usd") });
    // `USDC` is also the bridged token's symbol: a symbol is not identity, so
    // it is ambiguous, never guessed, and never the USD currency.
    const usdc = await resolve("USDC");
    expect(usdc.status).toBe("ambiguous");
    expect(usdc.candidates.map((c) => (c.kind === "node" ? c.node.id : null)).sort()).toStrictEqual(
      [text("instrument", "usdc"), text("instrument", "bridged")].sort(),
    );
    expect(usdc.candidates.every((c) => c.kind === "node" && c.node.kind !== "currency")).toBe(
      true,
    );
    // BTC/USDC therefore has two readings, neither quoted: not found, and
    // explain says the quoted-pair rule removed both.
    const e = await explainOf("BTC/USDC");
    expect([e.status, e.quotedPairsOnly, e.candidates]).toStrictEqual(["not_found", true, []]);
  });

  it("a stablecoin keeps one identity with separately identified deployments", async () => {
    const usdc = await graphOf("usdc");
    expect(usdc.deployments).toStrictEqual(
      [
        {
          id: text("deployment", "usdcEth"),
          chain: expect.objectContaining({ id: text("chain", "ethereum"), kind: "chain" }),
          caip19: `eip155:1/erc20:${EVM_ADDRESS}`,
        },
        {
          id: text("deployment", "usdcSol"),
          chain: expect.objectContaining({ id: text("chain", "solana"), kind: "chain" }),
          caip19: `solana:${SOLANA}/token:${SPL_MINT}`,
        },
      ].sort((a, b) => a.id.localeCompare(b.id)),
    );
    // The bridged token is its own instrument that tokenizes USDC; its Base
    // deployment is not a USDC deployment.
    expect(usdc.deployments?.map((d) => d.id)).not.toContain(text("deployment", "bridgedBase"));
    const bridged = await graphOf("bridged");
    expect(bridged.deployments?.map((d) => d.caip19)).toStrictEqual([
      `eip155:8453/erc20:${EVM_ADDRESS}`,
    ]);
    expect(
      bridged.edges
        .filter((e) => e.subject.id === text("instrument", "bridged"))
        .map((e) => [e.relationshipType, e.object.id]),
    ).toStrictEqual([["TOKENIZES", text("instrument", "usdc")]]);
  });

  it("an address identifies a deployment only together with its chain", async () => {
    const onEthereum = nodeOf(await resolve(`caip19:eip155:1/erc20:${EVM_ADDRESS}`));
    const onBase = nodeOf(await resolve(`caip19:eip155:8453/erc20:${EVM_ADDRESS}`));
    expect(onEthereum?.id).toBe(text("deployment", "usdcEth"));
    expect(onBase?.id).toBe(text("deployment", "bridgedBase"));
    // Hex case does not matter; the address alone, or on another chain, matches nothing.
    const upper = `caip19:eip155:1/erc20:0x${EVM_ADDRESS.slice(2).toUpperCase()}`;
    expect(nodeOf(await resolve(upper))?.id).toBe(text("deployment", "usdcEth"));
    expect((await resolve(EVM_ADDRESS)).status).toBe("not_found");
    // A Solana mint alone is not identity either: it never resolves to the
    // deployment, and never straight to the asset it represents.
    expect((await resolve(SPL_MINT)).status).toBe("not_found");
    expect((await resolve(`caip19:eip155:10/erc20:${EVM_ADDRESS}`)).status).toBe("not_found");
    // A Solana mint is not an EVM address and vice versa; base58 is case-sensitive.
    expect((await resolve(`caip19:eip155:1/token:${SPL_MINT}`)).status).toBe("not_found");
    expect((await resolve(`caip19:solana:${SOLANA}/erc20:${EVM_ADDRESS}`)).status).toBe(
      "not_found",
    );
    expect((await resolve(`caip19:solana:${SOLANA}/token:${SPL_MINT.toLowerCase()}`)).status).toBe(
      "not_found",
    );
    expect(nodeOf(await resolve(`caip19:solana:${SOLANA}/token:${SPL_MINT}`))?.id).toBe(
      text("deployment", "usdcSol"),
    );
    expect(nodeOf(await resolve("caip2:eip155:8453"))).toMatchObject({
      id: text("chain", "base"),
      kind: "chain",
    });
    // Malformed CAIP text is a bad request, not a guess.
    expect((await get("/v1/resolve?q=caip19:eip155:1")).status).toBe(400);
  });

  it("explains a deployment: its chain, what it represents, its CAIP-19 id", async () => {
    const e = await explainOf(`caip19:eip155:1/erc20:${EVM_ADDRESS}`);
    expect(e).toMatchObject({ parsedAs: "identifier", status: "resolved", method: "identifier" });
    const c = e.candidates[0];
    expect(c?.matches).toStrictEqual([
      {
        rule: "identifier",
        side: null,
        namespace: "caip19",
        value: `eip155:1/erc20:${EVM_ADDRESS}`,
        venue: null,
        source: null,
      },
    ]);
    expect(c?.identifiers).toStrictEqual([
      { namespace: "caip19", value: `eip155:1/erc20:${EVM_ADDRESS}` },
    ]);
    expect(
      c?.relationships.map((r) => [r.relationshipType, r.object.id, r.projected]),
    ).toStrictEqual([
      ["REPRESENTS", text("instrument", "usdc"), false],
      ["DEPLOYED_ON", text("chain", "ethereum"), true],
    ]);
    expect(c?.relationships.every((r) => r.provenance.sourceId === "undrly-fixture")).toBe(true);
  });

  it("a tokenized security is not the security: separate ids, identifiers and venues", async () => {
    // The ISIN names only the stock.
    expect(nodeOf(await resolve("isin:US67066G1040"))?.id).toBe(text("instrument", "stock"));
    // The shared ticker is ambiguous, and explain shows why each matched.
    const e = await explainOf("EXMP");
    expect(e.status).toBe("ambiguous");
    expect(
      e.candidates
        .map((c) => (c.resolution.kind === "node" ? c.resolution.node.class : null))
        .sort(),
    ).toStrictEqual(["equity", "tokenized_security"]);
    for (const c of e.candidates) {
      expect(c.matches.map((m) => [m.rule, m.namespace, m.value])).toStrictEqual([
        ["alias", "symbol", "EXMP"],
      ]);
    }
    const stock = e.candidates.find(
      (c) => c.resolution.kind === "node" && c.resolution.node.class === "equity",
    );
    const token = e.candidates.find(
      (c) => c.resolution.kind === "node" && c.resolution.node.class === "tokenized_security",
    );
    expect(stock?.identifiers).toStrictEqual([{ namespace: "isin", value: "US67066G1040" }]);
    expect(
      stock?.relationships.map((r) => [r.relationshipType, r.object.name, r.projected]),
    ).toStrictEqual([
      ["ISSUED_BY", "Example Issuer (fixture)", false],
      ["LISTED_ON", "Nasdaq", true],
    ]);
    expect(token?.identifiers).toStrictEqual([]);
    expect(token?.relationships.map((r) => [r.relationshipType, r.object.id])).toStrictEqual([
      ["TOKENIZES", text("instrument", "stock")],
    ]);
    // Venue symbol → the listing's instrument only; the token is on a chain.
    expect(nodeOf(await resolve("NASDAQ:EXMP"))?.id).toBe(text("instrument", "stock"));
    expect(nodeOf(await resolve("XNAS:EXMP"))?.id).toBe(text("instrument", "stock"));
    const tokenGraph = await graphOf("token");
    expect(tokenGraph.listings).toStrictEqual([]);
    expect(tokenGraph.deployments?.map((d) => d.caip19)).toStrictEqual([
      `solana:${SOLANA}/token:${TOKEN_MINT}`,
    ]);
    const stockGraph = await graphOf("stock");
    expect(stockGraph.deployments).toBeUndefined();
    expect(stockGraph.listings.map((l) => l.symbols)).toStrictEqual([["EXMP"]]);
    expect(
      stockGraph.edges.filter((e) => e.relationshipType === "TOKENIZES").map((e) => e.subject.id),
    ).toStrictEqual([text("instrument", "token")]);
  });

  it("a provider feed is not an instrument: it resolves to the market it prices", async () => {
    const e = await explainOf("KRAKEN:XXBTZUSD");
    expect(e).toMatchObject({
      parsedAs: "venue_symbol",
      status: "resolved",
      method: "feed_symbol",
    });
    const c = e.candidates[0];
    if (c?.resolution.kind !== "pair") throw new Error("expected a pair");
    expect(c.resolution.subject.id).toBe(text("instrument", "btc"));
    expect(c.matches).toStrictEqual([
      {
        rule: "feed_symbol",
        side: null,
        namespace: null,
        value: "XXBTZUSD",
        venue: expect.objectContaining({ id: text("venue", "kraken") }),
        source: { id: "kraken" },
      },
    ]);
  });

  it("explains a perpetual: derivative, underlying, price unit, settlement and margin, venue", async () => {
    const e = await explainOf("BTC-PERP");
    const c = e.candidates[0];
    expect(c?.resolution).toMatchObject({ kind: "node", node: { class: "perpetual_future" } });
    expect(c?.matches.map((m) => [m.rule, m.namespace, m.value, m.source?.id])).toStrictEqual([
      ["alias", "symbol", "BTC-PERP", "undrly-fixture"],
    ]);
    expect(c?.relationships.map((r) => [r.relationshipType, r.object.name])).toStrictEqual([
      ["DERIVES_FROM", "Bitcoin"],
      ["DENOMINATED_IN", "Tether"],
      ["SETTLES_IN", "USD Coin"],
      ["MARGINED_IN", "USD Coin"],
      ["TRADES_ON", "Hyperliquid"],
    ]);
  });

  it("keeps contract multipliers on perpetual markets", async () => {
    const r = await resolve("HYPERLIQUID:kPEPE");
    if (r.match?.kind !== "pair") throw new Error("expected a pair");
    expect(r.match.subject).toMatchObject({
      class: "perpetual_future",
      contractMultiplier: "1000",
    });
    expect(r.match.unit.id).toBe(text("instrument", "usdt"));
    const e = await explainOf("kPEPE-PERP");
    expect(
      e.candidates[0]?.relationships.map((r) => [r.relationshipType, r.object.name]),
    ).toStrictEqual([["DERIVES_FROM", "Pepe"]]);
  });

  it("explain agrees with resolve and both are deterministic", async () => {
    for (const q of [
      "BTC",
      "USDC",
      "BTC/USD",
      "BTC/USDC",
      "EXMP",
      "isin:US67066G1040",
      "KRAKEN:XXBTZUSD",
      "NASDAQ:EXMP",
      `caip19:eip155:1/erc20:${EVM_ADDRESS}`,
      "caip2:eip155:1",
      text("instrument", "perp"),
      "nothing-matches-this",
    ]) {
      const r = await resolve(q);
      const e = await explainOf(q);
      expect([e.status, e.method], q).toStrictEqual([r.status, r.method]);
      const listed = r.status === "resolved" ? [r.match] : r.candidates;
      expect(
        e.candidates.map((c) => c.resolution),
        q,
      ).toStrictEqual(listed);
      expect(await explainOf(q), q).toStrictEqual(e);
    }
    expect(await graphOf("usdc")).toStrictEqual(await graphOf("usdc"));
    expect(await graphOf("btc")).toStrictEqual(await graphOf("btc"));
  });

  it("serves no JSON numbers besides schemaVersion (no floating point)", async () => {
    const numbers: string[] = [];
    const walk = (value: unknown, path: string) => {
      if (typeof value === "number" && !path.endsWith(".schemaVersion")) numbers.push(path);
      if (value !== null && typeof value === "object") {
        for (const [k, v] of Object.entries(value)) walk(v, `${path}.${k}`);
      }
    };
    walk(await graphOf("perp"), "graph");
    walk(await graphOf("usdc"), "graph");
    walk(await explainOf("BTC-PERP"), "explain");
    walk(await explainOf("HYPERLIQUID:kPEPE"), "explain");
    expect(numbers).toStrictEqual([]);
  });
});
