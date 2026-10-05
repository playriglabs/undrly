/**
 * V1.3 market data surface (docs/v1.3-market-data.md): candles, reference
 * series history, market context and perpetual derivatives data. Same
 * philosophy as quotes: exact decimal strings, explicit semantics, `null`
 * where a source states nothing, provider identity never named.
 */
import { z } from "zod";
import { changeOf, decimalCompare } from "./decimal.ts";
import { PriceUnitV1 } from "./market-observation.ts";
import { DecimalString, TimestampString, timestampMicros } from "./primitives.ts";
import { INSTRUMENT_CLASSES, PRICE_TYPES, PriceSubjectV1, VenueRefV1 } from "./quote.ts";

/**
 * Candle intervals served. `1h` and `1d` are the venue's own bars; `4h` is
 * derived from four consecutive `1h` bars (aligned to 00, 04, … UTC).
 * Mirrors `undrly_core::BarInterval` plus the derived `4h`.
 */
export const CANDLE_INTERVALS = ["1h", "4h", "1d"] as const;
export type CandleInterval = (typeof CANDLE_INTERVALS)[number];

/** Largest `limit` a candle or history request may ask for. */
export const MAX_LIMIT = 1000;
export const DEFAULT_LIMIT = 100;

type Issue = { message: string; path?: (string | number)[] };

const le = (a: string, b: string) => decimalCompare(a, b) <= 0;

/** One OHLC bar of trade prices at one venue. */
export const CandleV1 = z
  .strictObject({
    /** The bar's start (inclusive). */
    openTime: TimestampString,
    /** The bar's end (exclusive): `openTime` + interval (a New York day for equities' `1d`). */
    closeTime: TimestampString,
    open: DecimalString,
    high: DecimalString,
    low: DecimalString,
    close: DecimalString,
    /** Quantity of the subject traded at the venue in the bar; `null` when the source states none. */
    volume: DecimalString.nullable(),
    /** `false` when the bar was still in progress when last collected. */
    complete: z.boolean(),
  })
  .refine(
    (c) =>
      le(c.low, c.high) &&
      le(c.low, c.open) &&
      le(c.open, c.high) &&
      le(c.low, c.close) &&
      le(c.close, c.high),
    { message: "low <= open, close <= high" },
  )
  .refine((c) => timestampMicros(c.closeTime) > timestampMicros(c.openTime), {
    message: "a candle closes after it opens",
  });
export type CandleV1 = z.infer<typeof CandleV1>;

/**
 * `GET /v1/candles/{query}?interval=&limit=&start=&end=`: a market's candles
 * from one venue (Kraken, Hyperliquid or IEX; not named), oldest first.
 * `derived` is `true` for `4h`: each candle is
 * four stored `1h` bars (open of the first, max high, min low, close of the
 * last, sum of volumes); a 4-hour period missing any hour is omitted, never
 * filled.
 */
export const CandlesV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    subject: PriceSubjectV1,
    unit: PriceUnitV1,
    interval: z.enum(CANDLE_INTERVALS),
    /** Candles are trade prices (`last`). */
    priceType: z.literal("last"),
    derived: z.boolean(),
    candles: z.array(CandleV1),
  })
  .superRefine((c, ctx) => {
    const issues: Issue[] = [];
    if (c.derived !== (c.interval === "4h")) {
      issues.push({ message: "only 4h candles are derived" });
    }
    for (let i = 1; i < c.candles.length; i++) {
      const [a, b] = [c.candles[i - 1], c.candles[i]];
      if (a && b && timestampMicros(b.openTime) < timestampMicros(a.closeTime)) {
        issues.push({ message: "candles are ascending and do not overlap", path: ["candles", i] });
      }
    }
    for (const issue of issues) ctx.addIssue({ code: "custom", ...issue });
  });
export type CandlesV1 = z.infer<typeof CandlesV1>;

/**
 * `GET /v1/history/{query}?limit=&start=&end=`: a reference or average
 * series (central-bank rates, commodity reference prices, monthly averages)
 * as published, oldest first: one value per publication, never OHLC. The
 * series is the one feed that produces the pair's current canonical quote.
 *
 * For a derived cross (V1.9, `cross-via-stablecoin-v1`) the series is the
 * cross of its legs' bar closes at the same bar times (`?interval=1h|1d`,
 * default `1d`): `priceType` `mid`, `basis` `derived`, each value
 * `crossRate(numerator close, denominator close)`, `asOf` the bars' close
 * time. A conversion (V1.10, `convert-via-stablecoin-v1`) is the same with
 * `convertRate` (the legs' product). `?series=reference` asks for the
 * pair's reference feed instead.
 */
