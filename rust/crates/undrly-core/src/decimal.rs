//! Decimal rules for exact financial values.
//!
//! Exact financial values use [`Decimal`], never `f32`/`f64`. As text they use
//! canonical form: the exact output of `Decimal::to_string`, which preserves
//! scale (`"183.4200"` stays `"183.4200"`). Parsing rejects anything that would
//! not round-trip to the same text: exponents, leading zeros, `+` signs,
//! whitespace, negative zero, and values that would need rounding to fit.
//! In PostgreSQL they are unconstrained `numeric`, which also preserves scale.

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    #[error("`{0}` is not a decimal representable without rounding")]
    Invalid(String),
    #[error("`{0}` is not in canonical decimal form")]
    NonCanonical(String),
}

/// Parses a decimal string, requiring canonical form.
pub fn parse_canonical(s: &str) -> Result<Decimal, DecimalError> {
    let value = Decimal::from_str_exact(s).map_err(|_| DecimalError::Invalid(s.to_owned()))?;
    let negative_zero = value.is_zero() && value.is_sign_negative();
    if negative_zero || value.to_string() != s {
        return Err(DecimalError::NonCanonical(s.to_owned()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_scale() {
        let value = parse_canonical("183.4200").unwrap();
        assert_eq!(value.scale(), 4);
        assert_eq!(value.to_string(), "183.4200");
        assert_ne!(value.to_string(), "183.42");
    }

    #[test]
    fn round_trips_exactly() {
        for text in [
            "0",
            "0.00",
            "1",
            "-1.50",
            "183.4200",
            "0.00000001",
            "79228162514264337593543950335",
            "0.0000000000000000000000000001",
        ] {
            assert_eq!(parse_canonical(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn rejects_non_canonical_text() {
        for text in [
            "", " 1", "1 ", "+1", "01", "1.", ".5", "1e3", "1E3", "-0", "-0.00", "1,000", "NaN",
        ] {
            assert!(parse_canonical(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn rejects_values_that_need_rounding() {
        // More fractional digits than Decimal can hold.
        assert!(parse_canonical("0.00000000000000000000000000001").is_err());
        // Larger than the 96-bit mantissa.
        assert!(parse_canonical("79228162514264337593543950336").is_err());
    }
}
