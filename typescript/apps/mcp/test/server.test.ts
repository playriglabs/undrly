/**
 * The MCP layer alone (docs/v1.8-mcp.md §13), over a fake Undrly API that
 * returns contract documents: tool catalog, argument validation, the error
 * model, response bounds, composition and both protocol eras. The journeys
 * over real resolver and PostgreSQL code are in `journeys.db.test.ts`.
 */
import { v1 } from "@undrly/contracts";
import { afterEach, describe, expect, it } from "vitest";
import type { ApiResponse, UndrlyApi } from "../src/api.ts";
import { ERROR_CODES } from "../src/errors.ts";
import { LIMITS } from "../src/server.ts";
import { connect, type Era } from "./connect.ts";

const uuid = (n: number) => `01920000-0000-7000-8000-${n.toString(16).padStart(12, "0")}`;
const id = (category: v1.Category, n: number) => v1.formatCanonicalId(category, uuid(n));
const ref = (category: v1.Category, n: number, name: string, cls: string | null = null) => ({
  id: id(category, n),
  kind: category,
  name,
  class: cls,
});

const PERP = ref("instrument", 1, "BTC Perpetual (Hyperliquid)", "perpetual_future");
const BTC = ref("instrument", 2, "Bitcoin", "crypto_asset");
const USDT = ref("instrument", 3, "Tether", "crypto_asset");
const USDC = ref("instrument", 4, "USD Coin", "crypto_asset");
const HL = ref("venue", 5, "Hyperliquid");
const USD = ref("currency", 6, "US Dollar");
const PROV = { sourceId: "undrly-curated", receivedAt: "2026-09-28T07:52:16Z" };
const USDT_UNIT = { id: USDT.id, kind: "asset", code: "USDT" };
const USD_UNIT = { id: USD.id, kind: "currency", code: "USD" };
const PERP_SUBJECT = {
  id: PERP.id,
  kind: "instrument",
  class: "perpetual_future",
  name: PERP.name,
};

const rel = (type: string, object: object, projected = false) => ({
  relationshipType: type,
  object,
  projected,
  provenance: PROV,
});

const explainPerp = (query: string) => ({
  schemaVersion: 1,
  query,
  parsedAs: query.startsWith("undrly:") ? "canonical_id" : "alias",
  status: "resolved",
  method: query.startsWith("undrly:") ? "canonical_id" : "alias",
  quotedPairsOnly: false,
  candidates: [
    {
      resolution: { kind: "node", node: PERP },
      matches: [
        query.startsWith("undrly:")
          ? {
              rule: "canonical_id",
              side: null,
              namespace: null,
              value: query,
              venue: null,
              source: null,
            }
          : {
              rule: "alias",
              side: null,
              namespace: "symbol",
              value: "BTC-PERP",
              venue: null,
              source: { id: "undrly-curated" },
            },
      ],
      identifiers: [],
      relationships: [
        rel("DERIVES_FROM", BTC),
        rel("SETTLES_IN", USDC),
        rel("TRADES_ON", HL),
        rel("DENOMINATED_IN", USDT),
        rel("MARGINED_IN", USDC),
      ],
      quoted: null,
    },
  ],
});

/** USD Coin: 60 perpetuals SETTLES_IN it, one Solana deployment REPRESENTS it. */
const usdcGraph = () => {
  const perps = Array.from({ length: 60 }, (_, i) =>
    ref("instrument", 100 + i, `P${i} Perpetual`, "perpetual_future"),
  );
  const deployment = ref("deployment", 50, "Solana token:EPjF");
  return {
    schemaVersion: 1,
    root: USDC,
    edges: [
      { subject: USDC, relationshipType: "TRADES_ON", object: HL, provenance: PROV },
      ...perps.map((p) => ({
        subject: p,
        relationshipType: "SETTLES_IN",
        object: USDC,
        provenance: PROV,
      })),
      {
        subject: deployment,
        relationshipType: "REPRESENTS",
        object: USDC,
        provenance: { sourceId: "circle", receivedAt: PROV.receivedAt },
      },
    ],
    listings: [],
    deployments: [
      {
        id: deployment.id,
        chain: ref("chain", 51, "Solana"),
        caip19: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/token:EPjF",
      },
    ],
  };
};