export const HistoryV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    subject: PriceSubjectV1,
    unit: PriceUnitV1,
    priceType: z.enum(["reference", "average", "mid"]),
    basis: z.enum(["aggregated", "derived"]),
    observations: z.array(
      z.strictObject({
        /** The source's time for the value (a publication date at 00:00 UTC when only a date is stated). */
        asOf: TimestampString,
        price: DecimalString,
      }),
    ),
  })
  .refine(
    (h) =>
      h.observations.every(
        (o, i) =>
          i === 0 ||
          timestampMicros(o.asOf) > timestampMicros(h.observations[i - 1]?.asOf ?? o.asOf),
      ),
    { message: "observations are strictly ascending" },
  )
  .refine((h) => (h.basis === "derived") === (h.priceType === "mid"), {
    message: "a derived cross series is mid; a published series is reference or average",
  });
export type HistoryV1 = z.infer<typeof HistoryV1>;

/**
 * Whether the market a quote comes from is trading. `continuous`: a venue
 * that trades around the clock (crypto spot, perpetuals, Kraken/Bitstamp
 * FX books). `open` / `pre_market` / `after_hours` / `closed`: by the venue's
 * loaded trading calendar (equities). `unknown`: a session market whose
 * calendar is not loaded for today. `null`: not a traded market (reference
 * rates and prices, monthly averages).
 */
export const MARKET_STATUSES = [
  "continuous",
  "open",
  "pre_market",
  "after_hours",
  "closed",
  "unknown",
] as const;

/**
 * Statistics from the venue's own bars:
 * - `rolling_24h` (continuous markets): the 24 consecutive `1h` bars ending
 *   with the latest; `open` of the first, max `high`, min `low`, `close` of
 *   the latest, sum of `volume`; `change = close - open`. When hours have no
 *   bar (no trades: an onchain pool, a venue's session; V1.10), `open` is
 *   the last close at or before 24 hours before the latest bar opened, and
 *   `from` that bar's close time.
 * - `session` (equities): the venue's latest `1d` bar (its New York trading
 *   day) and the one before; `previousClose` is that earlier bar's close;
 *   `change = close - previousClose`.
 * - `rolling_24h_closes` (derived crosses, V1.9): the cross of the legs'
 *   hourly closes at the 25 consecutive hours ending with the latest; `open`
 *   is the close 24 hours before, `high`/`low` the max/min **of those
 *   closes** (not an intrabar range), `volume` `null`. With hours missing,
 *   from the last close at or before 24 hours earlier (V1.10).
 * - `rolling_24h` for a published series: the source's own open/high/low/
 *   close over the 24 hours before its poll (gold-api metals), `volume` `null`.
 * - Published series (reference rates and averages; `marketStatus` `null`),
 *   from the stored values of the feed behind the canonical quote:
 *   - `rolling_24h_observations`: a series polled within the day (metals):
 *     `open` is the last value at or before 24 hours ago, `close` the
 *     latest, `high`/`low` the max/min of the values in between;
 *   - `previous_publication`: a daily or monthly publication: `close` is the
 *     latest value, `previousClose` the one before (`from`/`to` are their
 *     dates), `high`/`low` the larger/smaller of the two.
 *   `volume` is `null`: nothing is traded.
 * `changePercent` is `change / (open or previousClose) × 100`, half to even
 * at 4 places. Missing bars make the statistics `null`, never estimated.
 * The bars are one venue's (Kraken, Hyperliquid or IEX; the same venue
 * `/v1/candles` names); the statistics do not name it.
 */
export const MarketStatisticsV1 = z
  .strictObject({
    window: z.enum([
      "rolling_24h",
      "session",
      "rolling_24h_closes",
      "rolling_24h_observations",
      "previous_publication",
    ]),
    from: TimestampString,
    to: TimestampString,
    open: DecimalString,
    high: DecimalString,
    low: DecimalString,
    close: DecimalString,
    previousClose: DecimalString.nullable(),
    change: DecimalString,
    changePercent: DecimalString,
    /** Quantity of the subject traded at the venue in the window; `null` when unknown. */
    volume: DecimalString.nullable(),
    /** `false` when the latest bar was still in progress when collected. */
    complete: z.boolean(),
  })
  .superRefine((s, ctx) => {
    const issues: Issue[] = [];
    const withPrevious = s.window === "session" || s.window === "previous_publication";
    if (withPrevious !== (s.previousClose !== null)) {
      issues.push({
        message: "a session or publication states its previous value; a rolling window does not",
      });
    }
    const base = s.previousClose ?? s.open;
    const c = changeOf(s.close, base);
    if (c === null || c.absolute !== s.change || c.percent !== s.changePercent) {
      issues.push({ message: "change is close - (previousClose or open), percent of that" });
    }
    if (!(le(s.low, s.high) && le(s.low, s.close) && le(s.close, s.high))) {
      issues.push({ message: "low <= close <= high" });
    }
    for (const issue of issues) ctx.addIssue({ code: "custom", ...issue });
  });
