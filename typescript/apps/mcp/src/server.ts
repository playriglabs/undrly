/**
 * Undrly as an MCP server (docs/v1.8-mcp.md): a thin, read-only, stateless
 * interface over the Undrly API. Every fact comes from an API route (the
 * same resolver, explain, graph and market-data code as REST); this module
 * only validates arguments, bounds response sizes, composes routes and maps
 * errors. It never resolves, infers, ranks or writes anything itself.
 */
import { McpServer, type StandardSchemaWithJSON } from "@modelcontextprotocol/server";
import { v1 } from "@undrly/contracts";
import { z } from "zod";
import type { UndrlyApi } from "./api.ts";
import { apiFailure, type ErrorCode, ToolErrorV1, ToolFailure } from "./errors.ts";
import { VOCABULARY } from "./vocabulary.ts";

export const SERVER_NAME = "undrly";
export const SERVER_VERSION = "0.1.0";

/** Response-size bounds (docs/v1.8-mcp.md §10). */
export const LIMITS = {
  search: { default: 10, max: 20 },
  graph: { default: 50, max: 200 },
  history: { default: 30, max: 200 },
  /** Markets per instrument probed by get_instrument and get_markets. */
  markets: 10,
} as const;

const INSTRUCTIONS = `Undrly is a deterministic financial identity and market-data service. It provides stored facts; it never guesses.

Typical flow: resolve_instrument or explain_instrument (any ticker, pair, ISIN, FIGI, canonical id, caip2:…, caip19:…) → get_instrument with the canonical id (identifiers and which market data exists) → get_instrument_graph (relationships with provenance) → get_quote / get_markets / get_history / get_derivatives.

Read the undrly://vocabulary resource for the node categories, relationship types, price types and identifier forms.

Keep these distinctions: a deployment is not the asset it REPRESENTS; a tracker is not the security it TRACKS; a stablecoin is not the currency it TRACKS; a perpetual's price unit (DENOMINATED_IN), margin (MARGINED_IN) and settlement (SETTLES_IN) are separate; a feed or a listing is not an instrument; equal symbols and bare contract addresses are not identity; a missing issuer or relationship is unknown, not absent in reality. search_instruments is discovery only; resolution is resolve_instrument. An ambiguous result lists candidates and picks none: choose one by its canonical id.`;

const READ_ONLY = {
  readOnlyHint: true,
  destructiveHint: false,
  idempotentHint: true,
  openWorldHint: false,
} as const;

// --- arguments ----------------------------------------------------------------

const Query = z
  .string()
  .trim()
  .min(1)
  .max(256)
  .describe(
    "Ticker, symbol, name, BASE/QUOTE pair, VENUE:SYMBOL, isin:/figi:/lei:/cik:/mic:/iso4217: identifier, caip2:<chain>, caip19:<chain>/<asset>, or a canonical undrly:<category>:<id>.",
  );

const canonicalId = (categories: readonly v1.Category[] | null, what: string) =>
  z
    .string()
    .trim()
    .refine((id) => {
      const category = v1.canonicalIdCategory(id);
      return category !== null && (categories === null || categories.includes(category));
    }, `expected ${what}`)
    .describe(`${what} (undrly:<category>:<id>), as returned by resolve or explain.`);

const AnyId = canonicalId(null, "a canonical Undrly id");
/** Any canonical id is accepted so another kind is `unsupported`, not malformed. */
const InstrumentId = canonicalId(null, "an instrument id");
const UnitId = canonicalId(["currency", "instrument"], "a currency or instrument id")
  .optional()
  .describe(
    "Optional price unit: the canonical id of a currency or asset (get_instrument lists each market's unit). Selects one market when an instrument is priced in several units.",
  );

/** Argument fields whose failures are `invalid_identifier`. */
const IDENTIFIER_ARGS = new Set(["id", "unit"]);

/**
 * Advertises `schema` in `tools/list` but lets every argument through the
 * SDK, so the handler validates with the same schema and answers a failure
 * with a structured `invalid_query` / `invalid_identifier` error instead of
 * the SDK's plain-text one.
 */
function advertised(schema: z.ZodType): StandardSchemaWithJSON {
  const std = schema["~standard"] as unknown as StandardSchemaWithJSON["~standard"];
  return {
    "~standard": {
      version: 1,
      vendor: "undrly",
      validate: (value: unknown) => ({ value }),
      jsonSchema: std.jsonSchema,
    },
  };
}

