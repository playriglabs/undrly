/**
 * V1.3 market data reads (docs/v1.3-market-data.md): candles, reference
 * series history, market context and perpetual derivatives data. Read-only
 * over what the collector stored; no request contacts an upstream.
 */
import { v1 } from "@undrly/contracts";
import { ageMs, policyElapsedMs } from "./market.ts";
import { canonicalTimestamp } from "./query.ts";
import {
  type CanonicalResult,
  type Pair,
  type Sql,
  subjectOf,
  tsText,
  unitOf,
  venueRef,
} from "./service.ts";

/** Why a market-data read has nothing to return. */
export type NoData = { kind: "no_data"; message: string };

export type Bounds = { limit: number; start: string | null; end: string | null };

type BarRow = {
  open_time: string;
  close_time: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string | null;
  complete: boolean;
  received_at: string;
};

const BAR_COLUMNS = `${tsText("open_time")} AS open_time, ${tsText("close_time")} AS close_time,
  open::text, high::text, low::text, close::text, volume::text,
  (close_time <= received_at) AS complete, ${tsText("received_at")} AS received_at`;

/** The venue bars of a market come from: the (source, venue) with the newest bar. */
async function barSource(
  sql: Sql,
  pair: Pair,
  interval: "1h" | "1d",
): Promise<{ source: string; venue: string } | null> {
  const rows = await sql<{ source: string; venue: string }[]>`
    SELECT source_id AS source, venue_id::text AS venue FROM market_bars
    WHERE subject_id = ${pair.subject} AND unit_id = ${pair.unit} AND bar_interval = ${interval}
    ORDER BY open_time DESC, source_id LIMIT 1`;
  return rows[0] ?? null;
}

/** The newest `limit` bars (within bounds), oldest first. */
async function bars(
  sql: Sql,
  pair: Pair,
  interval: "1h" | "1d",
  from: { source: string; venue: string },
  limit: number,
  start: string | null,
  end: string | null,
): Promise<BarRow[]> {
  const rows = await sql.unsafe<BarRow[]>(
    `SELECT ${BAR_COLUMNS} FROM market_bars
     WHERE subject_id = $1 AND unit_id = $2 AND bar_interval = $3 AND source_id = $4
       AND venue_id = $5 AND ($6::timestamptz IS NULL OR open_time >= $6::timestamptz)
       AND ($7::timestamptz IS NULL OR open_time < $7::timestamptz)
     ORDER BY open_time DESC LIMIT $8`,
    [pair.subject, pair.unit, interval, from.source, from.venue, start, end, limit],
  );
  return rows.reverse();
}

const dec = (s: string) => v1.parseDecimal(s) as v1.Decimal;
const max = (a: string, b: string) => (v1.decimalCompare(a, b) >= 0 ? a : b);
const min = (a: string, b: string) => (v1.decimalCompare(a, b) <= 0 ? a : b);

/** Sum of volumes, or `null` if any is unknown. */
function sumVolume(rows: BarRow[]): string | null {
  let total: v1.Decimal | null = { mantissa: 0n, scale: 0 };
  for (const r of rows) {
    if (r.volume === null || total === null) return null;
    total = v1.add(total, dec(r.volume));
  }
  return total === null ? null : v1.formatDecimal(total);
}

type Candle = {
  openTime: string;
  closeTime: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string | null;
  complete: boolean;
};

/** One candle from consecutive bars: first open, max high, min low, last close. */
function merge(rows: BarRow[]): Candle {
  const first = rows[0] as BarRow;
  const last = rows[rows.length - 1] as BarRow;
  return {
    openTime: canonicalTimestamp(first.open_time),
    closeTime: canonicalTimestamp(last.close_time),
    open: first.open,
    high: rows.map((r) => r.high).reduce(max),
    low: rows.map((r) => r.low).reduce(min),
    close: last.close,
    volume: sumVolume(rows),
    complete: rows.every((r) => r.complete),
  };
}

const HOUR_MS = 3_600_000;
const ms = (pgText: string) => Date.parse(`${canonicalTimestamp(pgText)}`);