const btcGraph = {
  schemaVersion: 1,
  root: BTC,
  edges: [{ subject: PERP, relationshipType: "DERIVES_FROM", object: BTC, provenance: PROV }],
  listings: [],
  markets: [
    { unit: USD_UNIT, venues: [ref("venue", 7, "Kraken")] },
    { unit: USDT_UNIT, venues: [HL] },
  ],
};

const quote = (unit: object) => ({
  schemaVersion: 1,
  subject: { id: BTC.id, kind: "instrument", class: "crypto_asset", name: "Bitcoin" },
  unit,
  priceType: "mid",
  price: "83000.5",
  bid: "83000.0",
  ask: "83001.0",
  spread: "1.0",
  spreadBps: "0.1205",
  basis: "aggregated",
  receivedAt: "2026-09-28T16:57:16Z",
  asOf: "2026-09-28T16:57:16Z",
  ageMs: 1000,
  freshness: "fresh",
  aggregation: {
    method: "mean-venue-mid-v1",
    eligibleObservations: 2,
    computedAt: "2026-09-28T16:57:16Z",
  },
});

const derivatives = {
  schemaVersion: 1,
  subject: PERP_SUBJECT,
  unit: USDT_UNIT,
  basis: "venue",
  venue: { id: HL.id, name: HL.name },
  markPrice: "83592.0",
  indexPrice: "83608.1",
  midPrice: "83583.5",
  fundingRate: "0.0000125",
  fundingIntervalHours: 1,
  openInterest: "37246.43562",
  volume24h: "30441.02758",
  volume24hNotional: "2543485928.3153305054",
  price24hAgo: "84401.0",
  asOf: "2026-09-28T16:50:26Z",
  ageMs: 7987,
  freshness: "fresh",
};

const apiError = (status: number, code: string, message: string, extra: object = {}) => ({
  status,
  body: { schemaVersion: 1, error: { code, message, ...extra } },
});

/** The fake API: canned contract documents keyed by path; records every path. */
function fakeApi() {
  const paths: string[] = [];
  const api: UndrlyApi = async (path): Promise<ApiResponse> => {
    paths.push(path);
    const url = new URL(path, "http://undrly.test");
    const q = url.searchParams.get("q") ?? "";
    const unit = url.searchParams.get("unit");
    const route = url.pathname;
    const ok = (body: unknown) => ({ status: 200, body });
    if (route === "/v1/search") {
      const results = Array.from({ length: 20 }, (_, i) => ({
        node: ref("instrument", 200 + i, `${q} ${i}`, "equity"),
        matched: `${q} ${i}`,
        rank: "prefix",
      }));
      return ok({ schemaVersion: 1, query: q, results });
    }
    if (route === "/v1/resolve") {
      if (q.startsWith("caip19:")) return apiError(400, "bad_request", "invalid query");
      if (q === "BTC")
        return ok({
          schemaVersion: 1,
          query: q,
          status: "resolved",
          method: "alias",
          match: { kind: "node", node: BTC },
          candidates: [],
        });
      return ok({
        schemaVersion: 1,
        query: q,
        status: "not_found",
        method: null,
        match: null,
        candidates: [],
      });
    }
    if (route === "/v1/explain") {
      if (q === "BTC-PERP" || q === PERP.id) return ok(explainPerp(q));
      return ok({
        schemaVersion: 1,
        query: q,
        parsedAs: "alias",
        status: "not_found",
        method: null,
        quotedPairsOnly: false,
        candidates: [],
      });
    }
    if (route === `/v1/instruments/${encodeURIComponent(BTC.id)}/graph`) return ok(btcGraph);
    if (route === `/v1/instruments/${encodeURIComponent(USDC.id)}/graph`) return ok(usdcGraph());
    if (route.startsWith("/v1/instruments/")) return apiError(404, "not_found", "no instrument");
    if (route === "/v1/quote") {
      if (q === "BOOM")
        return { status: 500, body: { detail: 'relation "secret_table" does not exist' } };
      if (q === "GARBAGE") return ok({ price: 1 });
      if (q !== "BTC") return apiError(404, "not_found", `nothing resolves to ${q}`);
      if (unit === null) {
        return apiError(409, "ambiguous", "BTC is ambiguous; use a pair or an id", {
          candidates: [{ kind: "node", node: BTC }],
        });
      }
      return ok(quote(unit === USD.id ? USD_UNIT : USDT_UNIT));
    }
    if (route.startsWith("/v1/derivatives/")) {
      if (decodeURIComponent(route) === "/v1/derivatives/BTC-PERP") return ok(derivatives);
      return apiError(404, "no_data", "derivatives data is for perpetuals");
    }
    if (route.startsWith("/v1/candles/")) {
      return ok({
        schemaVersion: 1,
        subject: PERP_SUBJECT,
        unit: USDT_UNIT,
        interval: url.searchParams.get("interval"),
        priceType: "last",
        derived: url.searchParams.get("interval") === "4h",
        candles: [],
      });
    }
    return apiError(404, "not_found", "no such route");
  };
  return { api, paths };
}