export type MarketStatisticsV1 = z.infer<typeof MarketStatisticsV1>;

/**
 * `GET /v1/market/{query}`: the canonical quote's price in market context.
 * `price` … `freshness` are exactly `/v1/quote`'s.
 */
export const MarketV1 = z.strictObject({
  schemaVersion: z.literal(1),
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  price: DecimalString,
  priceType: z.enum(PRICE_TYPES),
  basis: z.enum(["venue", "aggregated", "derived"]),
  asOf: TimestampString,
  freshness: z.enum(["fresh", "stale"]),
  marketStatus: z.enum(MARKET_STATUSES).nullable(),
  statistics: MarketStatisticsV1.nullable(),
});
export type MarketV1 = z.infer<typeof MarketV1>;

/**
 * `GET /v1/derivatives/{query}`: a perpetual's context as its venue reports
 * it. `unit` is the contract's **price denomination** (its `DENOMINATED_IN`;
 * for most Hyperliquid perpetuals Tether USD, for PURR and HYPE USD Coin).
 * It is not the margin asset (`MARGINED_IN`) or the asset cash flows are paid
 * in (`SETTLES_IN`); see the graph. Hyperliquid pays profit and loss and
 * funding in USDC without converting from the USDT denomination (a quanto
 * contract).
 * - `markPrice`, `indexPrice`, `midPrice`, `price24hAgo`: in `unit`.
 * - `indexPrice`: the venue's oracle price (Hyperliquid: a weighted median of
 *   CEX spot prices).
 * - `fundingRate`: a fraction per `fundingIntervalHours` (not annualized).
 *   A payment is `contracts × indexPrice × fundingRate`, paid in the
 *   settlement asset.
 * - `openInterest`, `volume24h`: in contracts (units of the subject; see
 *   `subject.contractMultiplier`). `volume24hNotional`: contracts × price, in
 *   `unit`. Both volumes are the venue's trailing 24 hours.
 * Only contexts normalized under the market's current unit are served.
 * The venue states no time: `asOf` is when Undrly received the context.
 */
export const DerivativesV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    subject: PriceSubjectV1,
    unit: PriceUnitV1,
    basis: z.literal("venue"),
    venue: VenueRefV1,
    markPrice: DecimalString,
    indexPrice: DecimalString.nullable(),
    midPrice: DecimalString.nullable(),
    fundingRate: DecimalString.nullable(),
    fundingIntervalHours: z.number().int().min(1).nullable(),
    openInterest: DecimalString.nullable(),
    volume24h: DecimalString.nullable(),
    volume24hNotional: DecimalString.nullable(),
    price24hAgo: DecimalString.nullable(),
    asOf: TimestampString,
    ageMs: z.number().int().min(0),
    freshness: z.enum(["fresh", "stale"]),
  })
  .refine((d) => d.subject.kind === "instrument" && d.subject.class === "perpetual_future", {
    message: "derivatives data is for perpetuals",
  })
  .refine((d) => (d.fundingRate === null) === (d.fundingIntervalHours === null), {
    message: "a funding rate states its interval",
  });
export type DerivativesV1 = z.infer<typeof DerivativesV1>;

/** A calendar date, `YYYY-MM-DD`. */
export const DateString = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "expected YYYY-MM-DD");

/** Largest span, in days, a calendar request may cover. */
export const MAX_CALENDAR_DAYS = 366;

