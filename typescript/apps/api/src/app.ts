/**
 * Read-only HTTP API (docs/hackathon-v1.md §7).
 *
 * GET /v1/search?q=              ranked candidates (discovery only)
 * GET /v1/resolve?q=             resolved | ambiguous | not_found
 * GET /v1/quote/:query  (?q=)    one canonical QuoteV1
 * GET /v1/quotes/:query (?q=)    the per-feed observations behind it
 * GET /v1/instruments/:id/graph  one hop of edges + listings
 *
 * Queries may contain `/` (`EUR/USD`); path forms accept it unencoded.
 */
import { v1 } from "@undrly/contracts";
import { Hono } from "hono";
import {
  canonicalQuote,
  feedObservations,
  graph,
  pricedPairs,
  resolveQuery,
  resolveResult,
  type Sql,
  search,
} from "./service.ts";

type ErrorCode = v1.ErrorV1["error"]["code"];
const STATUS: Record<ErrorCode, 400 | 404 | 409> = {
  bad_request: 400,
  not_found: 404,
  no_quote: 404,
  ambiguous: 409,
};

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

  /** Resolution → exactly one priced pair, or an error response. */
  const onePair = async (query: string) => {
    const r = await resolveQuery(sql, query);
    if (r === null) return fail("bad_request", "invalid query");
    if (r.status === "not_found") return fail("not_found", `nothing resolves to ${query}`);
    const pairs = await pricedPairs(sql, r);
    if (pairs.length === 1 && pairs[0]) return pairs[0];
    if (pairs.length === 0 && r.status === "resolved")
      return fail("no_quote", `no quote for ${query}`);
    return fail("ambiguous", `${query} is ambiguous; use a pair or an id`, r.resolutions);
  };

  const quoteRoute = async (query: string) => {
    const pair = await onePair(query);
    if (pair instanceof Response) return pair;
    const quote = await canonicalQuote(sql, pair, now(), options.staleAfterSeconds);
    if (quote === null) return fail("no_quote", `no quote for ${query}`);
    return Response.json(quote);
  };
  app.get("/v1/quote", (c) => quoteRoute((c.req.query("q") ?? "").trim()));
  app.get("/v1/quote/:query{.+}", (c) => quoteRoute(queryOf(c.req.param("query"), undefined)));

  const quotesRoute = async (query: string) => {
    const pair = await onePair(query);
    if (pair instanceof Response) return pair;
    const body = v1.ObservationsV1.parse({
      schemaVersion: 1,
      query,
      observations: await feedObservations(sql, pair),
    });
    return Response.json(body);
  };
  app.get("/v1/quotes", (c) => quotesRoute((c.req.query("q") ?? "").trim()));
  app.get("/v1/quotes/:query{.+}", (c) => quotesRoute(queryOf(c.req.param("query"), undefined)));

  app.get("/v1/instruments/:id/graph", async (c) => {
    const id = c.req.param("id");
    const uuid = v1.canonicalIdCategory(id) === "instrument" ? v1.canonicalIdUuid(id) : null;
    if (uuid === null) return fail("bad_request", "expected an instrument id");
    const body = await graph(sql, uuid);
    return body === null ? fail("not_found", `no instrument ${id}`) : c.json(body);
  });

  app.notFound(() => fail("not_found", "no such route"));
  return app;
}