let open: { close: () => Promise<void> } | null = null;
afterEach(async () => {
  await open?.close();
  open = null;
});
const start = async (era: Era = "legacy", log: (m: string, e: unknown) => void = () => {}) => {
  const fake = fakeApi();
  const c = await connect(fake.api, era, { log });
  open = c;
  return { ...c, paths: fake.paths };
};

describe("tool catalog", () => {
  it("lists nine read-only tools in a fixed order, each with input and output schemas", async () => {
    const { client } = await start();
    const { tools } = await client.listTools();
    expect(tools.map((t) => t.name)).toStrictEqual([
      "search_instruments",
      "resolve_instrument",
      "explain_instrument",
      "get_instrument",
      "get_instrument_graph",
      "get_quote",
      "get_markets",
      "get_history",
      "get_derivatives",
    ]);
    for (const t of tools) {
      expect(t.annotations).toMatchObject({
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      });
      expect(t.inputSchema.type).toBe("object");
      expect(t.outputSchema?.["type"]).toBe("object");
      expect(t.description?.length ?? 0).toBeGreaterThan(50);
    }
    const history = tools.find((t) => t.name === "get_history");
    expect(history?.inputSchema.properties?.["limit"]).toMatchObject({
      maximum: LIMITS.history.max,
      minimum: 1,
    });
  });

  it("serves the vocabulary resource from the contract's constants", async () => {
    const { client } = await start();
    const { resources } = await client.listResources();
    expect(resources.map((r) => r.uri)).toStrictEqual(["undrly://vocabulary"]);
    const read = await client.readResource({ uri: "undrly://vocabulary" });
    const text = (read.contents[0] as { text: string }).text;
    const vocab = JSON.parse(text);
    expect(vocab.categories.values).toStrictEqual([...v1.CATEGORIES]);
    expect(vocab.relationships.stored).toContainEqual({
      type: "REPRESENTS",
      subject: "deployment",
      object: "instrument",
    });
    expect(vocab.relationships.projections.values).toStrictEqual([
      "LISTED_ON",
      "DEPLOYED_ON",
      "TRACKED_BY",
    ]);
    expect(vocab.prices.priceTypes).toStrictEqual([...v1.PRICE_TYPES]);
    expect(vocab.invariants).toHaveLength(10);
  });

  it("offers no prompts", async () => {
    const { client } = await start();
    expect(client.getServerCapabilities()?.prompts).toBeUndefined();
  });
});

describe.each(["legacy", "modern"] as const)("protocol era %s", (era) => {
  it("connects, identifies the server and answers with structured content", async () => {
    const { client, call } = await start(era);
    expect(client.getProtocolEra()).toBe(era);
    expect(client.getServerVersion()?.name).toBe("undrly");
    const r = await call("explain_instrument", { query: "BTC-PERP" });
    expect(r.isError).toBe(false);
    expect(v1.ExplainV1.parse(r.data).candidates[0]?.resolution).toStrictEqual({
      kind: "node",
      node: PERP,
    });
    expect(JSON.parse(r.text)).toStrictEqual(r.data);
  });

  it("returns structured errors", async () => {
    const { call } = await start(era);
    const r = await call("get_quote", { query: "NOPE" });
    expect(r.isError).toBe(true);
    expect(r.data).toStrictEqual({
      error: { code: "not_found", message: "nothing resolves to NOPE" },
    });
  });
});