/** `CandlesV1` for a market, or why there is none. */
export async function candles(
  sql: Sql,
  pair: Pair,
  interval: v1.CandleInterval,
  b: Bounds,
): Promise<v1.CandlesV1 | NoData> {
  const base = interval === "1d" ? "1d" : "1h";
  const from = await barSource(sql, pair, base);
  if (from === null) {
    return { kind: "no_data", message: `no ${interval} candles stored for this market` };
  }
  let out: Candle[];
  if (interval !== "4h") {
    out = (await bars(sql, pair, base, from, b.limit, b.start, b.end)).map((r) => merge([r]));
  } else {
    // Four consecutive hours per UTC-aligned 4-hour period; a gap omits it.
    const hours = await bars(sql, pair, "1h", from, b.limit * 4 + 3, b.start, b.end);
    const groups = new Map<number, BarRow[]>();
    for (const r of hours) {
      const t = ms(r.open_time);
      const key = t - (t % (4 * HOUR_MS));
      groups.set(key, [...(groups.get(key) ?? []), r]);
    }
    out = [...groups.entries()]
      .filter(
        ([key, rows]) =>
          rows.length === 4 && rows.every((r, i) => ms(r.open_time) === key + i * HOUR_MS),
      )
      .map(([, rows]) => merge(rows))
      .slice(-b.limit);
  }
  return v1.CandlesV1.parse({
    schemaVersion: 1,
    subject: await subjectOf(sql, pair.subject),
    unit: await unitOf(sql, pair.unit, pair.unitCategory),
    interval,
    priceType: "last",
    derived: interval === "4h",
    candles: out,
  });
}

/**
 * `HistoryV1`: the published values of the feed behind the pair's current
 * canonical quote, when that quote is a reference or average series.
 */
export async function history(sql: Sql, pair: Pair, b: Bounds): Promise<v1.HistoryV1 | NoData> {
  const feed = await sql<{ source: string; price_type: "reference" | "average" | string }[]>`
    SELECT o.source_id AS source, o.price_type FROM canonical_quote_inputs i
    JOIN market_observations o ON o.id = i.observation_id
    WHERE i.subject_id = ${pair.subject} AND i.unit_id = ${pair.unit} ORDER BY o.id LIMIT 1`;
  const f = feed[0];
  if (f === undefined) return { kind: "no_data", message: "no canonical quote to follow" };
  if (f.price_type !== "reference" && f.price_type !== "average") {
    return { kind: "no_data", message: "a traded market: use /v1/candles" };
  }
  const rows = await sql.unsafe<{ as_of: string; price: string }[]>(
    `SELECT ${tsText("observed_at")} AS as_of, price::text FROM market_observations
     WHERE subject_id = $1 AND unit_id = $2 AND source_id = $3 AND price_type = $4
       AND observed_at IS NOT NULL
       AND ($5::timestamptz IS NULL OR observed_at >= $5::timestamptz)
       AND ($6::timestamptz IS NULL OR observed_at < $6::timestamptz)
     ORDER BY observed_at DESC LIMIT $7`,
    [pair.subject, pair.unit, f.source, f.price_type, b.start, b.end, b.limit],
  );
  return v1.HistoryV1.parse({
    schemaVersion: 1,
    subject: await subjectOf(sql, pair.subject),
    unit: await unitOf(sql, pair.unit, pair.unitCategory),
    priceType: f.price_type,
    basis: "aggregated",
    observations: rows
      .reverse()
      .map((r) => ({ asOf: canonicalTimestamp(r.as_of), price: r.price })),
  });
}

type Status = (typeof v1.MARKET_STATUSES)[number];

/** A session venue's status at `now`, by its loaded calendar. */
async function sessionStatus(sql: Sql, venue: string, now: Date): Promise<Status> {
  const session = await sql<{ pre: Date; open: Date; close: Date; post: Date }[]>`
    SELECT pre_open_at AS pre, open_at AS open, close_at AS close, post_close_at AS post
    FROM trading_sessions
    WHERE venue_id = ${venue} AND pre_open_at <= ${now} AND ${now} < post_close_at`;
  const s = session[0];
  if (s !== undefined) {
    if (now < s.open) return "pre_market";
    if (now < s.close) return "open";
    return "after_hours";
  }
  // No session now: closed if the calendar covers today (New York date).
  const newYorkDate = new Date(now.getTime() - 5 * HOUR_MS).toISOString().slice(0, 10);
  const covered = await sql`
    SELECT 1 FROM trading_calendar_ranges
    WHERE venue_id = ${venue} AND first_date <= ${newYorkDate}::date
      AND last_date >= ${newYorkDate}::date LIMIT 1`;
  return covered.length > 0 ? "closed" : "unknown";
}