/**
 * `GET /v1/calendar/{query}?from=&to=` (dates, inclusive; default: the next
 * 14 days): the trading sessions of the venue a session market's quote
 * comes from (equities: IEX, from Alpaca's US equity calendar), by local
 * trading date (`timezone`).
 * - `sessions`: each date with a session; `preOpen`/`postClose` bound the
 *   extended session, `open`/`close` the regular one. `earlyClose` is
 *   `true` when the regular session is shorter than 6 h 30 min.
 * - `closedWeekdays`: Monday–Friday dates in the window and in the loaded
 *   calendar with no session (holidays). Weekends are not listed.
 * - `coverage`: the dates the loaded calendar covers; outside it nothing is
 *   known (not listed as closed).
 * - `events`: corporate actions whose date (`exDate`, else `effectiveDate`,
 *   else `processDate`) falls in the window, as the source announced them.
 *   `role` is what this instrument is in the action (`acquiree`/`acquirer`
 *   in a merger, `spin_off_parent`). `cashAmount` is per share and the source
 *   states no currency; `stockRate` is shares per share held; `ratio` is
 *   old : new (splits: shares before : after; mergers: acquiree : acquirer;
 *   spin-offs: parent : new). `otherSymbol` is the other security as the
 *   source spells it (not an identifier).
 * - `earnings`: earnings reports dated in the window, as the earnings source
 *   states them: `time` (`before_open`, `after_close`, `during_hours`, or
 *   `null` when not stated), fiscal year and quarter, and the source's own
 *   EPS and revenue consensus estimates and actuals (no currency stated;
 *   `null` when not given). Estimates are one provider's consensus, not
 *   Undrly's.
 * Continuous markets and reference rates have no calendar (`no_data`).
 */
export const EarningsEventV1 = z.strictObject({
  date: DateString,
  time: z.enum(["before_open", "after_close", "during_hours"]).nullable(),
  fiscalYear: z.number().int(),
  fiscalQuarter: z.number().int().min(1).max(4),
  epsEstimate: DecimalString.nullable(),
  epsActual: DecimalString.nullable(),
  revenueEstimate: DecimalString.nullable(),
  revenueActual: DecimalString.nullable(),
});
export type EarningsEventV1 = z.infer<typeof EarningsEventV1>;

export const CORPORATE_ACTION_TYPES = [
  "cash_dividend",
  "stock_dividend",
  "forward_split",
  "reverse_split",
  "spin_off",
  "cash_merger",
  "stock_merger",
  "stock_and_cash_merger",
  "name_change",
] as const;

export const CorporateActionEventV1 = z.strictObject({
  type: z.enum(CORPORATE_ACTION_TYPES),
  role: z.enum(["subject", "acquiree", "acquirer", "spin_off_parent"]),
  date: DateString,
  exDate: DateString.nullable(),
  recordDate: DateString.nullable(),
  payableDate: DateString.nullable(),
  effectiveDate: DateString.nullable(),
  cashAmount: DecimalString.nullable(),
  stockRate: DecimalString.nullable(),
  ratio: z.strictObject({ old: DecimalString, new: DecimalString }).nullable(),
  special: z.boolean().nullable(),
  otherSymbol: z.string().min(1).nullable(),
});
export type CorporateActionEventV1 = z.infer<typeof CorporateActionEventV1>;

export const CalendarV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    subject: PriceSubjectV1,
    timezone: z.literal("America/New_York"),
    marketStatus: z.enum(["open", "pre_market", "after_hours", "closed", "unknown"]),
    from: DateString,
    to: DateString,
    coverage: z.strictObject({ from: DateString, to: DateString }),
    sessions: z.array(
      z.strictObject({
        date: DateString,
        preOpen: TimestampString,
        open: TimestampString,
        close: TimestampString,
        postClose: TimestampString,
        earlyClose: z.boolean(),
      }),
    ),
    closedWeekdays: z.array(DateString),
    events: z.array(CorporateActionEventV1),
    earnings: z.array(EarningsEventV1),
  })
  .refine((c) => c.from <= c.to, { message: "from is on or before to" })
  .refine(
    (c) =>
      c.sessions.every(
        (s) =>
          timestampMicros(s.preOpen) <= timestampMicros(s.open) &&
          timestampMicros(s.open) < timestampMicros(s.close) &&
          timestampMicros(s.close) <= timestampMicros(s.postClose),
      ),
    { message: "preOpen <= open < close <= postClose" },
  )
  .refine((c) => c.sessions.every((s) => !c.closedWeekdays.includes(s.date)), {
    message: "a date is either a session or closed",
  })
  .refine((c) => [...c.events, ...c.earnings].every((e) => c.from <= e.date && e.date <= c.to), {
    message: "events and earnings fall in the window",
  });
export type CalendarV1 = z.infer<typeof CalendarV1>;

/** Undrly's categories of economic releases. */
export const ECONOMIC_CATEGORIES = [
  "inflation",
  "labor",
  "growth",
  "consumption",
  "production",
  "housing",
  "sentiment",
  "trade",
] as const;

/**
 * `GET /v1/economic-calendar?from=&to=&category=` (dates inclusive; default:
 * today in New York + 30 days; at most 366 days): scheduled release dates
 * of a curated list of major US economic releases (CPI, employment, GDP,
 * …), from the newest schedule collected. Dates only: no time of day,
 * forecasts or figures. `notice` is the data provider's required
 * attribution. The FOMC meeting schedule is not included (no approved
 * machine-readable source).
 */
