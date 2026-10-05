//! V1.3 normalizers (docs/v1.3-market-data.md): venue OHLC bars, perpetual
//! contexts and trading calendars.
//!
//! Every value is kept exactly as the source states it (decimal text, scale
//! preserved). Bar times are the source's own bar start; the close time is
//! the start plus the interval (a New York calendar day for Alpaca's daily
//! bars). A bar that is internally inconsistent (low above high, ...) is an
//! error, never repaired.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use undrly_core::{
    BarInterval, CorporateActionRole, CorporateActionType, Decimal, ReportTime, Timestamp,
    VenueSymbol,
};
use undrly_provider::JsonNumber;
use undrly_provider::alpaca::{BarsPage, CalendarDay, CorporateActionsPage};
use undrly_provider::finnhub::EarningsCalendar;
use undrly_provider::fred::ReleaseDates;
use undrly_provider::hyperliquid::{Candle, MetaAndAssetCtxs};
use undrly_provider::kraken::Ohlc;
use undrly_provider::{binance, bitkub, coinbase, geckoterminal, indodax, okx};

use crate::alpaca::us_daylight_saving;
use crate::{NormalizeError, decimal, invalid};

/// One bar a source reported under its own symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedBar {
    pub symbol: VenueSymbol,
    pub interval: BarInterval,
    pub open_time: Timestamp,
    pub close_time: Timestamp,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Option<Decimal>,
    pub trade_count: Option<u64>,
}

/// Normalizes one provider's bar payloads, for the requested symbols only.
pub trait BarNormalizer {
    type Bars;

    fn normalize_bars(
        &self,
        bars: &Self::Bars,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError>;
}

fn ts(field: &'static str, t: DateTime<Utc>) -> Result<Timestamp, NormalizeError> {
    Timestamp::from_datetime(t).map_err(|e| invalid(field, e))
}

fn from_unix_seconds(field: &'static str, s: i64) -> Result<Timestamp, NormalizeError> {
    let t = DateTime::from_timestamp(s, 0).ok_or_else(|| invalid(field, "out of range"))?;
    ts(field, t)
}

fn from_unix_millis(field: &'static str, ms: i64) -> Result<Timestamp, NormalizeError> {
    let t = DateTime::from_timestamp_millis(ms).ok_or_else(|| invalid(field, "out of range"))?;
    ts(field, t)
}

/// A JSON number as written in the payload, as an exact decimal.
fn number(field: &'static str, n: &JsonNumber) -> Result<Decimal, NormalizeError> {
    decimal(field, &n.0)
}

fn checked(bar: NormalizedBar) -> Result<NormalizedBar, NormalizeError> {
    let (o, h, l, c) = (bar.open, bar.high, bar.low, bar.close);
    if !(l <= h && l <= o && o <= h && l <= c && c <= h) {
        return Err(invalid(
            "bar",
            format!("{} {}: OHLC out of order", bar.symbol, bar.open_time),
        ));
    }
    if bar.volume.is_some_and(|v| v < Decimal::ZERO) {
        return Err(invalid("volume", format!("{}: negative", bar.symbol)));
    }
    Ok(bar)
}

fn close_of(interval: BarInterval, open: Timestamp) -> Result<Timestamp, NormalizeError> {
    ts("time", open.as_datetime() + interval.nominal())
}

// ---------------------------------------------------------------- Kraken

/// Kraken OHLC at the requested interval (the payload does not state it).
/// The bar still in progress (after `last`) is kept; its close time is in
/// the future relative to the fetch, which marks it incomplete.
pub struct KrakenBarNormalizer(pub BarInterval);

impl BarNormalizer for KrakenBarNormalizer {
    type Bars = Ohlc;

