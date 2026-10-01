/**
 * Read-only HTTP API (docs/hackathon-v1.md §7).
 *
 * GET /                         service name, endpoints, examples
 * GET /v1/search?q=              ranked candidates (discovery only)
 * GET /v1/resolve?q=             resolved | ambiguous | not_found
 * GET /v1/quote/:query  (?q=)    one canonical QuoteV1
 * GET /v1/quotes/:query (?q=)    the per-feed observations behind it
 * GET /v1/explain?q=             why resolve concluded what it did (V1.4)
 * GET /v1/instruments/:id/graph  one hop of edges + listings (+ deployments, markets)
 * GET /v1/universes              universes with a snapshot (V1.1)
 * GET /v1/universes/:key         one universe's latest membership
 * GET /v1/candles/:query         venue candles (?interval=1h|4h|1d&limit=&start=&end=) (V1.3)
 * GET /v1/history/:query         a reference series' published values, or a derived
 *                                cross's closes (?limit=&start=&end=&interval=&series=)
 * GET /v1/market/:query          the quote in market context: status and statistics
 * GET /v1/markets                every quoted market, paged (?class=a,b&q=&limit=&offset=)
 * GET /v1/markets/query          the shortest query for one market (?id=&unit=)
 * GET /v1/derivatives/:query     a perpetual's mark, index, funding and open interest
 * GET /v1/calendar/:query        a session market's trading calendar (?from=&to= dates)
 * GET /v1/economic-calendar      scheduled US economic releases (?from=&to=&category=)
 *
 * Queries may contain `/` (`EUR/USD`); path forms accept it unencoded. The
 * quote, quotes, candles, history, market, derivatives and calendar routes
 * also take `?unit=<currency or instrument id>` (V1.8) to select one market.
 */
import { v1 } from "@undrly/contracts";
import { Hono } from "hono";
import {
  addDays,
  type Bounds,
  calendar,
  candles,
  crossLegs,
  derivatives,
  economicCalendar,
  history,
  market,
  markets,
  newYorkToday,
} from "./market-data.ts";
import {
  canonicalQuote,
  explain,
  feedObservations,
  graph,
  pricedPairs,
  resolveQuery,
  resolveResult,
  type Sql,
  search,
  shortestQuery,
  universe,
  universes,
} from "./service.ts";

type ErrorCode = v1.ErrorV1["error"]["code"];
const STATUS: Record<ErrorCode, 400 | 404 | 409> = {
  bad_request: 400,
  not_found: 404,
  no_quote: 404,
  no_data: 404,
  ambiguous: 409,
};

/** `GET /`: discovery only; no capabilities beyond the routes below. */
const SERVICE_INDEX = v1.ServiceIndexV1.parse({
  schemaVersion: 1,
  name: "Undrly",
  description: "One normalized API across every market.",
  endpoints: [
    { path: "/v1/quote/{query}", returns: "one canonical quote for a market" },
    { path: "/v1/quotes/{query}", returns: "the per-source observations behind it" },
    { path: "/v1/search?q=", returns: "matching instruments, currencies and venues" },
    { path: "/v1/resolve?q=", returns: "what a query refers to" },
    { path: "/v1/explain?q=", returns: "why a query resolves the way it does" },
    { path: "/v1/instruments/{id}/graph", returns: "an instrument's direct relationships" },
    {
      path: "/v1/universes",
      returns: "the imported universes (crypto, S&P 500, Nasdaq-100, perps, FX)",
    },
    { path: "/v1/universes/{key}", returns: "a universe's latest membership" },
    {
      path: "/v1/candles/{query}?interval=1h|4h|1d&limit=",
      returns: "a market's venue candles (OHLCV)",
    },
    { path: "/v1/history/{query}?limit=", returns: "a reference series' published values" },
    { path: "/v1/market/{query}", returns: "the quote with market status and statistics" },
    {
      path: "/v1/markets?class=&q=&limit=&offset=",
      returns: "every quoted market, paged, with status, statistics and a 24h sparkline",
    },
    {
      path: "/v1/derivatives/{query}",
      returns: "a perpetual's mark, index, funding, open interest",
    },
    {
      path: "/v1/calendar/{query}?from=&to=",
      returns: "a stock's trading days, holidays, corporate actions and earnings",
    },
    {
      path: "/v1/economic-calendar?from=&to=&category=",
      returns: "scheduled US economic releases (CPI, jobs, GDP, …)",
    },
  ],
  examples: [
    "/v1/quote/NVDA",
    "/v1/quote/BTC/USD",
    "/v1/quote/EUR/USD",
    "/v1/quote/USD/IDR",
    "/v1/quote/XAU/USD",
    "/v1/quote/BTC-PERP",
    "/v1/quotes/BTC/USD",
    "/v1/search?q=gold",
    "/v1/explain?q=BTC-PERP",
    "/v1/universes/sp500",
    "/v1/universes/fx-southeast-asia",
    "/v1/candles/BTC/USD?interval=1h&limit=5",
    "/v1/market/AAPL",
    "/v1/derivatives/BTC-PERP",
  ],
  dataUse:
    "Local/private demo only. Upstream redistribution terms are unreviewed; no production redistribution rights are claimed.",
});