export const EconomicCalendarV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    region: z.literal("US"),
    from: DateString,
    to: DateString,
    releases: z.array(
      z.strictObject({
        date: DateString,
        name: z.string().min(1),
        category: z.enum(ECONOMIC_CATEGORIES),
      }),
    ),
    notice: z.string().min(1),
  })
  .refine((c) => c.releases.every((r) => c.from <= r.date && r.date <= c.to), {
    message: "releases fall in the window",
  });
export type EconomicCalendarV1 = z.infer<typeof EconomicCalendarV1>;

/** Largest and default page size of `GET /v1/markets`. */
export const MARKETS_MAX_LIMIT = 100;
export const MARKETS_DEFAULT_LIMIT = 50;

/** `sort=` of `/v1/markets` (V1.10): the market's price, 24-hour change or percent. */
export const MARKETS_SORT_KEYS = ["price", "changePercent", "change"] as const;
export type MarketsSortKey = (typeof MARKETS_SORT_KEYS)[number];
export const MARKETS_SORT_ORDERS = ["asc", "desc"] as const;

/**
 * One market (subject, unit) with a canonical quote. `market` is exactly
 * `/v1/market`'s body, `null` when no quote is servable now (an aggregate
 * older than its window). `sparkline` is the closes of up to the latest 24
 * hourly candles, oldest first; empty when the market has no hourly bars
 * (reference series, for one).
 */
export const MarketsRowV1 = z.strictObject({
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  market: MarketV1.nullable(),
  sparkline: z.array(DecimalString).max(24),
});
export type MarketsRowV1 = z.infer<typeof MarketsRowV1>;

/**
 * `GET /v1/markets?class=&q=&limit=&offset=&sort=&order=`: every market with a
 * canonical quote, one page at a time, ordered by subject name then unit.
 * - `sort` (`sort=price|changePercent|change`, `order=asc|desc`, default
 *   `asc`; V1.10): ordered by that value of the market as `/v1/market`
 *   serves it, as of at most a minute ago; markets without one come last,
 *   by name. Prices in different units are compared as numbers.
 * - `classes` (`class=crypto_asset,fx`): only instrument subjects of those
 *   classes; empty means all markets, currency subjects included.
 * - `query` (`q=`): subjects whose name or an alias (symbol or name)
 *   contains the text, case-insensitively.
 * `counts` is the number of markets per subject class over all markets
 * (`class: null` counts currency subjects), independent of both filters;
 * `total` is the filtered count.
 */
export const MarketsV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    classes: z.array(z.enum(INSTRUMENT_CLASSES)),
    query: z.string().min(1).nullable(),
    sort: z
      .strictObject({ key: z.enum(MARKETS_SORT_KEYS), order: z.enum(MARKETS_SORT_ORDERS) })
      .nullable(),
    total: z.number().int().nonnegative(),
    offset: z.number().int().nonnegative(),
    limit: z.number().int().min(1).max(MARKETS_MAX_LIMIT),
    counts: z.array(
      z.strictObject({
        class: z.enum(INSTRUMENT_CLASSES).nullable(),
        count: z.number().int().positive(),
      }),
    ),
    markets: z.array(MarketsRowV1),
  })
  .refine((m) => m.markets.length <= m.limit, { message: "at most limit markets per page" })
  .refine((m) => m.markets.length === 0 || m.offset + m.markets.length <= m.total, {
    message: "a page lies within total",
  });
export type MarketsV1 = z.infer<typeof MarketsV1>;

/**
 * `GET /v1/markets/query?id=&unit=`: the shortest query that resolves to
 * exactly this market (subject, unit), for people and code that would rather
 * write `/v1/quote/BTC-PERP` than ids (V1.9). Tried in order: an FX pair's
 * name, a symbol (`NVDA`), a symbol in the unit (`USDT/IDR`), a name, a
 * name in the unit; each must resolve, by the API's own resolver, to this
 * one market.
 * When none does, `query` is the canonical id with `?unit=` and `exact` is
 * `false`. A symbol can be reassigned: store ids, not queries.
 */
export const MarketQueryV1 = z.strictObject({
  schemaVersion: z.literal(1),
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  /** The path segment after `/v1/quote/` (with `?unit=` for the id form). */
  query: z.string().min(1),
  /** `true` when `query` is a readable form; `false` for the id fallback. */
  readable: z.boolean(),
});
export type MarketQueryV1 = z.infer<typeof MarketQueryV1>;