// --- outputs ------------------------------------------------------------------

const Provenanced = z.strictObject({ object: v1.NodeRefV1, provenance: v1.ProvenanceV1 });

const Availability = z.strictObject({
  /** A canonical quote is served now (a stale multi-source aggregate is not). */
  quote: z.boolean(),
  /** Candle intervals with at least one stored bar. */
  candles: z.array(z.enum(v1.CANDLE_INTERVALS)),
  /** A published reference or average series. */
  referenceHistory: z.boolean(),
  /** Perpetual context (mark, index, funding, open interest). */
  derivatives: z.boolean(),
});

const GetInstrumentV1 = z.strictObject({
  node: v1.NodeRefV1,
  identifiers: z.array(z.strictObject({ namespace: z.string(), value: z.string() })),
  markets: z.array(
    z.strictObject({
      unit: v1.PriceUnitV1,
      venues: z.array(v1.NodeRefV1),
      available: Availability,
    }),
  ),
  marketsTotal: z.number().int().min(0),
});

const edgeCounts = z.record(z.string(), z.number().int().min(1));

const GraphOutV1 = z.strictObject({
  root: v1.NodeRefV1,
  edges: v1.GraphV1.shape.edges,
  edgeCounts: z.strictObject({ outgoing: edgeCounts, incoming: edgeCounts }),
  listings: v1.GraphV1.shape.listings,
  deployments: z.array(v1.DeploymentRefV1),
  markets: z.array(z.strictObject({ unit: v1.PriceUnitV1, venues: z.array(v1.NodeRefV1) })),
  totals: z.strictObject({
    edges: z.number().int().min(0),
    listings: z.number().int().min(0),
    deployments: z.number().int().min(0),
    markets: z.number().int().min(0),
  }),
  truncated: z.boolean(),
});

const FeedV1 = z.strictObject({
  source: z.strictObject({ id: v1.SourceId }),
  venue: v1.VenueRefV1.nullable(),
  priceType: z.enum(v1.PRICE_TYPES),
  basis: z.enum(v1.OBSERVATION_BASES),
  price: v1.DecimalString,
  bid: v1.DecimalString.nullable(),
  ask: v1.DecimalString.nullable(),
  observedAt: v1.TimestampString.nullable(),
  receivedAt: v1.TimestampString,
  freshness: z.enum(["fresh", "stale"]),
  sourceRecord: z.strictObject({ id: z.string(), key: z.string() }),
});

const MarketsV1 = z.strictObject({
  instrument: v1.NodeRefV1,
  listings: v1.GraphV1.shape.listings,
  markets: z.array(
    z.strictObject({
      unit: v1.PriceUnitV1,
      venues: z.array(v1.NodeRefV1),
      status: z.enum(v1.MARKET_STATUSES).nullable(),
      statistics: v1.MarketV1.shape.statistics,
      feeds: z.array(FeedV1),
    }),
  ),
  marketsTotal: z.number().int().min(0),
});

const QuoteOutV1 = z.strictObject({ quote: v1.QuoteV1 });

const HistoryOutV1 = z.strictObject({
  series: z.enum(["candles", "reference"]),
  candles: v1.CandlesV1.optional(),
  history: v1.HistoryV1.optional(),
});

const DerivativesOutV1 = z.strictObject({
  derivatives: v1.DerivativesV1,
  contract: z.strictObject({
    derivesFrom: z.array(Provenanced),
    denominatedIn: z.array(Provenanced),
    marginedIn: z.array(Provenanced),
    settlesIn: z.array(Provenanced),
    tradesOn: z.array(Provenanced),
  }),
});

// --- server -------------------------------------------------------------------

const enc = encodeURIComponent;
const unitParam = (unit: string | undefined) => (unit === undefined ? "" : `&unit=${enc(unit)}`);

function textOf(value: unknown) {
  return [{ type: "text" as const, text: JSON.stringify(value) }];
}

function errorResult(code: ErrorCode, message: string, details: object = {}) {
  const structuredContent = ToolErrorV1.parse({ error: { code, message, ...details } });
  return { isError: true, content: textOf(structuredContent), structuredContent };
}

export type ServerOptions = {
  /** Where unexpected failures are reported (stderr by default; never to the client). */
  log?: (message: string, error: unknown) => void;
};