async function statistics(
  sql: Sql,
  pair: Pair,
  session: boolean,
): Promise<v1.MarketStatisticsV1 | null> {
  const interval = session ? "1d" : "1h";
  const from = await barSource(sql, pair, interval);
  if (from === null) return null;
  const rows = await bars(sql, pair, interval, from, session ? 2 : 24, null, null);
  const last = rows[rows.length - 1];
  if (last === undefined) return null;
  const to = last.complete ? last.close_time : last.received_at;
  if (session) {
    const [prev, today] = rows;
    if (prev === undefined || today === undefined || rows.length !== 2) return null;
    const c = v1.changeOf(today.close, prev.close);
    if (c === null) return null;
    return v1.MarketStatisticsV1.parse({
      window: "session",
      from: canonicalTimestamp(today.open_time),
      to: canonicalTimestamp(to),
      open: today.open,
      high: today.high,
      low: today.low,
      close: today.close,
      previousClose: prev.close,
      change: c.absolute,
      changePercent: c.percent,
      volume: today.volume,
      complete: today.complete,
    });
  }
  // 24 consecutive hourly bars ending with the latest, or nothing.
  const contiguous =
    rows.length === 24 &&
    rows.every((r, i) => i === 0 || ms(r.open_time) === ms(rows[i - 1]?.open_time ?? "") + HOUR_MS);
  if (!contiguous) return null;
  const w = merge(rows);
  const c = v1.changeOf(w.close, w.open);
  if (c === null) return null;
  return v1.MarketStatisticsV1.parse({
    window: "rolling_24h",
    from: w.openTime,
    to: canonicalTimestamp(to),
    open: w.open,
    high: w.high,
    low: w.low,
    close: w.close,
    previousClose: null,
    change: c.absolute,
    changePercent: c.percent,
    volume: w.volume,
    complete: w.complete,
  });
}

/** `MarketV1` from a canonical quote. */
export async function market(
  sql: Sql,
  pair: Pair,
  quote: Extract<CanonicalResult, { kind: "quote" }>["quote"],
  now: Date,
): Promise<v1.MarketV1> {
  const subject = quote.subject;
  const equity = subject.kind === "instrument" && subject.class === "equity";
  let marketStatus: Status | null;
  if (quote.priceType === "reference" || quote.priceType === "average") {
    marketStatus = null; // a publication, not a traded market
  } else if (equity) {
    marketStatus =
      quote.basis === "venue"
        ? await sessionStatus(sql, v1.canonicalIdUuid(quote.venue.id) ?? "", now)
        : "unknown";
  } else {
    marketStatus = "continuous";
  }
  return v1.MarketV1.parse({
    schemaVersion: 1,
    subject,
    unit: quote.unit,
    price: quote.price,
    priceType: quote.priceType,
    basis: quote.basis,
    asOf: quote.asOf,
    freshness: quote.freshness,
    marketStatus,
    statistics: marketStatus === null ? null : await statistics(sql, pair, equity),
  });
}

/** `DerivativesV1` for a perpetual, or why there is none. */
export async function derivatives(
  sql: Sql,
  pair: Pair,
  now: Date,
  staleAfterSeconds: number,
): Promise<v1.DerivativesV1 | NoData> {
  const subject = await subjectOf(sql, pair.subject);
  if (subject.kind !== "instrument" || subject.class !== "perpetual_future") {
    return { kind: "no_data", message: "derivatives data is for perpetuals" };
  }
  const rows = await sql.unsafe<
    {
      unit: string;
      unit_category: string;
      venue: string;
      source: string;
      mark: string;
      oracle: string | null;
      mid: string | null;
      funding: string | null;
      hours: number | null;
      oi: string | null;
      vol: string | null;
      ntl: string | null;
      prev: string | null;
      received_at: string;
    }[]
  >(
    `SELECT unit_id::text AS unit, unit_category, venue_id::text AS venue, source_id AS source,
            mark_price::text AS mark, oracle_price::text AS oracle, mid_price::text AS mid,
            funding_rate::text AS funding, funding_interval_hours AS hours,
            open_interest::text AS oi, volume_24h_base::text AS vol,
            volume_24h_notional::text AS ntl, price_24h_ago::text AS prev,
            ${tsText("received_at")} AS received_at
     FROM perp_contexts WHERE subject_id = $1 ORDER BY received_at DESC, id DESC LIMIT 1`,
    [pair.subject],
  );
  const c = rows[0];
  if (c === undefined) return { kind: "no_data", message: "no perpetual context collected" };
  const feed = await sql<{ seconds: number }[]>`
    SELECT stale_after_seconds AS seconds FROM quote_feeds
    WHERE subject_id = ${pair.subject} AND feed_source_id = ${c.source} AND price_type = 'mark'
    ORDER BY id LIMIT 1`;
  const window = feed[0]?.seconds ?? staleAfterSeconds;
  const asOf = canonicalTimestamp(c.received_at);
  const venue = await venueRef(sql, c.venue);
  if (venue === null) throw new Error(`unknown venue ${c.venue}`);
  return v1.DerivativesV1.parse({
    schemaVersion: 1,
    subject,
    unit: await unitOf(sql, c.unit, c.unit_category),
    basis: "venue",
    venue,
    markPrice: c.mark,
    indexPrice: c.oracle,
    midPrice: c.mid,
    fundingRate: c.funding,
    fundingIntervalHours: c.hours,
    openInterest: c.oi,
    volume24h: c.vol,
    volume24hNotional: c.ntl,
    price24hAgo: c.prev,
    asOf,
    ageMs: ageMs(asOf, now),
    freshness:
      policyElapsedMs(new Date(Date.parse(asOf)), now, "continuous") / 1000 <= window
        ? "fresh"
        : "stale",
  });
}

