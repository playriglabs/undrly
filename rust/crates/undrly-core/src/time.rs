//! UTC timestamps.
//!
//! [`Timestamp`] wraps `chrono::DateTime<Utc>`, so UTC is part of the type and
//! local/offset times cannot be stored by accident. Two extra invariants:
//!
//! - **microsecond precision.** PostgreSQL `timestamptz` stores microseconds;
//!   holding finer precision here would be silently truncated on persistence
//!   and break deterministic round-trips. Sub-microsecond input is rejected
//!   unless the caller explicitly truncates.
//! - **years 0001..=9999**, the range RFC 3339 can express, and no leap-second
//!   representation.
//!
//! The canonical text form is RFC 3339 in UTC with a `Z` suffix and the
//! shortest of 0, 3, or 6 fractional digits that represents the value exactly,
//! e.g. `2026-09-24T12:00:00Z`, `2026-09-24T12:00:00.012Z`.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, SecondsFormat, Timelike, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(DateTime<Utc>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimestampError {
    #[error("timestamp has sub-microsecond precision")]
    SubMicrosecond,
    #[error("timestamp is a leap second")]
    LeapSecond,
    #[error("timestamp year is outside 0001..=9999")]
    OutOfRange,
    #[error("`{0}` is not a canonical RFC 3339 UTC timestamp")]
    NonCanonical(String),
}

impl Timestamp {
    /// Wraps a UTC datetime, rejecting values that violate the invariants.
    pub fn from_datetime(value: DateTime<Utc>) -> Result<Self, TimestampError> {
        let nanos = value.nanosecond();
        if nanos >= 1_000_000_000 {
            return Err(TimestampError::LeapSecond);
        }
        if !nanos.is_multiple_of(1_000) {
            return Err(TimestampError::SubMicrosecond);
        }
        if !(1..=9999).contains(&value.year()) {
            return Err(TimestampError::OutOfRange);
        }
        Ok(Self(value))
    }

    /// Wraps a UTC datetime, explicitly discarding sub-microsecond precision.
    pub fn from_datetime_truncating(value: DateTime<Utc>) -> Result<Self, TimestampError> {
        let nanos = value.nanosecond();
        if nanos >= 1_000_000_000 {
            return Err(TimestampError::LeapSecond);
        }
        let truncated = value
            .with_nanosecond(nanos - nanos % 1_000)
            .ok_or(TimestampError::OutOfRange)?;
        Self::from_datetime(truncated)
    }

    pub fn from_unix_micros(micros: i64) -> Result<Self, TimestampError> {
        DateTime::from_timestamp_micros(micros)
            .ok_or(TimestampError::OutOfRange)
            .and_then(Self::from_datetime)
    }

    /// Current time from the system clock, truncated to microseconds.
    pub fn now() -> Self {
        Self::from_datetime_truncating(Utc::now())
            .expect("system clock is within the supported range")
    }

    /// Parses canonical text only (see module docs). Accepting exactly one
    /// spelling per instant keeps serialization deterministic.
    pub fn parse(s: &str) -> Result<Self, TimestampError> {
        let non_canonical = || TimestampError::NonCanonical(s.to_owned());
        let parsed = DateTime::parse_from_rfc3339(s).map_err(|_| non_canonical())?;
        let timestamp = Self::from_datetime(parsed.with_timezone(&Utc))?;
        if timestamp.to_rfc3339() != s {
            return Err(non_canonical());
        }
        Ok(timestamp)
    }

    pub fn to_rfc3339(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::AutoSi, true)
    }

    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }

    pub fn unix_micros(&self) -> i64 {
        self.0.timestamp_micros()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// A half-open period `[from, until)`. `None` means unbounded on that side.
/// Invariant: `from < until` when both are set, so a period is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Validity {
    from: Option<Timestamp>,
    until: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("validity period is empty: `from` must be before `until`")]
pub struct EmptyValidity;

impl Validity {
    /// No known bounds.
    pub const UNBOUNDED: Validity = Validity {
        from: None,
        until: None,
    };

    pub fn new(from: Option<Timestamp>, until: Option<Timestamp>) -> Result<Self, EmptyValidity> {
        match (from, until) {
            (Some(f), Some(u)) if f >= u => Err(EmptyValidity),
            _ => Ok(Self { from, until }),
        }
    }

    pub fn from(&self) -> Option<Timestamp> {
        self.from
    }

    pub fn until(&self) -> Option<Timestamp> {
        self.until
    }

    pub fn contains(&self, t: Timestamp) -> bool {
        self.from.is_none_or(|f| f <= t) && self.until.is_none_or(|u| t < u)
    }

