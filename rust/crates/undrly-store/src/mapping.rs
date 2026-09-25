//! Conversions between core domain types and PostgreSQL representations.
//!
//! | Core | PostgreSQL |
//! | --- | --- |
//! | typed ids / `CanonicalId` | `uuid` (+ `category` text from `Category::as_str`) |
//! | `Timestamp` | `timestamptz` (both microsecond precision) |
//! | `Decimal` | `financial_decimal` (`numeric`, scale preserved), transferred as canonical text |
//! | `Validity` | `validity` (`tstzrange`, half-open) |
//! | enums | `text` constrained by `CHECK` or rule tables, using core `as_str` names |

use std::ops::Bound;

use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgRange;
use undrly_core::decimal::{self, DecimalError};
use undrly_core::identifier::IdentifierError;
use undrly_core::time::TimestampError;
use undrly_core::{
    CanonicalId, Category, Cik, CurrencyCode, Decimal, EntityKind, ExternalIdentifier, Figi,
    IdError, InstrumentClass, Isin, Lei, Mic, Namespace, Redistribution, Timestamp, Validity,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    #[error("stored validity range is not half-open [from, until)")]
    NotHalfOpen,
    #[error("stored validity range is empty")]
    EmptyValidity,
    #[error(transparent)]
    Timestamp(#[from] TimestampError),
    #[error(transparent)]
    Decimal(#[from] DecimalError),
    #[error(transparent)]
    Id(#[from] IdError),
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    #[error("unknown {what} `{value}` in storage")]
    UnknownName { what: &'static str, value: String },
}

pub fn timestamp_from_sql(value: DateTime<Utc>) -> Result<Timestamp, MappingError> {
    Ok(Timestamp::from_datetime(value)?)
}

pub fn canonical_id_from_sql(
    uuid: sqlx::types::Uuid,
    category: &str,
) -> Result<CanonicalId, MappingError> {
    let category: Category = category.parse()?;
    Ok(CanonicalId::from_parts(category, uuid)?)
}

pub fn redistribution_to_sql(value: Redistribution) -> &'static str {
    match value {
        Redistribution::Permitted => "permitted",
        Redistribution::Restricted => "restricted",
        Redistribution::Unknown => "unknown",
    }
}

pub fn redistribution_from_sql(value: &str) -> Result<Redistribution, MappingError> {
    [
        Redistribution::Permitted,
        Redistribution::Restricted,
        Redistribution::Unknown,
    ]
    .into_iter()
    .find(|r| redistribution_to_sql(*r) == value)
    .ok_or_else(|| unknown("redistribution", value))
}

pub fn entity_kind_from_sql(value: &str) -> Result<EntityKind, MappingError> {
    EntityKind::ALL
        .into_iter()
        .find(|k| k.as_str() == value)
        .ok_or_else(|| unknown("entity kind", value))
}

pub fn instrument_class_from_sql(value: &str) -> Result<InstrumentClass, MappingError> {
    InstrumentClass::ALL
        .into_iter()
        .find(|c| c.as_str() == value)
        .ok_or_else(|| unknown("instrument class", value))
}

/// Rebuilds a validated external identifier from `(scheme, value)` columns.
/// Values are re-validated with the namespace's own rules.
pub fn external_identifier_from_sql(
    scheme: &str,
    value: &str,
) -> Result<ExternalIdentifier, MappingError> {
    let namespace = Namespace::ALL
        .into_iter()
        .find(|ns| ns.as_str() == scheme)
        .ok_or_else(|| unknown("identifier namespace", scheme))?;
    Ok(match namespace {
        Namespace::Isin => ExternalIdentifier::Isin(Isin::parse(value)?),
        Namespace::Figi => ExternalIdentifier::Figi(Figi::parse(value)?),
        Namespace::Lei => ExternalIdentifier::Lei(Lei::parse(value)?),
        Namespace::Mic => ExternalIdentifier::Mic(Mic::parse(value)?),
        Namespace::Iso4217 => ExternalIdentifier::Iso4217(CurrencyCode::parse(value)?),
        Namespace::Cik => ExternalIdentifier::Cik(Cik::parse(value)?),
    })
}

fn unknown(what: &'static str, value: &str) -> MappingError {
    MappingError::UnknownName {
        what,
        value: value.to_owned(),
    }
}

/// Financial decimals cross the PostgreSQL boundary as canonical text, never
/// through sqlx's native `rust_decimal` codec: that codec drops the scale of
/// zero in both directions (`0.00` is written and read back as `0`).
///
/// Write: bind this string and cast in SQL, e.g. `VALUES ($1::numeric)`.
pub fn decimal_to_sql(value: Decimal) -> String {
    value.to_string()
}

/// Read: select the column as text, e.g. `SELECT price::text`, then parse.
/// PostgreSQL's numeric output is the canonical `Decimal` text.
pub fn decimal_from_sql(text: &str) -> Result<Decimal, MappingError> {
    Ok(decimal::parse_canonical(text)?)
}

pub fn validity_to_range(validity: Validity) -> PgRange<DateTime<Utc>> {
    PgRange {
        start: validity
            .from()
            .map_or(Bound::Unbounded, |t| Bound::Included(t.as_datetime())),
        end: validity
            .until()
            .map_or(Bound::Unbounded, |t| Bound::Excluded(t.as_datetime())),
    }
}

pub fn validity_from_range(range: PgRange<DateTime<Utc>>) -> Result<Validity, MappingError> {
    let from = match range.start {
        Bound::Unbounded => None,
        Bound::Included(t) => Some(Timestamp::from_datetime(t)?),
        Bound::Excluded(_) => return Err(MappingError::NotHalfOpen),
    };
    let until = match range.end {
        Bound::Unbounded => None,
        Bound::Excluded(t) => Some(Timestamp::from_datetime(t)?),
        Bound::Included(_) => return Err(MappingError::NotHalfOpen),
    };
    Validity::new(from, until).map_err(|_| MappingError::EmptyValidity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_round_trips() {
        let t = |s: &str| Some(Timestamp::parse(s).unwrap());
        for validity in [
            Validity::UNBOUNDED,
            Validity::new(t("2020-01-01T00:00:00Z"), None).unwrap(),
            Validity::new(None, t("2020-01-01T00:00:00.000001Z")).unwrap(),
            Validity::new(t("2020-01-01T00:00:00Z"), t("2021-01-01T00:00:00Z")).unwrap(),
        ] {
            assert_eq!(
                validity_from_range(validity_to_range(validity)),
                Ok(validity)
            );
        }
    }

    #[test]
    fn decimal_text_round_trips_with_scale() {
        for text in [
            "0.00",
            "183.4200",
            "-37.63",
            "79228162514264337593543950335",
        ] {
            let value = decimal_from_sql(text).unwrap();
            assert_eq!(decimal_to_sql(value), text);
        }
        assert!(decimal_from_sql("NaN").is_err());
    }

    #[test]
    fn rejects_closed_upper_bound() {
        let t = Timestamp::parse("2020-01-01T00:00:00Z")
            .unwrap()
            .as_datetime();
        let range = PgRange {
            start: Bound::Unbounded,
            end: Bound::Included(t),
        };
        assert_eq!(validity_from_range(range), Err(MappingError::NotHalfOpen));
    }
}