export function createUndrlyMcpServer(api: UndrlyApi, options: ServerOptions = {}): McpServer {
  const log = options.log ?? ((m, e) => console.error(`undrly-mcp: ${m}`, e));
  const server = new McpServer(
    { name: SERVER_NAME, title: "Undrly", version: SERVER_VERSION },
    {
      instructions: INSTRUCTIONS,
      cacheHints: {
        "tools/list": { ttlMs: 3_600_000, cacheScope: "public" },
        "resources/list": { ttlMs: 3_600_000, cacheScope: "public" },
        "server/discover": { ttlMs: 3_600_000, cacheScope: "public" },
      },
    },
  );

  /** `GET path`, parsed with `schema`; an API error becomes a ToolFailure. */
  async function get<T>(schema: z.ZodType<T>, path: string, query: string | null): Promise<T> {
    const res = await api(path);
    if (res.status !== 200) {
      if (res.status >= 500) throw new Error(`${path}: HTTP ${res.status}`);
      throw apiFailure(res, query);
    }
    return schema.parse(res.body);
  }

  /** Like `get`, but "no such data" (404) is `null`. */
  async function probe<T>(schema: z.ZodType<T>, path: string): Promise<T | null> {
    const res = await api(path);
    if (res.status === 404) return null;
    if (res.status !== 200) throw new Error(`${path}: HTTP ${res.status}`);
    return schema.parse(res.body);
  }

  function tool<I extends z.ZodType, O extends z.ZodType>(
    name: string,
    config: { title: string; description: string; input: I; output: O },
    run: (args: z.output<I>) => Promise<z.input<O>>,
  ) {
    server.registerTool(
      name,
      {
        title: config.title,
        description: config.description,
        inputSchema: advertised(config.input),
        outputSchema: config.output as unknown as StandardSchemaWithJSON,
        annotations: READ_ONLY,
      },
      async (raw: unknown) => {
        const args = config.input.safeParse(raw ?? {});
        if (!args.success) {
          const issue = args.error.issues[0];
          const field = String(issue?.path[0] ?? "");
          const message = `${field === "" ? "" : `${field}: `}${issue?.message ?? "invalid arguments"}`;
          return errorResult(
            IDENTIFIER_ARGS.has(field) ? "invalid_identifier" : "invalid_query",
            message,
          );
        }
        try {
          const out = config.output.parse(await run(args.data));
          return { content: textOf(out), structuredContent: out as Record<string, unknown> };
        } catch (e) {
          if (e instanceof ToolFailure) return errorResult(e.code, e.message, e.details);
          log(`${name} failed`, e);
          return errorResult("internal", "Undrly could not answer this request");
        }
      },
    );
  }

  /** An id's node, or `not_found` (explain is the one lookup of any node kind). */
  async function nodeOf(id: string) {
    const e = await get(v1.ExplainV1, `/v1/explain?q=${enc(id)}`, id);
    const c = e.candidates[0];
    if (e.status !== "resolved" || c === undefined || c.resolution.kind !== "node") {
      throw new ToolFailure("not_found", `no ${id}`);
    }
    return { node: c.resolution.node, candidate: c };
  }

  async function instrumentGraph(id: string) {
    return get(v1.GraphV1, `/v1/instruments/${enc(id)}/graph`, id);
  }

  /** Rejects valid ids of other kinds with `unsupported`. */
  function requireInstrument(id: string, tool: string) {
    const category = v1.canonicalIdCategory(id);
    if (category !== "instrument") {
      throw new ToolFailure(
        "unsupported",
        `${tool} is for instruments; ${id} is a ${category}. Use explain_instrument for its relationships.`,
      );
    }
  }

  /** Canonical-id syntax is checked before the resolver sees it. */
  function checkQuery(query: string) {
    if (/^undrly:/i.test(query) && v1.canonicalIdCategory(query) === null) {
      throw new ToolFailure("invalid_identifier", `not a canonical Undrly id: ${query}`);
    }
  }

  // 1. search -------------------------------------------------------------------
  tool(
    "search_instruments",
    {
      title: "Search instruments",
      description:
        "Discover candidate financial objects (instruments, currencies, entities, venues, listings, chains, deployments) whose symbol or name matches text, best match first. Discovery only: a result is not a resolution and shares a symbol or name only. Use resolve_instrument or explain_instrument to identify.",
      input: z.strictObject({
        query: Query,
        limit: z
          .number()
          .int()
          .min(1)
          .max(LIMITS.search.max)
          .default(LIMITS.search.default)
          .describe(`Results to return (1–${LIMITS.search.max}).`),
      }),
      output: v1.SearchResultV1,
    },
    async ({ query, limit }) => {
      const r = await get(v1.SearchResultV1, `/v1/search?q=${enc(query)}`, query);
      return { ...r, results: r.results.slice(0, limit) };
    },
  );

  // 2. resolve ------------------------------------------------------------------
  tool(
    "resolve_instrument",
    {
      title: "Resolve an identifier",
      description:
        "Deterministically resolve a query to one Undrly object (status resolved), several (ambiguous: candidates listed, none chosen), or nothing (not_found). Same result as GET /v1/resolve. A pair (EUR/USD, BTC/USD) resolves to a market: subject priced in unit.",
      input: z.strictObject({ query: Query }),
      output: v1.ResolveResultV1,
    },
    async ({ query }) => {
      checkQuery(query);
      return get(v1.ResolveResultV1, `/v1/resolve?q=${enc(query)}`, query);
    },
  );

  // 3. explain ------------------------------------------------------------------
  tool(
    "explain_instrument",
    {
      title: "Explain a resolution",
      description:
        "Why a query resolves the way it does (GET /v1/explain): the resolver's status and method, and per candidate the rules that matched with the stored value, its external identifiers, and its direct relationships (canonical direction, plus projected LISTED_ON, DEPLOYED_ON, TRACKED_BY) each with provenance (source and received time). No scores: a rule matched exactly or not at all.",
      input: z.strictObject({ query: Query }),
      output: v1.ExplainV1,
    },
    async ({ query }) => {
      checkQuery(query);
      return get(v1.ExplainV1, `/v1/explain?q=${enc(query)}`, query);
    },
  );

  // 4. get_instrument -----------------------------------------------------------
  tool(
    "get_instrument",
    {
      title: "Get an object by canonical id",
      description:
        "The canonical record of a known Undrly object by its canonical id (any category): its name, kind and class, its current external identifiers (ISIN, FIGI, CAIP-2, CAIP-19, …), and for an instrument each market (price unit) Undrly has with the data available for it (quote, candle intervals, reference history, derivatives context). Relationships are in get_instrument_graph (instruments) or explain_instrument (any object).",
      input: z.strictObject({ id: AnyId }),
      output: GetInstrumentV1,
    },
    async ({ id }) => {
      const { node, candidate } = await nodeOf(id);
      const markets = [];
      let marketsTotal = 0;
      if (node.kind === "instrument") {
        const graph = await instrumentGraph(id);
        const all = graph.markets ?? [];
        marketsTotal = all.length;
        for (const m of all.slice(0, LIMITS.markets)) {
          const at = (route: string, extra = "") =>
            `/v1/${route}/${enc(id)}?unit=${enc(m.unit.id)}${extra}`;
          const candles: v1.CandleInterval[] = [];
          for (const interval of v1.CANDLE_INTERVALS) {
            const c = await probe(v1.CandlesV1, at("candles", `&interval=${interval}&limit=1`));
            if (c !== null && c.candles.length > 0) candles.push(interval);
          }
          const history = await probe(v1.HistoryV1, at("history", "&limit=1"));
          markets.push({
            unit: m.unit,
            venues: m.venues,
            available: {
              quote: (await probe(v1.QuoteV1, at("quote"))) !== null,
              candles,
              referenceHistory: history !== null && history.observations.length > 0,
              derivatives:
                node.class === "perpetual_future" &&
                (await probe(v1.DerivativesV1, at("derivatives"))) !== null,
            },
          });
        }
      }
      return { node, identifiers: candidate.identifiers, markets, marketsTotal };
    },
  );

  // 5. graph --------------------------------------------------------------------
  tool(
    "get_instrument_graph",
    {
      title: "Instrument relationships",
      description:
        "One hop of the relationship graph around an instrument, in both directions (GET /v1/instruments/{id}/graph): stored edges in canonical direction (subject → object) with provenance, its listings (venue + symbols), the deployments that REPRESENT it (chain + CAIP-19), and its markets (price unit + feed venues). edgeCounts summarises all edges by direction and type; lists are bounded by limit (totals and truncated say what was cut). Filter with relationshipType and direction.",
      input: z.strictObject({
        id: InstrumentId,
        relationshipType: z.enum(v1.RELATIONSHIP_TYPES).optional().describe("Only this edge type."),
        direction: z
          .enum(["outgoing", "incoming", "both"])
          .default("both")
          .describe("outgoing: the instrument is the subject; incoming: the object."),
        limit: z
          .number()
          .int()
          .min(1)
          .max(LIMITS.graph.max)
          .default(LIMITS.graph.default)
          .describe(`Most items per list (1–${LIMITS.graph.max}).`),
      }),
      output: GraphOutV1,
    },
    async ({ id, relationshipType, direction, limit }) => {
      requireInstrument(id, "get_instrument_graph");
      const g = await instrumentGraph(id);
      const outgoing = (e: (typeof g.edges)[number]) => e.subject.id === g.root.id;
      const counts = {
        outgoing: {} as Record<string, number>,
        incoming: {} as Record<string, number>,
      };
      for (const e of g.edges) {
        const side = outgoing(e) ? counts.outgoing : counts.incoming;
        side[e.relationshipType] = (side[e.relationshipType] ?? 0) + 1;
      }
      const edges = g.edges.filter(
        (e) =>
          (relationshipType === undefined || e.relationshipType === relationshipType) &&
          (direction === "both" || outgoing(e) === (direction === "outgoing")),
      );
      const [deployments, markets] = [g.deployments ?? [], g.markets ?? []];
      return {
        root: g.root,
        edges: edges.slice(0, limit),
        edgeCounts: counts,
        listings: g.listings.slice(0, limit),
        deployments: deployments.slice(0, limit),
        markets: markets.slice(0, limit),
        totals: {
          edges: edges.length,
          listings: g.listings.length,
          deployments: deployments.length,
          markets: markets.length,
        },
        truncated: [edges, g.listings, deployments, markets].some((l) => l.length > limit),
      };
    },
  );

  // 6. quote --------------------------------------------------------------------
  tool(
    "get_quote",
    {
      title: "Canonical quote",
      description:
        "Undrly's canonical quote for one market (GET /v1/quote): price with its unit (currency or asset, never implied), price type (last, mid, mark, reference, average), bid/ask, basis and venue, asOf/receivedAt, freshness, and the aggregation method. If the query names an instrument priced in several units the result is ambiguous and lists the units; pass one as unit.",
      input: z.strictObject({ query: Query, unit: UnitId }),
      output: QuoteOutV1,
    },
    async ({ query, unit }) => {
      checkQuery(query);
      try {
        return {
          quote: await get(v1.QuoteV1, `/v1/quote?q=${enc(query)}${unitParam(unit)}`, query),
        };
      } catch (e) {
        if (!(e instanceof ToolFailure) || e.code !== "ambiguous") throw e;
        // One instrument in several units: name the units (from its graph).
        const r = await get(v1.ResolveResultV1, `/v1/resolve?q=${enc(query)}`, query);
        if (r.match?.kind !== "node" || r.match.node.kind !== "instrument") throw e;
        const units = ((await instrumentGraph(r.match.node.id)).markets ?? []).map((m) => m.unit);
        throw new ToolFailure("ambiguous", `${e.message}; pass one of these units`, {
          ...e.details,
          units,
        });
      }
    },
  );

  // 7. markets ------------------------------------------------------------------
  tool(
    "get_markets",
    {
      title: "Markets, listings and feeds",
      description:
        "Where an instrument trades and who reports it, keeping four things apart: listings (the instrument on a venue, with the venue's symbols), markets (the instrument priced in one unit), each market's status and 24h statistics when it has a quote, and its feeds (one source's latest observation per venue and price type, with the stored source record it came from).",
      input: z.strictObject({ id: InstrumentId }),
      output: MarketsV1,
    },
    async ({ id }) => {
      requireInstrument(id, "get_markets");
      const g = await instrumentGraph(id);
      const all = g.markets ?? [];
      const markets = [];
      for (const m of all.slice(0, LIMITS.markets)) {
        const u = `?unit=${enc(m.unit.id)}`;
        const obs = await probe(v1.ObservationsV1, `/v1/quotes/${enc(id)}${u}`);
        const ctx = await probe(v1.MarketV1, `/v1/market/${enc(id)}${u}`);
        markets.push({
          unit: m.unit,
          venues: m.venues,
          status: ctx?.marketStatus ?? null,
          statistics: ctx?.statistics ?? null,
          feeds: (obs?.observations ?? []).map((o) => ({
            source: o.source,
            venue: o.venue,
            priceType: o.priceType,
            basis: o.basis,
            price: o.price,
            bid: o.bid,
            ask: o.ask,
            observedAt: o.observedAt,
            receivedAt: o.receivedAt,
            freshness: o.freshness,
            sourceRecord: o.sourceRecord,
          })),
        });
      }
      return { instrument: g.root, listings: g.listings, markets, marketsTotal: all.length };
    },
  );

  // 8. history ------------------------------------------------------------------
  tool(
    "get_history",
    {
      title: "Price history",
      description: `Bounded price history for one market, oldest first. series "candles": a venue's OHLCV bars at interval 1h, 4h (derived from 1h) or 1d (GET /v1/candles). series "reference": a published reference or average series such as central-bank rates (GET /v1/history). Every response states its unit and price type. limit 1–${LIMITS.history.max}; start/end are RFC 3339 UTC timestamps.`,
      input: z.strictObject({
        query: Query,
        series: z.enum(["candles", "reference"]).default("candles"),
        interval: z
          .enum(v1.CANDLE_INTERVALS)
          .default("1d")
          .describe("Candle interval (series candles only)."),
        limit: z
          .number()
          .int()
          .min(1)
          .max(LIMITS.history.max)
          .default(LIMITS.history.default)
          .describe(`Most recent points to return (1–${LIMITS.history.max}).`),
        start: v1.TimestampString.optional().describe("Inclusive lower bound (…Z)."),
        end: v1.TimestampString.optional().describe("Exclusive upper bound (…Z)."),
        unit: UnitId,
      }),
      output: HistoryOutV1,
    },
    async ({ query, series, interval, limit, start, end, unit }) => {
      checkQuery(query);
      const bounds = `limit=${limit}${start ? `&start=${enc(start)}` : ""}${end ? `&end=${enc(end)}` : ""}${unitParam(unit)}`;
      if (series === "candles") {
        const path = `/v1/candles/${enc(query)}?interval=${interval}&${bounds}`;
        return { series, candles: await get(v1.CandlesV1, path, query) };
      }
      return {
        series,
        history: await get(v1.HistoryV1, `/v1/history/${enc(query)}?${bounds}`, query),
      };
    },
  );

  // 9. derivatives --------------------------------------------------------------
  tool(
    "get_derivatives",
    {
      title: "Derivative context",
      description:
        "A perpetual's venue context (GET /v1/derivatives): mark, index (oracle), mid, funding rate and interval, open interest and 24h volume, all prices in derivatives.unit. contract lists the stored contract terms with provenance: DERIVES_FROM (underlying), DENOMINATED_IN (price unit), MARGINED_IN, SETTLES_IN and TRADES_ON. Price denomination, margin and settlement are separate and may differ.",
      input: z.strictObject({ query: Query, unit: UnitId }),
      output: DerivativesOutV1,
    },
    async ({ query, unit }) => {
      checkQuery(query);
      const derivatives = await get(
        v1.DerivativesV1,
        `/v1/derivatives/${enc(query)}?${unitParam(unit).slice(1)}`,
        query,
      );
      const { candidate } = await nodeOf(derivatives.subject.id);
      const terms = (type: string) =>
        candidate.relationships
          .filter((r) => r.relationshipType === type && !r.projected)
          .map((r) => ({ object: r.object, provenance: r.provenance }));
      return {
        derivatives,
        contract: {
          derivesFrom: terms("DERIVES_FROM"),
          denominatedIn: terms("DENOMINATED_IN"),
          marginedIn: terms("MARGINED_IN"),
          settlesIn: terms("SETTLES_IN"),
          tradesOn: terms("TRADES_ON"),
        },
      };
    },
  );

  // Resource: the vocabulary -----------------------------------------------------
  server.registerResource(
    "vocabulary",
    "undrly://vocabulary",
    {
      title: "Undrly vocabulary",
      description:
        "Node categories, instrument classes, relationship types (with allowed endpoints), query and identifier forms, price types, units and the semantic invariants Undrly keeps.",
      mimeType: "application/json",
      cacheHint: { ttlMs: 3_600_000, cacheScope: "public" },
    },
    async (uri) => ({
      contents: [
        { uri: uri.href, mimeType: "application/json", text: JSON.stringify(VOCABULARY, null, 2) },
      ],
    }),
  );

  return server;
}
