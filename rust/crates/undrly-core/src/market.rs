//! Market data beyond quotes (V1.3, docs/v1.3-market-data.md): OHLC bars,
//! perpetual contexts, funding schedules and trading sessions.
//!
//! Each is a fact a source states, with its own time semantics. Nothing here
//! aggregates or interpolates: derived candles and statistics are computed at
//! read time from these facts, by documented rules.

use chrono::{NaiveDate, TimeDelta};
use rust_decimal::Decimal;

use crate::id::{InstrumentId, VenueId};
use crate::observation::{PriceSubject, PriceUnit};
use crate::time::Timestamp;

/// An interval a source publishes bars for (stored as published).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BarInterval {
    OneHour,
    OneDay,
}

impl BarInterval {
    pub const ALL: [BarInterval; 2] = [BarInterval::OneHour, BarInterval::OneDay];

    pub const fn as_str(self) -> &'static str {
        match self {
            BarInterval::OneHour => "1h",
            BarInterval::OneDay => "1d",
        }
    }

    /// The nominal length (a source's daily bar may span a local calendar day
    /// of 23 or 25 hours; its own close time is what is stored).
    pub fn nominal(self) -> TimeDelta {
        match self {
            BarInterval::OneHour => TimeDelta::hours(1),
            BarInterval::OneDay => TimeDelta::days(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BarError {
    #[error("a bar closes after it opens")]
    Time,
    #[error("OHLC out of order: low {low}, open {open}, high {high}, close {close}")]
    Ohlc {
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
    },
    #[error("negative volume {0}")]
    Volume(Decimal),
}

/// One OHLC bar of trade prices, as a venue published it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub venue: VenueId,
    pub interval: BarInterval,
    pub open_time: Timestamp,
    pub close_time: Timestamp,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    /// Quantity of the subject traded in the bar; `None` when not stated.
    pub volume: Option<Decimal>,
    pub trade_count: Option<u64>,
}

impl Bar {
    /// Checks the bar's own consistency: it closes after it opens, the low
    /// and high bound the open and close, and volume is not negative.
    pub fn validate(&self) -> Result<(), BarError> {
        if self.close_time <= self.open_time {
            return Err(BarError::Time);
        }
        let (o, h, l, c) = (self.open, self.high, self.low, self.close);
        if !(l <= h && l <= o && o <= h && l <= c && c <= h) {
            return Err(BarError::Ohlc {
                open: o,
                high: h,
                low: l,
                close: c,
            });
        }
        if let Some(v) = self.volume
            && v < Decimal::ZERO
        {
            return Err(BarError::Volume(v));
        }
        Ok(())
    }
}

/// A perpetual's market context as its venue reports it. Prices are in the
/// perpetual's price unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpContext {
    pub perpetual: InstrumentId,
    pub unit: PriceUnit,
    pub venue: VenueId,
    pub mark_price: Decimal,
    /// The venue's oracle (index) price.
    pub oracle_price: Option<Decimal>,
    pub mid_price: Option<Decimal>,
    /// A fraction per `funding_interval_hours` (e.g. 0.0000125 per 1 h).
    pub funding_rate: Option<(Decimal, u32)>,
    /// Open positions, in units of the underlying per contract multiplier.
    pub open_interest: Option<Decimal>,
    /// Traded quantity over the venue's trailing 24 hours, in contracts.
    pub volume_24h_base: Option<Decimal>,
    /// Traded value over the venue's trailing 24 hours, in the price unit.
    pub volume_24h_notional: Option<Decimal>,
    /// The price 24 hours earlier, as the venue states it.
    pub price_24h_ago: Option<Decimal>,
}

/// One trading date of a venue: extended (pre/post) and regular session
/// bounds, as absolute instants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradingSession {
    pub venue: VenueId,
    pub date: NaiveDate,
    pub pre_open_at: Timestamp,
    pub open_at: Timestamp,
    pub close_at: Timestamp,
    pub post_close_at: Timestamp,
}

impl TradingSession {
    pub fn is_ordered(&self) -> bool {
        self.pre_open_at <= self.open_at
            && self.open_at < self.close_at
            && self.close_at <= self.post_close_at
    }
}

/// A corporate action type (the supported subset of a source's types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorporateActionType {
    CashDividend,
    StockDividend,
    ForwardSplit,
    ReverseSplit,
    SpinOff,
    CashMerger,
    StockMerger,
    StockAndCashMerger,
    NameChange,
}