describe("arguments", () => {
  it("rejects out-of-bounds and unknown arguments as invalid_query", async () => {
    const { call, paths } = await start();
    for (const [name, args] of [
      ["search_instruments", { query: "x", limit: 21 }],
      ["search_instruments", { query: "" }],
      ["get_history", { query: "BTC-PERP", limit: LIMITS.history.max + 1 }],
      ["get_history", { query: "BTC-PERP", limit: 0 }],
      ["get_history", { query: "BTC-PERP", interval: "5m" }],
      ["get_history", { query: "BTC-PERP", start: "yesterday" }],
      ["get_instrument_graph", { id: BTC.id, limit: 1000 }],
      ["resolve_instrument", { query: "BTC", extra: true }],
      ["resolve_instrument", {}],
    ] as const) {
      const r = await call(name, args);
      expect(r.isError, `${name} ${JSON.stringify(args)}`).toBe(true);
      expect(r.data.error.code).toBe("invalid_query");
    }
    expect(paths).toStrictEqual([]);
  });

  it("rejects malformed identifiers as invalid_identifier before any lookup", async () => {
    const { call, paths } = await start();
    for (const [name, args] of [
      ["get_instrument", { id: "BTC" }],
      ["get_instrument", { id: "undrly:instrument:nope" }],
      ["get_markets", { id: "0x20c0000000000000000000000000000000000000" }],
      ["get_quote", { query: "BTC", unit: "USD" }],
      ["resolve_instrument", { query: "undrly:instrument:nope" }],
    ] as const) {
      const r = await call(name, args);
      expect(r.data.error.code, `${name} ${JSON.stringify(args)}`).toBe("invalid_identifier");
    }
    expect(paths).toStrictEqual([]);
  });

  it("maps the API's bad_request on an identifier query to invalid_identifier", async () => {
    const { call } = await start();
    expect((await call("resolve_instrument", { query: "caip19:bad" })).data.error.code).toBe(
      "invalid_identifier",
    );
  });
});

describe("error model", () => {
  it("uses only the documented codes", () => {
    expect(ERROR_CODES).toStrictEqual([
      "not_found",
      "ambiguous",
      "invalid_query",
      "invalid_identifier",
      "no_data",
      "unsupported",
      "internal",
    ]);
  });

  it("never leaks database errors; logs them to the server's stderr instead", async () => {
    const logged: unknown[] = [];
    const { call } = await start("legacy", (_m, e) => logged.push(e));
    for (const q of ["BOOM", "GARBAGE"]) {
      const r = await call("get_quote", { query: q });
      expect(r.data).toStrictEqual({
        error: { code: "internal", message: "Undrly could not answer this request" },
      });
      expect(r.text).not.toContain("secret_table");
    }
    expect(logged).toHaveLength(2);
  });

  it("an instrument in several units is ambiguous and names the units; unit selects one", async () => {
    const { call, paths } = await start();
    const r = await call("get_quote", { query: "BTC" });
    expect(r.data.error.code).toBe("ambiguous");
    expect(r.data.error.candidates).toStrictEqual([{ kind: "node", node: BTC }]);
    expect(r.data.error.units).toStrictEqual([USD_UNIT, USDT_UNIT]);
    const q = await call("get_quote", { query: "BTC", unit: USD.id });
    expect(q.data.quote.unit).toStrictEqual(USD_UNIT);
    expect(paths.at(-1)).toBe(`/v1/quote?q=BTC&unit=${encodeURIComponent(USD.id)}`);
  });

  it("no_data and unsupported are distinct from not_found", async () => {
    const { call } = await start();
    expect((await call("get_derivatives", { query: "NVDA" })).data.error.code).toBe("no_data");
    const g = await call("get_instrument_graph", { id: USD.id });
    expect(g.data.error.code).toBe("unsupported");
    expect(g.data.error.message).toContain("explain_instrument");
    expect((await call("get_instrument", { id: id("instrument", 999) })).data.error.code).toBe(
      "not_found",
    );
  });

  it("resolve reports not_found as its answer, not as an error", async () => {
    const { call } = await start();
    const r = await call("resolve_instrument", { query: "NOTHING" });
    expect(r.isError).toBe(false);
    expect(r.data.status).toBe("not_found");
  });
});