    fn normalize_bars(
        &self,
        o: &Ohlc,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = symbols.iter().find(|s| s.as_str() == o.pair) else {
            return Ok(Vec::new());
        };
        o.bars
            .iter()
            .map(|b| {
                let open_time = from_unix_seconds("time", b.time)?;
                checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time: close_of(self.0, open_time)?,
                    open: decimal("open", &b.open)?,
                    high: decimal("high", &b.high)?,
                    low: decimal("low", &b.low)?,
                    close: decimal("close", &b.close)?,
                    volume: Some(decimal("volume", &b.volume)?),
                    trade_count: Some(b.count),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------- Hyperliquid

/// Hyperliquid candles: interval from each candle (`1h`, `1d`); its stated
/// end (`T`, inclusive) must be the start plus the interval, less 1 ms.
pub struct HyperliquidBarNormalizer;

impl BarNormalizer for HyperliquidBarNormalizer {
    type Bars = Vec<Candle>;

    fn normalize_bars(
        &self,
        candles: &Vec<Candle>,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let mut out = Vec::new();
        for c in candles {
            let Some(symbol) = symbols.iter().find(|s| s.as_str() == c.s) else {
                continue;
            };
            let interval = match c.i.as_str() {
                "1h" => BarInterval::OneHour,
                "1d" => BarInterval::OneDay,
                other => {
                    return Err(NormalizeError::Unsupported {
                        field: "i",
                        value: other.to_owned(),
                    });
                }
            };
            let open_time = from_unix_millis("t", c.t)?;
            let close_time = close_of(interval, open_time)?;
            if close_time.as_datetime().timestamp_millis() != c.end + 1 {
                return Err(invalid(
                    "T",
                    format!("{}: end {} is not t + {}", c.s, c.end, c.i),
                ));
            }
            out.push(checked(NormalizedBar {
                symbol: symbol.clone(),
                interval,
                open_time,
                close_time,
                open: decimal("o", &c.o)?,
                high: decimal("h", &c.h)?,
                low: decimal("l", &c.l)?,
                close: decimal("c", &c.c)?,
                volume: Some(decimal("v", &c.v)?),
                trade_count: Some(c.n),
            })?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- Alpaca

/// The next midnight in New York after `local_midnight` (a UTC instant that
/// is 00:00 in New York): 23, 24 or 25 hours later.
fn next_new_york_midnight(at: DateTime<Utc>) -> DateTime<Utc> {
    let offset = |t: DateTime<Utc>| if us_daylight_saving(t) { 4 } else { 5 };
    // 00:00 local of the next day, in UTC, with that day's offset.
    let local_date = (at - Duration::hours(offset(at))).date_naive() + Duration::days(1);
    let guess = Utc.from_utc_datetime(&local_date.and_time(NaiveTime::MIN)) + Duration::hours(5);
    let o = offset(guess);
    Utc.from_utc_datetime(&local_date.and_time(NaiveTime::MIN)) + Duration::hours(o)
}

/// Alpaca IEX bars at the requested interval (`1Hour` or `1Day`; the
/// payload does not state it). Volume is IEX's own, in shares. A daily bar
/// spans its New York calendar day.
pub struct AlpacaBarNormalizer(pub BarInterval);

impl BarNormalizer for AlpacaBarNormalizer {
    type Bars = BarsPage;

    fn normalize_bars(
        &self,
        page: &BarsPage,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(bars) = page.bars.get(symbol.as_str()) else {
                continue;
            };
            for b in bars {
                let open_time = crate::timestamp("t", &b.t)?;
                let close_time = match self.0 {
                    BarInterval::OneHour => close_of(self.0, open_time)?,
                    BarInterval::OneDay => {
                        ts("t", next_new_york_midnight(open_time.as_datetime()))?
                    }
                };
                out.push(checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time,
                    open: decimal("o", &b.o.0)?,
                    high: decimal("h", &b.h.0)?,
                    low: decimal("l", &b.l.0)?,
                    close: decimal("c", &b.c.0)?,
                    volume: Some(decimal("v", &b.v.0)?),
                    trade_count: b.n,
                })?);
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- perpetuals

/// A perpetual's context as Hyperliquid states it (no source time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPerpContext {
    pub symbol: VenueSymbol,
    pub mark_price: Decimal,
    pub oracle_price: Option<Decimal>,
    pub mid_price: Option<Decimal>,
    /// Fraction per hour (Hyperliquid pays funding hourly).
    pub funding_rate_hourly: Option<Decimal>,
    pub open_interest: Option<Decimal>,
    pub volume_24h_base: Option<Decimal>,
    pub volume_24h_notional: Option<Decimal>,
    pub price_24h_ago: Option<Decimal>,
}

fn opt(field: &'static str, v: Option<&str>) -> Result<Option<Decimal>, NormalizeError> {
    v.map(|t| decimal(field, t)).transpose()
}

pub fn perp_contexts(
    d: &MetaAndAssetCtxs,
    symbols: &[VenueSymbol],
) -> Result<Vec<NormalizedPerpContext>, NormalizeError> {
    let mut out = Vec::new();
    for symbol in symbols {
        let Some(c) = d.context(symbol.as_str()) else {
            continue;
        };
        let Some(mark) = c.mark_px.as_deref() else {
            continue;
        };
        out.push(NormalizedPerpContext {
            symbol: symbol.clone(),
            mark_price: decimal("markPx", mark)?,
            oracle_price: opt("oraclePx", c.oracle_px.as_deref())?,
            mid_price: opt("midPx", c.mid_px.as_deref())?,
            funding_rate_hourly: opt("funding", c.funding.as_deref())?,
            open_interest: opt("openInterest", c.open_interest.as_deref())?,
            volume_24h_base: opt("dayBaseVlm", c.day_base_vlm.as_deref())?,
            volume_24h_notional: opt("dayNtlVlm", c.day_ntl_vlm.as_deref())?,
            price_24h_ago: opt("prevDayPx", c.prev_day_px.as_deref())?,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- calendar

/// One trading date in absolute time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSession {
    pub date: NaiveDate,
    pub pre_open_at: Timestamp,
    pub open_at: Timestamp,
    pub close_at: Timestamp,
    pub post_close_at: Timestamp,
}

/// `date` at local New York `hhmm` (`09:30` or `0930`), as an instant.
fn new_york(date: NaiveDate, hhmm: &str) -> Result<Timestamp, NormalizeError> {
    let digits: String = hhmm.chars().filter(char::is_ascii_digit).collect();
    let time = NaiveTime::parse_from_str(&digits, "%H%M")
        .map_err(|e| invalid("calendar time", format!("`{hhmm}`: {e}")))?;
    let naive = date.and_time(time);
    // Offset of that local time: EDT (UTC-4) when DST is in effect.
    let as_est = Utc.from_utc_datetime(&naive) + Duration::hours(5);
    let hours = if us_daylight_saving(as_est) { 4 } else { 5 };
    ts(
        "calendar time",
        Utc.from_utc_datetime(&naive) + Duration::hours(hours),
    )
}

/// Alpaca's US equity calendar: each listed date is a session (regular
/// `open`–`close`, extended `session_open`–`session_close`, New York time).
/// Dates not listed are days without a session.
pub fn trading_sessions(days: &[CalendarDay]) -> Result<Vec<NormalizedSession>, NormalizeError> {
    days.iter()
        .map(|d| {
            let date =
                NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").map_err(|e| invalid("date", e))?;
            let s = NormalizedSession {
                date,
                pre_open_at: new_york(date, &d.session_open)?,
                open_at: new_york(date, &d.open)?,
                close_at: new_york(date, &d.close)?,
                post_close_at: new_york(date, &d.session_close)?,
            };
            if !(s.pre_open_at <= s.open_at
                && s.open_at < s.close_at
                && s.close_at <= s.post_close_at)
            {
                return Err(invalid(
                    "calendar",
                    format!("{}: session out of order", d.date),
                ));
            }
            Ok(s)
        })
        .collect()
}

// ---------------------------------------------------------------- corporate actions

/// A corporate action concerning one requested symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCorporateAction {
    pub symbol: VenueSymbol,
    pub source_action_id: String,
    pub action_type: CorporateActionType,
    pub role: CorporateActionRole,
    pub ex_date: Option<NaiveDate>,
    pub record_date: Option<NaiveDate>,
    pub payable_date: Option<NaiveDate>,
    pub effective_date: Option<NaiveDate>,
    pub process_date: Option<NaiveDate>,
    pub cash_amount: Option<Decimal>,
    pub stock_rate: Option<Decimal>,
    pub ratio: Option<(Decimal, Decimal)>,
    pub special: Option<bool>,
    pub other_symbol: Option<String>,
}

/// Normalizes Alpaca corporate actions for the requested symbols. Each
/// action is attributed to every requested symbol it names, in its role
/// (a merger to its acquiree and its acquirer). Types outside
/// [`CorporateActionType`] are counted in the second value and skipped.
pub fn corporate_actions(
    page: &CorporateActionsPage,
    symbols: &[VenueSymbol],
) -> Result<(Vec<NormalizedCorporateAction>, usize), NormalizeError> {
    use CorporateActionRole as R;
    use CorporateActionType as T;
    let date =
        |field: &'static str, v: &Option<String>| -> Result<Option<NaiveDate>, NormalizeError> {
            v.as_deref()
                .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|e| invalid(field, e)))
                .transpose()
        };
    let num =
        |field: &'static str, v: &Option<JsonNumber>| -> Result<Option<Decimal>, NormalizeError> {
            v.as_ref()
                .map(|n| crate::worldbank::stored_double(&n.0).map_err(|_| invalid(field, &n.0)))
                .transpose()
        };
    let ratio = |a: Option<Decimal>, b: Option<Decimal>| match (a, b) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    let mut out = Vec::new();
    let mut skipped = 0;
    for (kind, actions) in &page.corporate_actions {
        let action_type = match kind.as_str() {
            "cash_dividends" => T::CashDividend,
            "stock_dividends" => T::StockDividend,
            "forward_splits" => T::ForwardSplit,
            "reverse_splits" => T::ReverseSplit,
            "spin_offs" => T::SpinOff,
            "cash_mergers" => T::CashMerger,
            "stock_mergers" => T::StockMerger,
            "stock_and_cash_mergers" => T::StockAndCashMerger,
            "name_changes" => T::NameChange,
            _ => {
                skipped += actions.len();
                continue;
            }
        };
        for a in actions {
            // (symbol, role, the other security's symbol)
            let parties: Vec<(&Option<String>, CorporateActionRole, &Option<String>)> =
                match action_type {
                    T::SpinOff => vec![(&a.source_symbol, R::SpinOffParent, &a.new_symbol)],
                    T::CashMerger | T::StockMerger | T::StockAndCashMerger => vec![
                        (&a.acquiree_symbol, R::Acquiree, &a.acquirer_symbol),
                        (&a.acquirer_symbol, R::Acquirer, &a.acquiree_symbol),
                    ],
                    T::NameChange => vec![(&a.old_symbol, R::Subject, &a.new_symbol)],
                    _ => vec![(&a.symbol, R::Subject, &None)],
                };
            let (cash_amount, stock_rate, terms) = match action_type {
                T::CashDividend => (num("rate", &a.rate)?, None, None),
                T::StockDividend => (None, num("rate", &a.rate)?, None),
                T::ForwardSplit | T::ReverseSplit => (
                    None,
                    None,
                    ratio(num("old_rate", &a.old_rate)?, num("new_rate", &a.new_rate)?),
                ),
                T::SpinOff => (
                    None,
                    None,
                    ratio(
                        num("source_rate", &a.source_rate)?,
                        num("new_rate", &a.new_rate)?,
                    ),
                ),
                // A cash merger states its cash per share as `rate`.
                T::CashMerger => (num("rate", &a.rate)?, None, None),
                T::StockMerger | T::StockAndCashMerger => (
                    num("cash_rate", &a.cash_rate)?,
                    None,
                    ratio(
                        num("acquiree_rate", &a.acquiree_rate)?,
                        num("acquirer_rate", &a.acquirer_rate)?,
                    ),
                ),
                T::NameChange => (None, None, None),
            };
            for (symbol, role, other) in parties {
                let Some(symbol) = symbol
                    .as_deref()
                    .and_then(|s| symbols.iter().find(|x| x.as_str() == s))
                else {
                    continue;
                };
                out.push(NormalizedCorporateAction {
                    symbol: symbol.clone(),
                    source_action_id: a.id.clone(),
                    action_type,
                    role,
                    ex_date: date("ex_date", &a.ex_date)?,
                    record_date: date("record_date", &a.record_date)?,
                    payable_date: date("payable_date", &a.payable_date)?,
                    effective_date: date("effective_date", &a.effective_date)?,
                    process_date: date("process_date", &a.process_date)?,
                    cash_amount,
                    stock_rate,
                    ratio: terms,
                    special: a.special,
                    other_symbol: other.clone(),
                });
            }
        }
    }
    Ok((out, skipped))
}

// ---------------------------------------------------------------- event calendars

/// The dates FRED states for `release_key` (its release id); a date of
/// another release in the payload is an error, never a guess.
pub fn economic_release_dates(
    r: &ReleaseDates,
    release_key: &str,
) -> Result<Vec<NaiveDate>, NormalizeError> {
    r.release_dates
        .iter()
        .map(|d| {
            if d.release_id.to_string() != release_key {
                return Err(invalid(
                    "release_id",
                    format!("{} in a response for {release_key}", d.release_id),
                ));
            }
            NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").map_err(|e| invalid("date", e))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEarnings {
    pub symbol: VenueSymbol,
    pub fiscal_year: i32,
    pub fiscal_quarter: u32,
    pub date: NaiveDate,
    pub time: Option<ReportTime>,
    pub eps_estimate: Option<Decimal>,
    pub eps_actual: Option<Decimal>,
    pub revenue_estimate: Option<Decimal>,
    pub revenue_actual: Option<Decimal>,
}

/// Finnhub earnings for the requested symbols. `hour`: `bmo` before the
/// open, `amc` after the close, `dmh` during market hours, empty when not
/// stated; anything else is an error. Numbers are JSON doubles, rounded in
/// decimal to 15 significant digits.
pub fn earnings(
    c: &EarningsCalendar,
    symbols: &[VenueSymbol],
) -> Result<Vec<NormalizedEarnings>, NormalizeError> {
    let num =
        |field: &'static str, v: &Option<JsonNumber>| -> Result<Option<Decimal>, NormalizeError> {
            v.as_ref()
                .map(|n| crate::worldbank::stored_double(&n.0).map_err(|_| invalid(field, &n.0)))
                .transpose()
        };
    let mut out = Vec::new();
    for e in &c.earnings {
        let Some(symbol) = symbols.iter().find(|s| s.as_str() == e.symbol) else {
            continue;
        };
        if !(1..=4).contains(&e.quarter) {
            return Err(invalid("quarter", format!("{}: {}", e.symbol, e.quarter)));
        }
        let time = match e.hour.as_deref().unwrap_or("") {
            "bmo" => Some(ReportTime::BeforeOpen),
            "amc" => Some(ReportTime::AfterClose),
            "dmh" => Some(ReportTime::DuringHours),
            "" => None,
            other => {
                return Err(NormalizeError::Unsupported {
                    field: "hour",
                    value: other.to_owned(),
                });
            }
        };
        out.push(NormalizedEarnings {
            symbol: symbol.clone(),
            fiscal_year: e.year,
            fiscal_quarter: e.quarter,
            date: NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").map_err(|x| invalid("date", x))?,
            time,
            eps_estimate: num("epsEstimate", &e.eps_estimate)?,
            eps_actual: num("epsActual", &e.eps_actual)?,
            revenue_estimate: num("revenueEstimate", &e.revenue_estimate)?,
            revenue_actual: num("revenueActual", &e.revenue_actual)?,
        });
    }
    Ok(out)
}

// ------------------------------------------- V1.9 stablecoin/fiat venues

/// The single symbol a bar payload that names no market belongs to.
fn single<'a>(
    symbols: &'a [VenueSymbol],
    what: &'static str,
) -> Result<Option<&'a VenueSymbol>, NormalizeError> {
    match symbols {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(invalid(
            what,
            "bars name no market; request one market per record",
        )),
    }
}

/// Binance-layout klines (Binance, Coins.ph, HashKey) at the requested
/// interval. The payload names neither market nor interval.
pub struct KlineBarNormalizer(pub BarInterval);

impl BarNormalizer for KlineBarNormalizer {
    type Bars = binance::Klines;

    fn normalize_bars(
        &self,
        k: &binance::Klines,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "klines")? else {
            return Ok(Vec::new());
        };
        k.0.iter()
            .map(|b| {
                let open_time = from_unix_millis("openTime", b.open_time)?;
                checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time: close_of(self.0, open_time)?,
                    open: decimal("open", &b.open)?,
                    high: decimal("high", &b.high)?,
                    low: decimal("low", &b.low)?,
                    close: decimal("close", &b.close)?,
                    volume: Some(decimal("volume", &b.volume)?),
                    trade_count: b.trades,
                })
            })
            .collect()
    }
}

/// A JSON number in plain or scientific notation (`1.2e-05`), as the exact
/// decimal it writes. GeckoTerminal states prices as JSON numbers.
fn plain_number(field: &'static str, n: &JsonNumber) -> Result<Decimal, NormalizeError> {
    let Some((mantissa, exp)) = n.0.split_once(['e', 'E']) else {
        return number(field, n);
    };
    let exp: i64 = exp.parse().map_err(|_| invalid(field, "bad exponent"))?;
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int}{frac}");
    // The decimal point sits `int.len() + exp` digits into `digits`.
    let point = int.len() as i64 + exp;
    let text = if point <= 0 {
        format!("0.{}{digits}", "0".repeat(point.unsigned_abs() as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(point as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    };
    let trimmed = text.trim_start_matches('0');
    let text = if trimmed.is_empty() || trimmed.starts_with('.') {
        format!("0{trimmed}")
    } else {
        trimmed.to_owned()
    };
    decimal(field, &format!("{sign}{text}"))
}

/// Coinbase Exchange candles (V1.10, newest first as served) at the
/// requested interval; volume in the base asset. The payload names neither
/// product nor interval.
pub struct CoinbaseBarNormalizer(pub BarInterval);

impl BarNormalizer for CoinbaseBarNormalizer {
    type Bars = coinbase::Candles;

    fn normalize_bars(
        &self,
        c: &coinbase::Candles,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "candles")? else {
            return Ok(Vec::new());
        };
        let mut bars =
            c.0.iter()
                .map(|b| {
                    let open_time = from_unix_millis("time", b.time.saturating_mul(1000))?;
                    checked(NormalizedBar {
                        symbol: symbol.clone(),
                        interval: self.0,
                        open_time,
                        close_time: close_of(self.0, open_time)?,
                        open: plain_number("open", &b.open)?,
                        high: plain_number("high", &b.high)?,
                        low: plain_number("low", &b.low)?,
                        close: plain_number("close", &b.close)?,
                        volume: Some(plain_number("volume", &b.volume)?),
                        trade_count: None,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
        bars.reverse();
        Ok(bars)
    }
}

/// GeckoTerminal pool OHLC (V1.10, newest first as served) at the requested
/// interval, in USD for the priced side of the pool. The payload names
/// neither pool nor interval. Volume (USD, the pool's) is not kept.
pub struct GeckoTerminalBarNormalizer(pub BarInterval);

impl BarNormalizer for GeckoTerminalBarNormalizer {
    type Bars = geckoterminal::Ohlcv;

    fn normalize_bars(
        &self,
        o: &geckoterminal::Ohlcv,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "ohlcv")? else {
            return Ok(Vec::new());
        };
        let mut bars =
            o.0.iter()
                .map(|b| {
                    let open_time = from_unix_millis("openTime", b.open_time.saturating_mul(1000))?;
                    checked(NormalizedBar {
                        symbol: symbol.clone(),
                        interval: self.0,
                        open_time,
                        close_time: close_of(self.0, open_time)?,
                        open: plain_number("open", &b.open)?,
                        high: plain_number("high", &b.high)?,
                        low: plain_number("low", &b.low)?,
                        close: plain_number("close", &b.close)?,
                        volume: None,
                        trade_count: None,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
        bars.reverse();
        Ok(bars)
    }
}

/// OKX candles (newest first as served) at the requested interval.
pub struct OkxBarNormalizer(pub BarInterval);

impl BarNormalizer for OkxBarNormalizer {
    type Bars = okx::Candles;

    fn normalize_bars(
        &self,
        c: &okx::Candles,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "candles")? else {
            return Ok(Vec::new());
        };
        c.0.iter()
            .map(|b| {
                let open_time = from_unix_millis("ts", b.ts)?;
                checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time: close_of(self.0, open_time)?,
                    open: decimal("open", &b.open)?,
                    high: decimal("high", &b.high)?,
                    low: decimal("low", &b.low)?,
                    close: decimal("close", &b.close)?,
                    volume: Some(decimal("vol", &b.volume)?),
                    trade_count: None,
                })
            })
            .collect()
    }
}

/// Indodax chart bars (prices as JSON numbers, kept as written).
pub struct IndodaxBarNormalizer(pub BarInterval);

impl BarNormalizer for IndodaxBarNormalizer {
    type Bars = indodax::Bars;

    fn normalize_bars(
        &self,
        b: &indodax::Bars,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "bars")? else {
            return Ok(Vec::new());
        };
        b.0.iter()
            .map(|bar| {
                let open_time = from_unix_seconds("Time", bar.time)?;
                checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time: close_of(self.0, open_time)?,
                    open: number("Open", &bar.open)?,
                    high: number("High", &bar.high)?,
                    low: number("Low", &bar.low)?,
                    close: number("Close", &bar.close)?,
                    volume: Some(decimal("Volume", &bar.volume)?),
                    trade_count: None,
                })
            })
            .collect()
    }
}

/// Bitkub chart bars (columnar; prices as JSON numbers, kept as written).
/// `no_data` is an empty window.
pub struct BitkubBarNormalizer(pub BarInterval);

impl BarNormalizer for BitkubBarNormalizer {
    type Bars = bitkub::History;

    fn normalize_bars(
        &self,
        h: &bitkub::History,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedBar>, NormalizeError> {
        let Some(symbol) = single(symbols, "history")? else {
            return Ok(Vec::new());
        };
        if h.s == "no_data" {
            return Ok(Vec::new());
        }
        if h.s != "ok" {
            return Err(invalid("s", format!("status `{}`", h.s)));
        }
        (0..h.t.len())
            .map(|i| {
                let open_time = from_unix_seconds("t", h.t[i])?;
                checked(NormalizedBar {
                    symbol: symbol.clone(),
                    interval: self.0,
                    open_time,
                    close_time: close_of(self.0, open_time)?,
                    open: number("o", &h.o[i])?,
                    high: number("h", &h.h[i])?,
                    low: number("l", &h.l[i])?,
                    close: number("c", &h.c[i])?,
                    volume: Some(number("v", &h.v[i])?),
                    trade_count: None,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::hyperliquid::HyperliquidProvider;
    use undrly_provider::kraken::KrakenProvider;
    use undrly_provider::{BarsProvider, QuoteProvider, alpaca};

    use super::*;

    #[test]
    fn coinbase_candles_oldest_first_in_their_own_column_order() {
        let c = undrly_provider::coinbase::CoinbaseProvider::new()
            .decode_bars(
                br#"[[1791169200,0.00063,0.0006363,0.000634,0.0006359,1805388],
                     [1791165600,0.000631,0.000639,0.0006338,0.000634,24373202]]"#,
            )
            .unwrap();
        let sym = VenueSymbol::new("AMP-USD").unwrap();
        let b = CoinbaseBarNormalizer(BarInterval::OneHour)
            .normalize_bars(&c, std::slice::from_ref(&sym))
            .unwrap();
        assert_eq!(b.len(), 2);
        // [time, low, high, open, close, volume]
        assert_eq!(
            (
                b[0].open.to_string(),
                b[0].high.to_string(),
                b[0].low.to_string()
            ),
            ("0.0006338".into(), "0.000639".into(), "0.000631".into())
        );
        assert_eq!(b[1].close.to_string(), "0.0006359");
        assert_eq!(b[1].volume.unwrap().to_string(), "1805388");
    }

    #[test]
    fn geckoterminal_bars_oldest_first_with_exact_numbers() {
        use undrly_provider::BarsProvider;
        let o = undrly_provider::geckoterminal::GeckoTerminalProvider::new()
            .decode_bars(
                br#"{"data":{"attributes":{"ohlcv_list":[
                  [1791133200,1.2e-05,1.3e-05,1.1e-05,1.25e-05,10.5],
                  [1791129600,630.04,633.5,626.9,628.56,1206.0]]}}}"#,
            )
            .unwrap();
        let sym = VenueSymbol::new("b:CXio").unwrap();
        let b = GeckoTerminalBarNormalizer(BarInterval::OneHour)
            .normalize_bars(&o, std::slice::from_ref(&sym))
            .unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].open.to_string(), "630.04");
        assert_eq!(b[1].open.to_string(), "0.000012");
        assert_eq!(b[1].close.to_string(), "0.0000125");
        assert!(b[0].volume.is_none());
        assert_eq!(
            plain_number("x", &undrly_provider::JsonNumber("-1.5E3".into()))
                .unwrap()
                .to_string(),
            "-1500"
        );
    }

    fn fixture(path: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources")
                .join(path),
        )
        .unwrap()
    }

    fn sym(s: &str) -> Vec<VenueSymbol> {
        vec![VenueSymbol::new(s).unwrap()]
    }

    #[test]
    fn kraken_bars_keep_text_and_the_in_progress_bar() {
        let o = KrakenProvider::new()
            .decode_bars(&fixture("kraken/ohlc-XXBTZUSD-60.json"))
            .unwrap();
        let b = KrakenBarNormalizer(BarInterval::OneHour)
            .normalize_bars(&o, &sym("XXBTZUSD"))
            .unwrap();
        assert_eq!(b.len(), 6);
        assert_eq!(b[0].open_time.to_string(), "2026-09-25T23:00:00Z");
        assert_eq!(b[0].close_time.to_string(), "2026-09-26T00:00:00Z");
        assert!(
            b.iter()
                .all(|x| x.volume.is_some() && x.trade_count.is_some())
        );
        assert!(
            KrakenBarNormalizer(BarInterval::OneHour)
                .normalize_bars(&o, &sym("ZEURZUSD"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hyperliquid_candles_check_their_stated_end() {
        let mut c = HyperliquidProvider::new()
            .decode_bars(&fixture("hyperliquid/candles-BTC-1h.json"))
            .unwrap();
        let b = HyperliquidBarNormalizer
            .normalize_bars(&c, &sym("BTC"))
            .unwrap();
        assert_eq!(b.len(), 5);
        assert_eq!(
            b[0].close_time.as_datetime() - b[0].open_time.as_datetime(),
            Duration::hours(1)
        );
        c[0].end += 1;
        assert!(
            HyperliquidBarNormalizer
                .normalize_bars(&c, &sym("BTC"))
                .is_err()
        );
        c[0].end -= 1;
        c[0].h = "1".into();
        assert!(
            HyperliquidBarNormalizer
                .normalize_bars(&c, &sym("BTC"))
                .is_err()
        );
    }

    #[test]
    fn alpaca_daily_bars_span_a_new_york_day() {
        let p = alpaca::AlpacaProvider::new()
            .decode_bars(&fixture("alpaca/bars-1Day.json"))
            .unwrap();
        let b = AlpacaBarNormalizer(BarInterval::OneDay)
            .normalize_bars(&p, &sym("AAPL"))
            .unwrap();
        assert_eq!(b.len(), 5);
        assert_eq!(b[0].open_time.to_string(), "2026-09-21T04:00:00Z");
        assert_eq!(b[0].close_time.to_string(), "2026-09-22T04:00:00Z");
        assert_eq!(b[0].open.to_string(), "335.49");
        assert_eq!(b[0].volume.unwrap().to_string(), "929958");
        // DST ends 2026-11-01: that New York day lasts 25 hours.
        let nov1 = Utc.with_ymd_and_hms(2026, 11, 1, 4, 0, 0).unwrap();
        assert_eq!(
            next_new_york_midnight(nov1),
            Utc.with_ymd_and_hms(2026, 11, 2, 5, 0, 0).unwrap()
        );
        let mar8 = Utc.with_ymd_and_hms(2026, 3, 8, 5, 0, 0).unwrap();
        assert_eq!(
            next_new_york_midnight(mar8),
            Utc.with_ymd_and_hms(2026, 3, 9, 4, 0, 0).unwrap()
        );
    }

    #[test]
    fn perp_context_keeps_source_units() {
        let d = HyperliquidProvider::new()
            .decode_quote(&fixture("hyperliquid/metaAndAssetCtxs.json"))
            .unwrap();
        let c = perp_contexts(&d, &sym("BTC")).unwrap();
        assert_eq!(c[0].funding_rate_hourly.unwrap().to_string(), "0.0000125");
        assert_eq!(c[0].open_interest.unwrap().to_string(), "39202.60446");
        assert_eq!(c[0].oracle_price.unwrap().to_string(), "84217.0");
    }

    #[test]
    fn corporate_actions_by_role_with_exact_terms() {
        let p =
            alpaca::decode_corporate_actions(&fixture("alpaca/corporate-actions.json")).unwrap();
        let syms: Vec<VenueSymbol> = ["NVDA", "ADKT", "ACLX", "HURA"]
            .iter()
            .map(|s| VenueSymbol::new(s).unwrap())
            .collect();
        let (a, skipped) = corporate_actions(&p, &syms).unwrap();
        assert_eq!(skipped, 0);
        let nvda: Vec<_> = a.iter().filter(|x| x.symbol.as_str() == "NVDA").collect();
        assert!(!nvda.is_empty());
        assert!(
            nvda.iter()
                .all(|x| x.action_type == CorporateActionType::CashDividend
                    && x.role == CorporateActionRole::Subject
                    && x.cash_amount.is_some()
                    && x.ex_date.is_some())
        );
        let split = a
            .iter()
            .find(|x| x.action_type == CorporateActionType::ForwardSplit)
            .unwrap();
        assert_eq!(split.symbol.as_str(), "ADKT");
        assert_eq!(
            split.ratio.map(|(o, n)| (o.to_string(), n.to_string())),
            Some(("1".into(), "2".into()))
        );
        let merger = a
            .iter()
            .find(|x| x.action_type == CorporateActionType::StockAndCashMerger)
            .unwrap();
        assert_eq!(
            (merger.symbol.as_str(), merger.role),
            ("ACLX", CorporateActionRole::Acquiree)
        );
        assert_eq!(merger.cash_amount.unwrap().to_string(), "115");
        // HURA is the spun-off company (not the parent) in the spin-off, and
        // the acquirer in a stock merger: only the merger is its action.
        let hura: Vec<_> = a.iter().filter(|x| x.symbol.as_str() == "HURA").collect();
        assert_eq!(hura.len(), 1);
        assert_eq!(
            (hura[0].action_type, hura[0].role),
            (
                CorporateActionType::StockMerger,
                CorporateActionRole::Acquirer
            )
        );
    }

    #[test]
    fn economic_dates_and_earnings() {
        use undrly_provider::QuoteProvider;
        let r = undrly_provider::fred::FredProvider::new()
            .decode_quote(&fixture("fred/release-dates-10.json"))
            .unwrap();
        let d = economic_release_dates(&r, "10").unwrap();
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].to_string(), "2026-08-12");
        assert!(economic_release_dates(&r, "50").is_err());
        let c = undrly_provider::finnhub::FinnhubProvider::new()
            .decode_quote(&fixture("finnhub/earnings-calendar.json"))
            .unwrap();
        let e = earnings(&c, &sym("NVDA")).unwrap();
        assert_eq!(e.len(), 3, "ZZZZ is not requested");
        assert_eq!(e[0].time, Some(ReportTime::AfterClose));
        assert_eq!(e[0].eps_estimate.unwrap().to_string(), "1.2501");
        assert_eq!(e[1].revenue_actual.unwrap().to_string(), "46743000000");
        assert_eq!(e[2].time, None);
        let mut bad = c.clone();
        bad.earnings[0].hour = Some("noon".into());
        assert!(earnings(&bad, &sym("NVDA")).is_err());
    }

    #[test]
    fn calendar_sessions_in_new_york_time() {
        let days = alpaca::decode_calendar(&fixture("alpaca/calendar.json")).unwrap();
        let s = trading_sessions(&days).unwrap();
        let sep21 = s
            .iter()
            .find(|x| x.date.to_string() == "2026-09-21")
            .unwrap();
        assert_eq!(sep21.open_at.to_string(), "2026-09-21T13:30:00Z");
        assert_eq!(sep21.close_at.to_string(), "2026-09-21T20:00:00Z");
        assert_eq!(sep21.pre_open_at.to_string(), "2026-09-21T08:00:00Z");
        // After DST ends: EST, and the day after Thanksgiving closes at 13:00.
        let nov27 = s
            .iter()
            .find(|x| x.date.to_string() == "2026-11-27")
            .unwrap();
        assert_eq!(nov27.open_at.to_string(), "2026-11-27T14:30:00Z");
        assert_eq!(nov27.close_at.to_string(), "2026-11-27T18:00:00Z");
    }

    #[test]
    fn v19_venue_bars_are_hour_aligned_and_consistent() {
        use undrly_provider::bitkub::BitkubProvider;
        use undrly_provider::coins_ph::CoinsPhProvider;
        use undrly_provider::hashkey::HashKeyProvider;
        use undrly_provider::indodax::IndodaxProvider;
        use undrly_provider::okx::OkxProvider;

        let h = BarInterval::OneHour;
        let check = |bars: Vec<NormalizedBar>, symbol: &str| {
            assert!(!bars.is_empty(), "{symbol}");
            for b in &bars {
                assert_eq!(b.symbol.as_str(), symbol);
                assert_eq!(b.open_time.as_datetime().timestamp() % 3600, 0);
                assert_eq!(
                    b.close_time.as_datetime() - b.open_time.as_datetime(),
                    chrono::Duration::hours(1)
                );
            }
        };
        let k = binance::BinanceProvider::new()
            .decode_bars(&fixture("binance/klines-USDTIDR-1h.json"))
            .unwrap();
        check(
            KlineBarNormalizer(h)
                .normalize_bars(&k, &sym("USDTIDR"))
                .unwrap(),
            "USDTIDR",
        );
        let k = CoinsPhProvider::new()
            .decode_bars(&fixture("coins-ph/klines-USDTPHP-1h.json"))
            .unwrap();
        check(
            KlineBarNormalizer(h)
                .normalize_bars(&k, &sym("USDTPHP"))
                .unwrap(),
            "USDTPHP",
        );
        let k = HashKeyProvider::new()
            .decode_bars(&fixture("hashkey/klines-USDTHKD-1h.json"))
            .unwrap();
        check(
            KlineBarNormalizer(h)
                .normalize_bars(&k, &sym("USDTHKD"))
                .unwrap(),
            "USDTHKD",
        );
        let c = OkxProvider::new()
            .decode_bars(&fixture("okx/candles-USDT-SGD-1H.json"))
            .unwrap();
        check(
            OkxBarNormalizer(h)
                .normalize_bars(&c, &sym("USDT-SGD"))
                .unwrap(),
            "USDT-SGD",
        );
        let b = IndodaxProvider::new()
            .decode_bars(&fixture("indodax/history-USDTIDR-60.json"))
            .unwrap();
        let bars = IndodaxBarNormalizer(h)
            .normalize_bars(&b, &sym("usdtidr"))
            .unwrap();
        // JSON numbers become decimals exactly as written.
        assert_eq!(bars[0].open.to_string(), b.0[0].open.0);
        check(bars, "usdtidr");
        let b = BitkubProvider::new()
            .decode_bars(&fixture("bitkub/history-USDT_THB-60.json"))
            .unwrap();
        check(
            BitkubBarNormalizer(h)
                .normalize_bars(&b, &sym("USDT_THB"))
                .unwrap(),
            "USDT_THB",
        );
        // A payload naming no market belongs to exactly one requested symbol.
        let two = [
            VenueSymbol::new("A").unwrap(),
            VenueSymbol::new("B").unwrap(),
        ];
        assert!(
            KlineBarNormalizer(h)
                .normalize_bars(&k_empty(), &two)
                .is_err()
        );
    }

    fn k_empty() -> binance::Klines {
        binance::Klines(Vec::new())
    }
}