impl CorporateActionType {
    pub const ALL: [CorporateActionType; 9] = [
        CorporateActionType::CashDividend,
        CorporateActionType::StockDividend,
        CorporateActionType::ForwardSplit,
        CorporateActionType::ReverseSplit,
        CorporateActionType::SpinOff,
        CorporateActionType::CashMerger,
        CorporateActionType::StockMerger,
        CorporateActionType::StockAndCashMerger,
        CorporateActionType::NameChange,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CorporateActionType::CashDividend => "cash_dividend",
            CorporateActionType::StockDividend => "stock_dividend",
            CorporateActionType::ForwardSplit => "forward_split",
            CorporateActionType::ReverseSplit => "reverse_split",
            CorporateActionType::SpinOff => "spin_off",
            CorporateActionType::CashMerger => "cash_merger",
            CorporateActionType::StockMerger => "stock_merger",
            CorporateActionType::StockAndCashMerger => "stock_and_cash_merger",
            CorporateActionType::NameChange => "name_change",
        }
    }
}

/// What an instrument is in a corporate action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorporateActionRole {
    /// The dividend payer, the split or renamed security.
    Subject,
    /// The company being acquired in a merger.
    Acquiree,
    /// The acquiring company in a merger.
    Acquirer,
    /// The company a new security is spun off from.
    SpinOffParent,
}

impl CorporateActionRole {
    pub const ALL: [CorporateActionRole; 4] = [
        CorporateActionRole::Subject,
        CorporateActionRole::Acquiree,
        CorporateActionRole::Acquirer,
        CorporateActionRole::SpinOffParent,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CorporateActionRole::Subject => "subject",
            CorporateActionRole::Acquiree => "acquiree",
            CorporateActionRole::Acquirer => "acquirer",
            CorporateActionRole::SpinOffParent => "spin_off_parent",
        }
    }
}

/// A corporate action concerning one instrument, as its source states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateAction {
    pub instrument: InstrumentId,
    /// The source's own id for the action (stable across its updates).
    pub source_action_id: String,
    pub action_type: CorporateActionType,
    pub role: CorporateActionRole,
    pub ex_date: Option<NaiveDate>,
    pub record_date: Option<NaiveDate>,
    pub payable_date: Option<NaiveDate>,
    pub effective_date: Option<NaiveDate>,
    pub process_date: Option<NaiveDate>,
    /// Cash per share (the source states no currency).
    pub cash_amount: Option<Decimal>,
    /// Shares received per share held (stock dividends).
    pub stock_rate: Option<Decimal>,
    /// Old : new ratio (splits, mergers, spin-offs).
    pub ratio: Option<(Decimal, Decimal)>,
    pub special: Option<bool>,
    /// The other security's symbol, as the source spells it (not an identity).
    pub other_symbol: Option<String>,
}

/// When in the trading day a company reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportTime {
    BeforeOpen,
    AfterClose,
    DuringHours,
}

impl ReportTime {
    pub const ALL: [ReportTime; 3] = [
        ReportTime::BeforeOpen,
        ReportTime::AfterClose,
        ReportTime::DuringHours,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ReportTime::BeforeOpen => "before_open",
            ReportTime::AfterClose => "after_close",
            ReportTime::DuringHours => "during_hours",
        }
    }
}

/// A company's earnings report for one fiscal quarter, as its source states
/// it. Estimates are the source's own consensus; no currency is stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarningsReport {
    pub instrument: InstrumentId,
    pub fiscal_year: i32,
    pub fiscal_quarter: u32,
    pub date: NaiveDate,
    pub time: Option<ReportTime>,
    pub eps_estimate: Option<Decimal>,
    pub eps_actual: Option<Decimal>,
    pub revenue_estimate: Option<Decimal>,
    pub revenue_actual: Option<Decimal>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::parse_canonical;
    use crate::id::CurrencyId;

    fn bar(o: &str, h: &str, l: &str, c: &str, v: Option<&str>) -> Bar {
        let d = |s: &str| parse_canonical(s).unwrap();
        Bar {
            subject: PriceSubject::Instrument(InstrumentId::generate()),
            unit: PriceUnit::Currency(CurrencyId::generate()),
            venue: VenueId::generate(),
            interval: BarInterval::OneHour,
            open_time: Timestamp::parse("2026-09-26T03:00:00Z").unwrap(),
            close_time: Timestamp::parse("2026-09-26T04:00:00Z").unwrap(),
            open: d(o),
            high: d(h),
            low: d(l),
            close: d(c),
            volume: v.map(d),
            trade_count: None,
        }
    }

    #[test]
    fn bars_are_internally_consistent() {
        assert!(bar("10", "12", "9", "11", Some("3.5")).validate().is_ok());
        assert!(bar("10", "10", "10", "10", None).validate().is_ok());
        assert!(matches!(
            bar("13", "12", "9", "11", None).validate(),
            Err(BarError::Ohlc { .. })
        ));
        assert!(matches!(
            bar("10", "12", "9", "8", None).validate(),
            Err(BarError::Ohlc { .. })
        ));
        assert!(matches!(
            bar("10", "12", "9", "11", Some("-1")).validate(),
            Err(BarError::Volume(_))
        ));
        let mut b = bar("10", "12", "9", "11", None);
        b.close_time = b.open_time;
        assert_eq!(b.validate(), Err(BarError::Time));
    }
}