export type AppOptions = {
  staleAfterSeconds: number;
  now?: () => Date;
};

export function createApp(sql: Sql, options: AppOptions) {
  const app = new Hono();
  const now = options.now ?? (() => new Date());

  const fail = (code: ErrorCode, message: string, candidates?: v1.ResolutionV1[]) => {
    const body = v1.ErrorV1.parse({
      schemaVersion: 1,
      error: candidates === undefined ? { code, message } : { code, message, candidates },
    });
    return Response.json(body, { status: STATUS[code] });
  };

  const queryOf = (param: string | undefined, q: string | undefined) => (param ?? q ?? "").trim();

  app.get("/health", (c) => c.json({ status: "ok" }));

  app.get("/", (c) => c.json(SERVICE_INDEX));

  app.get("/v1/search", async (c) => {
    const q = (c.req.query("q") ?? "").trim();
    if (q === "") return fail("bad_request", "missing query parameter q");
    return c.json(await search(sql, q));
  });

  const resolveRoute = async (query: string) => {
    const r = await resolveQuery(sql, query);
    if (r === null) return fail("bad_request", "invalid query");
    return Response.json(resolveResult(query, r));
  };
  app.get("/v1/resolve", (c) => resolveRoute((c.req.query("q") ?? "").trim()));
  app.get("/v1/resolve/:query{.+}", (c) => resolveRoute(queryOf(c.req.param("query"), undefined)));

  const explainRoute = async (query: string) => {
    if (query === "") return fail("bad_request", "missing query parameter q");
    const body = await explain(sql, query);
    return body === null ? fail("bad_request", "invalid query") : Response.json(body);
  };
  app.get("/v1/explain", (c) => explainRoute((c.req.query("q") ?? "").trim()));
  app.get("/v1/explain/:query{.+}", (c) => explainRoute(queryOf(c.req.param("query"), undefined)));

  /**
   * Resolution → exactly one priced pair, or an error response. `unit` (V1.8,
   * optional) is the canonical id of a currency or instrument: only pairs
   * priced in exactly that unit are kept, so one market (subject, unit) is
   * addressable by ids alone. It filters; it never widens resolution.
   */
  const onePair = async (query: string, unit?: string) => {
    let unitUuid: string | null = null;
    if (unit !== undefined) {
      const category = v1.canonicalIdCategory(unit);
      unitUuid =
        category === "currency" || category === "instrument" ? v1.canonicalIdUuid(unit) : null;
      if (unitUuid === null) return fail("bad_request", "unit must be a currency or instrument id");
    }
    const r = await resolveQuery(sql, query);
    if (r === null) return fail("bad_request", "invalid query");
    if (r.status === "not_found") return fail("not_found", `nothing resolves to ${query}`);
    const pairs = (await pricedPairs(sql, r)).filter(
      (p) => unitUuid === null || p.unit === unitUuid,
    );
    if (pairs.length === 1 && pairs[0]) return pairs[0];
    if (pairs.length === 0 && r.status === "resolved")
      return fail("no_quote", `no quote for ${query}`);
    return fail("ambiguous", `${query} is ambiguous; use a pair or an id`, r.resolutions);
  };

  const quoteRoute = async (query: string, unit: string | undefined) => {
    const pair = await onePair(query, unit);
    if (pair instanceof Response) return pair;
    const result = await canonicalQuote(sql, pair, now(), options.staleAfterSeconds);
    if (result.kind === "none") return fail("no_quote", `no quote for ${query}`);
    if (result.kind === "stale") {
      return fail(
        "no_quote",
        `no fresh canonical quote for ${query}: the aggregate is as of ${result.asOf}`,
      );
    }
    return Response.json(result.quote);
  };
  app.get("/v1/quote", (c) => quoteRoute((c.req.query("q") ?? "").trim(), c.req.query("unit")));
  app.get("/v1/quote/:query{.+}", (c) =>
    quoteRoute(queryOf(c.req.param("query"), undefined), c.req.query("unit")),
  );

  const quotesRoute = async (query: string, unit: string | undefined) => {
    const pair = await onePair(query, unit);
    if (pair instanceof Response) return pair;
    const body = v1.ObservationsV1.parse({
      schemaVersion: 1,
      query,
      observations: await feedObservations(sql, pair, now(), options.staleAfterSeconds),
    });
    return Response.json(body);
  };
  app.get("/v1/quotes", (c) => quotesRoute((c.req.query("q") ?? "").trim(), c.req.query("unit")));
  app.get("/v1/quotes/:query{.+}", (c) =>
    quotesRoute(queryOf(c.req.param("query"), undefined), c.req.query("unit")),
  );

  /** `limit`, `start`, `end` query parameters, or an error response. */
  const bounds = (q: (name: string) => string | undefined): Bounds | Response => {
    const limitText = q("limit");
    let limit = v1.DEFAULT_LIMIT;
    if (limitText !== undefined) {
      if (
        !/^[0-9]{1,4}$/.test(limitText) ||
        Number(limitText) < 1 ||
        Number(limitText) > v1.MAX_LIMIT
      ) {
        return fail("bad_request", `limit must be an integer from 1 to ${v1.MAX_LIMIT}`);
      }
      limit = Number(limitText);
    }
    const time = (name: string) => {
      const t = q(name);
      if (t === undefined) return null;
      return v1.TimestampString.safeParse(t).success ? t : undefined;
    };
    const [start, end] = [time("start"), time("end")];
    if (start === undefined || end === undefined) {
      return fail("bad_request", "start and end are RFC 3339 UTC timestamps (…Z)");
    }
    if (start !== null && end !== null && v1.timestampMicros(start) >= v1.timestampMicros(end)) {
      return fail("bad_request", "start must be before end");
    }
    return { limit, start, end };
  };

  app.get("/v1/candles/:query{.+}", async (c) => {
    const interval = c.req.query("interval");
    if (!(v1.CANDLE_INTERVALS as readonly string[]).includes(interval ?? "")) {
      return fail("bad_request", `interval must be one of ${v1.CANDLE_INTERVALS.join(", ")}`);
    }
    const b = bounds((n) => c.req.query(n));
    if (b instanceof Response) return b;
    const query = queryOf(c.req.param("query"), undefined);
    const pair = await onePair(query, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    if ((await crossLegs(sql, pair)) !== null) {
      return fail(
        "no_data",
        `${query}: a derived cross has no venue bars; its closes are at /v1/history?interval=1h|1d`,
      );
    }
    const r = await candles(sql, pair, interval as v1.CandleInterval, b);
    return "kind" in r ? fail("no_data", `${query}: ${r.message}`) : Response.json(r);
  });

  app.get("/v1/history/:query{.+}", async (c) => {
    const b = bounds((n) => c.req.query(n));
    if (b instanceof Response) return b;
    const series = c.req.query("series") ?? "default";
    if (series !== "default" && series !== "reference") {
      return fail("bad_request", "series is `reference` (or omitted)");
    }
    const interval = c.req.query("interval") ?? "1d";
    if (interval !== "1h" && interval !== "1d") {
      return fail("bad_request", "interval is 1h or 1d");
    }
    const query = queryOf(c.req.param("query"), undefined);
    const pair = await onePair(query, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    const r = await history(sql, pair, b, { series, interval });
    return "kind" in r ? fail("no_data", `${query}: ${r.message}`) : Response.json(r);
  });

  app.get("/v1/market/:query{.+}", async (c) => {
    const query = queryOf(c.req.param("query"), undefined);
    const pair = await onePair(query, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    const at = now();
    const q = await canonicalQuote(sql, pair, at, options.staleAfterSeconds);
    if (q.kind !== "quote") return fail("no_quote", `no quote for ${query}`);
    return Response.json(await market(sql, pair, q.quote, at));
  });

  app.get("/v1/markets/query", async (c) => {
    const id = (c.req.query("id") ?? "").trim();
    if (id === "") return fail("bad_request", "missing query parameter id");
    const pair = await onePair(id, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    return Response.json(await shortestQuery(sql, pair));
  });

  app.get("/v1/markets", async (c) => {
    const classes = (c.req.query("class") ?? "").split(",").filter((x) => x !== "");
    if (!classes.every((x) => (v1.INSTRUMENT_CLASSES as readonly string[]).includes(x))) {
      return fail(
        "bad_request",
        `class is a comma-separated list of ${v1.INSTRUMENT_CLASSES.join(", ")}`,
      );
    }
    const q = (c.req.query("q") ?? "").trim();
    if (q.length > 64) return fail("bad_request", "q is at most 64 characters");
    const int = (name: string, fallback: number, min: number, max: number) => {
      const text = c.req.query(name);
      if (text === undefined) return fallback;
      return /^[0-9]{1,6}$/.test(text) && Number(text) >= min && Number(text) <= max
        ? Number(text)
        : null;
    };
    const limit = int("limit", v1.MARKETS_DEFAULT_LIMIT, 1, v1.MARKETS_MAX_LIMIT);
    if (limit === null) {
      return fail("bad_request", `limit must be an integer from 1 to ${v1.MARKETS_MAX_LIMIT}`);
    }
    const offset = int("offset", 0, 0, 999_999);
    if (offset === null) return fail("bad_request", "offset must be a non-negative integer");
    const body = await markets(
      sql,
      [...new Set(classes)] as v1.InstrumentClass[],
      q === "" ? null : q,
      limit,
      offset,
      now(),
      options.staleAfterSeconds,
    );
    return Response.json(body);
  });

  app.get("/v1/derivatives/:query{.+}", async (c) => {
    const query = queryOf(c.req.param("query"), undefined);
    const pair = await onePair(query, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    const r = await derivatives(sql, pair, now(), options.staleAfterSeconds);
    return "kind" in r ? fail("no_data", `${query}: ${r.message}`) : Response.json(r);
  });

  /** `from` / `to` date parameters (default: today in New York + `days`), or an error. */
  const dateWindow = (
    q: (name: string) => string | undefined,
    days: number,
    at: Date,
  ): { from: string; to: string } | Response => {
    const [fromText, toText] = [q("from"), q("to")];
    const valid = (d: string | undefined) =>
      d === undefined ||
      (v1.DateString.safeParse(d).success &&
        Number.isFinite(Date.parse(`${d}T00:00:00Z`)) &&
        addDays(d, 0) === d);
    if (!valid(fromText) || !valid(toText)) {
      return fail("bad_request", "from and to are calendar dates (YYYY-MM-DD)");
    }
    const from = fromText ?? newYorkToday(at);
    const to = toText ?? addDays(from, days);
    if (from > to) return fail("bad_request", "from must be on or before to");
    if (to > addDays(from, v1.MAX_CALENDAR_DAYS - 1)) {
      return fail("bad_request", `at most ${v1.MAX_CALENDAR_DAYS} days per request`);
    }
    return { from, to };
  };

  app.get("/v1/economic-calendar", async (c) => {
    const at = now();
    const w = dateWindow((n) => c.req.query(n), 30, at);
    if (w instanceof Response) return w;
    const category = c.req.query("category") ?? null;
    if (category !== null && !(v1.ECONOMIC_CATEGORIES as readonly string[]).includes(category)) {
      return fail("bad_request", `category must be one of ${v1.ECONOMIC_CATEGORIES.join(", ")}`);
    }
    return c.json(await economicCalendar(sql, w.from, w.to, category));
  });

  app.get("/v1/calendar/:query{.+}", async (c) => {
    const at = now();
    const w = dateWindow((n) => c.req.query(n), 14, at);
    if (w instanceof Response) return w;
    const { from, to } = w;
    const query = queryOf(c.req.param("query"), undefined);
    const pair = await onePair(query, c.req.query("unit"));
    if (pair instanceof Response) return pair;
    const r = await calendar(sql, pair, from, to, at);
    return "kind" in r ? fail("no_data", `${query}: ${r.message}`) : Response.json(r);
  });

  app.get("/v1/instruments/:id/graph", async (c) => {
    const id = c.req.param("id");
    const uuid = v1.canonicalIdCategory(id) === "instrument" ? v1.canonicalIdUuid(id) : null;
    if (uuid === null) return fail("bad_request", "expected an instrument id");
    const body = await graph(sql, uuid);
    return body === null ? fail("not_found", `no instrument ${id}`) : c.json(body);
  });

  app.get("/v1/universes", async (c) => c.json(await universes(sql)));

  app.get("/v1/universes/:key", async (c) => {
    const key = c.req.param("key");
    if (!(v1.UNIVERSE_KEYS as readonly string[]).includes(key)) {
      return fail("not_found", `no universe ${key}`);
    }
    const body = await universe(sql, key as v1.UniverseKey);
    return body === null ? fail("not_found", `universe ${key} has no snapshot`) : c.json(body);
  });

  app.notFound(() => fail("not_found", "no such route"));
  return app;
}
