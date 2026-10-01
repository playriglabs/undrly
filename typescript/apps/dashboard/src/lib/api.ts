/**
 * Server functions over the Undrly API. They run on the server only, so the
 * API key (UNDRLY_API_KEY) never reaches the browser. Every body is parsed
 * with the published v1 contracts; a non-2xx answer becomes `ApiError`.
 */
import { createServerFn } from "@tanstack/react-start";
import { v1 } from "@undrly/contracts";
import type { z } from "zod";
import { authMiddleware } from "../server/session";

export type ApiError = { ok: false; status: number; code: string; message: string };
export type ApiResult<T> = { ok: true; data: T } | ApiError;

async function get<S extends z.ZodType>(path: string, schema: S): Promise<ApiResult<z.output<S>>> {
  // Read per request (never at module scope): server-only and edge-safe.
  const base = process.env.UNDRLY_API_URL ?? "http://127.0.0.1:8787";
  const key = process.env.UNDRLY_API_KEY;
  let res: Response;
  try {
    res = await fetch(new URL(path, base), {
      headers: key ? { Authorization: `Bearer ${key}` } : {},
    });
  } catch {
    return { ok: false, status: 0, code: "unreachable", message: `Can't reach the API at ${base}` };
  }
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const error = v1.ErrorV1.safeParse(body);
    return error.success
      ? { ok: false, status: res.status, ...error.data.error }
      : { ok: false, status: res.status, code: "http", message: `HTTP ${res.status}` };
  }
  const parsed = schema.safeParse(body);
  return parsed.success
    ? { ok: true, data: parsed.data }
    : { ok: false, status: res.status, code: "contract", message: "Unexpected response shape" };
}

export const PAGE_SIZE = 50;

export type MarketsFilter = { classes: v1.InstrumentClass[]; q: string; page: number };

export const getMarkets = createServerFn({ method: "GET" })
  .middleware([authMiddleware])
  .validator((input: MarketsFilter) => input)
  .handler(async ({ data }) => {
    const params = new URLSearchParams({
      limit: String(PAGE_SIZE),
      offset: String((data.page - 1) * PAGE_SIZE),
    });
    if (data.classes.length > 0) params.set("class", data.classes.join(","));
    if (data.q.trim()) params.set("q", data.q.trim());
    return get(`/v1/markets?${params}`, v1.MarketsV1);
  });

/** Market counts per class for the sidebar: one row is enough, `counts` ignores filters. */
export const getMarketCounts = createServerFn({ method: "GET" })
  .middleware([authMiddleware])
  .handler(async () => {
    const r = await get("/v1/markets?limit=1", v1.MarketsV1);
    return r.ok ? { ok: true as const, data: r.data.counts } : r;
  });

/** One market by ids: `/v1/market/{subject}?unit=`. */
const marketPath = (route: string, id: string, unit: string | undefined, extra = "") => {
  const params = new URLSearchParams(extra);
  if (unit) params.set("unit", unit);
  const query = params.toString();
  return `/v1/${route}/${encodeURIComponent(id)}${query ? `?${query}` : ""}`;
};

export const getMarketDetail = createServerFn({ method: "GET" })
  .middleware([authMiddleware])
  .validator((input: { id: string; unit?: string }) => input)
  .handler(async ({ data }) => {
    const queryParams = new URLSearchParams({ id: data.id });
    if (data.unit) queryParams.set("unit", data.unit);
    const [market, quote, explain, query] = await Promise.all([
      get(marketPath("market", data.id, data.unit), v1.MarketV1),
      get(marketPath("quote", data.id, data.unit), v1.QuoteV1),
      get(`/v1/explain?${new URLSearchParams({ q: data.id })}`, v1.ExplainV1),
      // The shortest query for this market, for the copyable request.
      get(`/v1/markets/query?${queryParams}`, v1.MarketQueryV1),
    ]);
    const derivatives =
      market.ok &&
      market.data.subject.kind === "instrument" &&
      market.data.subject.class === "perpetual_future"
        ? await get(marketPath("derivatives", data.id, data.unit), v1.DerivativesV1)
        : null;
    return { market, quote, explain, derivatives, query };
  });

/**
 * Chart ranges: what each one asks of the candle intervals the API serves (1h, 4h, 1d).
 * The short ranges load twice their span, so the chart opens with more bars to read.
 */
export const RANGES = {
  "24H": { interval: "1h", limit: 48 },
  "1W": { interval: "4h", limit: 84 },
  "1M": { interval: "1d", limit: 60 },
  "3M": { interval: "1d", limit: 90 },
  "1Y": { interval: "1d", limit: 365 },
} as const satisfies Record<string, { interval: v1.CandleInterval; limit: number }>;
export type Range = keyof typeof RANGES;

/** One bar at its open time; a published reference value is a bar with only `close`. */
export type SeriesBar = {
  time: string;
  open: string | null;
  high: string | null;
  low: string | null;
  close: string;
};
/** `cross`: a derived pair's closes (its legs' ratio); `history`: published reference values. */
export type Series = { kind: "candles" | "history" | "cross"; bars: SeriesBar[] };

/** A market's price series for a range: venue candles (OHLC), else its published reference values. */
export const getSeries = createServerFn({ method: "GET" })
  .middleware([authMiddleware])
  .validator((input: { id: string; unit?: string; range: Range }) => input)
  .handler(async ({ data }): Promise<ApiResult<Series>> => {
    const { interval, limit } = RANGES[data.range];
    const candles = await get(
      marketPath("candles", data.id, data.unit, `interval=${interval}&limit=${limit}`),
      v1.CandlesV1,
    );
    if (candles.ok) {
      const bars = candles.data.candles.map((c) => ({
        time: c.openTime,
        open: c.open,
        high: c.high,
        low: c.low,
        close: c.close,
      }));
      return { ok: true, data: { kind: "candles", bars } };
    }
    if (candles.code !== "no_data") return candles;
    // No venue bars: a derived cross has its legs' closes, a reference series its
    // published values; a traded market has none yet. History has 1h and 1d only.
    const historyInterval = interval === "1d" ? "1d" : "1h";
    const historyLimit = interval === "4h" ? limit * 4 : limit;
    const history = await get(
      marketPath(
        "history",
        data.id,
        data.unit,
        `interval=${historyInterval}&limit=${historyLimit}`,
      ),
      v1.HistoryV1,
    );
    if (!history.ok) {
      return history.code === "no_data"
        ? { ok: false, status: 404, code: "no_data", message: "No price history stored yet" }
        : history;
    }
    const bars = history.data.observations.map((o) => ({
      time: o.asOf,
      open: null,
      high: null,
      low: null,
      close: o.price,
    }));
    const kind = history.data.basis === "derived" ? "cross" : "history";
    return { ok: true, data: { kind, bars } };
  });