    pub fn overlaps(&self, other: &Validity) -> bool {
        let starts_before_other_ends = match (self.from, other.until) {
            (Some(f), Some(u)) => f < u,
            _ => true,
        };
        let other_starts_before_self_ends = match (other.from, self.until) {
            (Some(f), Some(u)) => f < u,
            _ => true,
        };
        starts_before_other_ends && other_starts_before_self_ends
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn validity_is_half_open_and_non_empty() {
        let t = |s: &str| Timestamp::parse(s).unwrap();
        let (a, b, c) = (
            t("2020-01-01T00:00:00Z"),
            t("2021-01-01T00:00:00Z"),
            t("2022-01-01T00:00:00Z"),
        );
        assert_eq!(Validity::new(Some(b), Some(a)), Err(EmptyValidity));
        assert_eq!(Validity::new(Some(a), Some(a)), Err(EmptyValidity));
        let first = Validity::new(Some(a), Some(b)).unwrap();
        let second = Validity::new(Some(b), Some(c)).unwrap();
        assert!(first.contains(a));
        assert!(!first.contains(b));
        assert!(!first.overlaps(&second), "adjacent periods do not overlap");
        assert!(Validity::UNBOUNDED.overlaps(&first));
        assert!(Validity::new(None, Some(b)).unwrap().overlaps(&first));
        assert!(!Validity::new(Some(c), None).unwrap().overlaps(&first));
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    #[test]
    fn canonical_text_uses_shortest_exact_fraction() {
        let base = utc(2026, 9, 24, 12, 0, 0);
        let cases = [
            (0, "2026-09-24T12:00:00Z"),
            (12_000_000, "2026-09-24T12:00:00.012Z"),
            (12_345_000, "2026-09-24T12:00:00.012345Z"),
            (100_000, "2026-09-24T12:00:00.000100Z"),
        ];
        for (nanos, text) in cases {
            let ts = Timestamp::from_datetime(base.with_nanosecond(nanos).unwrap()).unwrap();
            assert_eq!(ts.to_rfc3339(), text);
            assert_eq!(Timestamp::parse(text).unwrap(), ts);
        }
    }

    #[test]
    fn rejects_non_canonical_text() {
        for text in [
            "2026-09-24T12:00:00",
            "2026-09-24T12:00:00+00:00",
            "2026-09-24T19:00:00+07:00",
            "2026-09-24T12:00:00z",
            "2026-09-24t12:00:00Z",
            "2026-09-24 12:00:00Z",
            "2026-09-24T12:00:00.000Z",
            "2026-09-24T12:00:00.0120Z",
            "2026-09-24T12:00:00.012000Z",
            "2026-09-24",
            "1790208000",
            "2026-02-30T00:00:00Z",
        ] {
            assert!(Timestamp::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn rejects_sub_microsecond_precision() {
        assert_eq!(
            Timestamp::parse("2026-09-24T12:00:00.000000001Z"),
            Err(TimestampError::SubMicrosecond)
        );
        let dt = utc(2026, 9, 24, 12, 0, 0)
            .with_nanosecond(1_234_567)
            .unwrap();
        assert_eq!(
            Timestamp::from_datetime(dt),
            Err(TimestampError::SubMicrosecond)
        );
        let truncated = Timestamp::from_datetime_truncating(dt).unwrap();
        assert_eq!(truncated.to_rfc3339(), "2026-09-24T12:00:00.001234Z");
    }

    #[test]
    fn rejects_leap_seconds() {
        let leap = utc(2016, 12, 31, 23, 59, 59)
            .with_nanosecond(1_000_000_000)
            .unwrap();
        assert_eq!(
            Timestamp::from_datetime(leap),
            Err(TimestampError::LeapSecond)
        );
        assert!(Timestamp::parse("2016-12-31T23:59:60Z").is_err());
    }

    #[test]
    fn offset_input_is_normalized_to_utc_by_the_caller() {
        let jakarta = DateTime::parse_from_rfc3339("2026-09-24T19:00:00+07:00").unwrap();
        let ts = Timestamp::from_datetime(jakarta.with_timezone(&Utc)).unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-09-24T12:00:00Z");
    }

    #[test]
    fn unix_micros_round_trip() {
        let ts = Timestamp::from_unix_micros(1_790_208_000_012_345).unwrap();
        assert_eq!(ts.unix_micros(), 1_790_208_000_012_345);
        assert_eq!(ts.to_rfc3339(), "2026-09-24T00:00:00.012345Z");
    }

    #[test]
    fn now_has_microsecond_precision() {
        assert_eq!(Timestamp::now().as_datetime().nanosecond() % 1_000, 0);
    }

    #[test]
    fn orders_chronologically() {
        let earlier = Timestamp::parse("2026-09-24T12:00:00Z").unwrap();
        let later = Timestamp::parse("2026-09-24T12:00:00.012Z").unwrap();
        assert!(earlier < later);
    }
}