describe("bounds", () => {
  it("search returns at most limit results (default 10)", async () => {
    const { call } = await start();
    expect((await call("search_instruments", { query: "NV" })).data.results).toHaveLength(
      LIMITS.search.default,
    );
    expect((await call("search_instruments", { query: "NV", limit: 3 })).data.results).toHaveLength(
      3,
    );
  });

  it("history forwards a bounded limit (default 30) and its window", async () => {
    const { call, paths } = await start();
    await call("get_history", { query: "BTC-PERP" });
    expect(paths.at(-1)).toBe("/v1/candles/BTC-PERP?interval=1d&limit=30");
    const r = await call("get_history", {
      query: "BTC-PERP",
      interval: "4h",
      limit: 200,
      start: "2026-09-01T00:00:00Z",
      end: "2026-09-02T00:00:00Z",
    });
    expect(r.data.series).toBe("candles");
    expect(r.data.candles.unit).toStrictEqual(USDT_UNIT);
    expect(paths.at(-1)).toBe(
      "/v1/candles/BTC-PERP?interval=4h&limit=200&start=2026-09-01T00%3A00%3A00Z&end=2026-09-02T00%3A00%3A00Z",
    );
  });

  it("graph lists are bounded, counted and filterable without changing edges", async () => {
    const { call } = await start();
    const all = await call("get_instrument_graph", { id: USDC.id });
    expect(all.data.edges).toHaveLength(LIMITS.graph.default);
    expect(all.data.totals).toStrictEqual({ edges: 62, listings: 0, deployments: 1, markets: 0 });
    expect(all.data.truncated).toBe(true);
    expect(all.data.edgeCounts).toStrictEqual({
      outgoing: { TRADES_ON: 1 },
      incoming: { SETTLES_IN: 60, REPRESENTS: 1 },
    });
    const represents = await call("get_instrument_graph", {
      id: USDC.id,
      relationshipType: "REPRESENTS",
    });
    expect(represents.data.truncated).toBe(false);
    expect(represents.data.edges).toStrictEqual([usdcGraph().edges.at(-1)]);
    expect(represents.data.edges[0].provenance.sourceId).toBe("circle");
    const out = await call("get_instrument_graph", { id: USDC.id, direction: "outgoing" });
    expect(
      out.data.edges.map((e: { relationshipType: string }) => e.relationshipType),
    ).toStrictEqual(["TRADES_ON"]);
  });
});

describe("composition", () => {
  it("get_derivatives keeps denomination, margin and settlement apart, with provenance", async () => {
    const { call } = await start();
    const r = await call("get_derivatives", { query: "BTC-PERP" });
    expect(r.data.derivatives.unit).toStrictEqual(USDT_UNIT);
    const ids = (k: string) =>
      r.data.contract[k].map((x: { object: { id: string } }) => x.object.id);
    expect(ids("derivesFrom")).toStrictEqual([BTC.id]);
    expect(ids("denominatedIn")).toStrictEqual([USDT.id]);
    expect(ids("marginedIn")).toStrictEqual([USDC.id]);
    expect(ids("settlesIn")).toStrictEqual([USDC.id]);
    expect(ids("tradesOn")).toStrictEqual([HL.id]);
    expect(r.data.contract.denominatedIn[0].provenance).toStrictEqual(PROV);
  });

  it("is deterministic: the same call gives the same answer", async () => {
    const { call } = await start();
    const a = await call("get_instrument_graph", { id: USDC.id, limit: 5 });
    const b = await call("get_instrument_graph", { id: USDC.id, limit: 5 });
    expect(a.text).toBe(b.text);
  });
});