const newYorkDate = new Intl.DateTimeFormat("en-CA", {
  timeZone: "America/New_York",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

/** `YYYY-MM-DD` plus `days`, in calendar arithmetic. */
export function addDays(date: string, days: number): string {
  const t = Date.parse(`${date}T00:00:00Z`) + days * 86_400_000;
  return new Date(t).toISOString().slice(0, 10);
}

/** Today's date in New York at `now`. */
export function newYorkToday(now: Date): string {
  return newYorkDate.format(now);
}

/**
 * `CalendarV1`: the sessions of the calendar-bearing venue a market's feeds
 * name (equities: IEX), for `from..=to`, or why there is none.
 */
export async function calendar(
  sql: Sql,
  pair: Pair,
  from: string,
  to: string,
  now: Date,
): Promise<v1.CalendarV1 | NoData> {
  const venues = await sql<{ venue: string }[]>`
    SELECT DISTINCT f.venue_id::text AS venue FROM quote_feeds f
    WHERE f.subject_id = ${pair.subject} AND f.venue_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM trading_calendar_ranges r WHERE r.venue_id = f.venue_id)
    ORDER BY 1 LIMIT 1`;
  const subject = await subjectOf(sql, pair.subject);
  const venue = venues[0]?.venue;
  if (venue === undefined) {
    const continuous =
      subject.kind === "instrument" && ["crypto_asset", "perpetual_future"].includes(subject.class);
    return {
      kind: "no_data",
      message: continuous
        ? "a continuous market has no trading calendar"
        : "no trading calendar is loaded for this market",
    };
  }
  const ranges = await sql<{ first: string; last: string }[]>`
    SELECT first_date::text AS first, last_date::text AS last
    FROM trading_calendar_ranges WHERE venue_id = ${venue} ORDER BY first_date`;
  const covered = (d: string) => ranges.some((r) => r.first <= d && d <= r.last);
  const rows = await sql.unsafe<
    { date: string; pre: string; open: string; close: string; post: string; minutes: number }[]
  >(
    `SELECT session_date::text AS date, ${tsText("pre_open_at")} AS pre, ${tsText("open_at")} AS open,
            ${tsText("close_at")} AS close, ${tsText("post_close_at")} AS post,
            (extract(epoch FROM close_at - open_at) / 60)::int AS minutes
     FROM trading_sessions WHERE venue_id = $1 AND session_date BETWEEN $2::date AND $3::date
     ORDER BY session_date`,
    [venue, from, to],
  );
  const sessionDates = new Set(rows.map((r) => r.date));
  const closedWeekdays: string[] = [];
  for (let d = from; d <= to; d = addDays(d, 1)) {
    const weekday = new Date(`${d}T00:00:00Z`).getUTCDay();
    if (weekday !== 0 && weekday !== 6 && covered(d) && !sessionDates.has(d)) {
      closedWeekdays.push(d);
    }
  }
  const events = await sql<
    {
      type: (typeof v1.CORPORATE_ACTION_TYPES)[number];
      role: "subject" | "acquiree" | "acquirer" | "spin_off_parent";
      date: string;
      ex: string | null;
      record: string | null;
      payable: string | null;
      effective: string | null;
      cash: string | null;
      stock: string | null;
      old: string | null;
      new: string | null;
      special: boolean | null;
      other: string | null;
    }[]
  >`
    SELECT action_type AS type, role,
           coalesce(ex_date, effective_date, process_date)::text AS date,
           ex_date::text AS ex, record_date::text AS record, payable_date::text AS payable,
           effective_date::text AS effective, cash_amount::text AS cash, stock_rate::text AS stock,
           old_rate::text AS old, new_rate::text AS new, special, other_symbol AS other
    FROM corporate_actions
    WHERE instrument_id = ${pair.subject}
      AND coalesce(ex_date, effective_date, process_date) BETWEEN ${from}::date AND ${to}::date
    ORDER BY coalesce(ex_date, effective_date, process_date), action_type, source_action_id`;
  const reports = await sql<
    {
      date: string;
      time: "before_open" | "after_close" | "during_hours" | null;
      year: number;
      quarter: number;
      eps_e: string | null;
      eps_a: string | null;
      rev_e: string | null;
      rev_a: string | null;
    }[]
  >`
    SELECT report_date::text AS date, report_time AS time, fiscal_year AS year,
           fiscal_quarter AS quarter, eps_estimate::text AS eps_e, eps_actual::text AS eps_a,
           revenue_estimate::text AS rev_e, revenue_actual::text AS rev_a
    FROM earnings_events
    WHERE instrument_id = ${pair.subject} AND report_date BETWEEN ${from}::date AND ${to}::date
    ORDER BY report_date, fiscal_year, fiscal_quarter`;
  return v1.CalendarV1.parse({
    schemaVersion: 1,
    subject,
    timezone: "America/New_York",
    marketStatus: await sessionStatus(sql, venue, now),
    from,
    to,
    coverage: {
      from: ranges.map((r) => r.first).reduce((a, b) => (a < b ? a : b)),
      to: ranges.map((r) => r.last).reduce((a, b) => (a > b ? a : b)),
    },
    sessions: rows.map((r) => ({
      date: r.date,
      preOpen: canonicalTimestamp(r.pre),
      open: canonicalTimestamp(r.open),
      close: canonicalTimestamp(r.close),
      postClose: canonicalTimestamp(r.post),
      earlyClose: r.minutes < 390,
    })),
    closedWeekdays,
    events: events.map((e) => ({
      type: e.type,
      role: e.role,
      date: e.date,
      exDate: e.ex,
      recordDate: e.record,
      payableDate: e.payable,
      effectiveDate: e.effective,
      cashAmount: e.cash,
      stockRate: e.stock,
      ratio: e.old === null || e.new === null ? null : { old: e.old, new: e.new },
      special: e.special,
      otherSymbol: e.other,
    })),
    earnings: reports.map((r) => ({
      date: r.date,
      time: r.time,
      fiscalYear: r.year,
      fiscalQuarter: r.quarter,
      epsEstimate: r.eps_e,
      epsActual: r.eps_a,
      revenueEstimate: r.rev_e,
      revenueActual: r.rev_a,
    })),
  });
}

/** The FRED attribution its terms require wherever its data is shown. */
export const FRED_NOTICE =
  "This product uses the FRED® API but is not endorsed or certified by the Federal Reserve Bank of St. Louis.";

/**
 * `EconomicCalendarV1`: each release date in `from..=to` from the newest
 * collected schedule covering that date (a moved or cancelled date
 * disappears with the next collection).
 */
export async function economicCalendar(
  sql: Sql,
  from: string,
  to: string,
  category: string | null,
): Promise<v1.EconomicCalendarV1> {
  const rows = await sql<
    { date: string; name: string; category: (typeof v1.ECONOMIC_CATEGORIES)[number] }[]
  >`
    SELECT d.release_date::text AS date, d.release_name::text AS name, w.category
    FROM economic_release_dates d JOIN economic_calendar_windows w USING (source_record_id)
    WHERE d.release_date BETWEEN ${from}::date AND ${to}::date
      AND (${category}::text IS NULL OR w.category = ${category})
      AND w.received_at = (
        SELECT max(w2.received_at) FROM economic_calendar_windows w2
        WHERE w2.release_key = w.release_key
          AND w2.window_from <= d.release_date AND d.release_date <= w2.window_to)
    ORDER BY d.release_date, w.category, d.release_name`;
  return v1.EconomicCalendarV1.parse({
    schemaVersion: 1,
    region: "US",
    from,
    to,
    releases: rows,
    notice: FRED_NOTICE,
  });
}
