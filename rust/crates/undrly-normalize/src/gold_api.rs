//! Normalizer for [`undrly_provider::gold_api`] prices.
//!
//! A **reference** price (aggregated, no venue) with the source's update time.
//! The feed's unit is USD, so a response stating another currency is rejected
//! rather than mislabeled.

use undrly_core::{Decimal, PriceType, Timestamp, VenueSymbol};
use undrly_provider::gold_api::{Ohlc, Price};

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid, timestamp};

/// A source's open/high/low/close over its own window (V1.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedWindow {
    pub start: Timestamp,
    pub end: Timestamp,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
}

fn unix(field: &'static str, s: i64) -> Result<Timestamp, NormalizeError> {
    let micros = s
        .checked_mul(1_000_000)
        .ok_or_else(|| invalid(field, "out of range"))?;
    Timestamp::from_unix_micros(micros).map_err(|e| invalid(field, e))
}

/// gold-api `/ohlc` → its window, exactly as stated. A window that ends
/// before it starts or values out of order are errors, never repaired.
pub fn normalize_window(o: &Ohlc) -> Result<NormalizedWindow, NormalizeError> {
    let w = NormalizedWindow {
        start: unix("startTimestamp", o.start_timestamp)?,
        end: unix("endTimestamp", o.end_timestamp)?,
        open: decimal("open", &o.open.0)?,
        high: decimal("high", &o.high.0)?,
        low: decimal("low", &o.low.0)?,
        close: decimal("close", &o.close.0)?,
    };
    if w.end <= w.start {
        return Err(invalid("endTimestamp", "the window ends before it starts"));
    }
    let (o, h, l, c) = (w.open, w.high, w.low, w.close);
    if !(l <= h && l <= o && o <= h && l <= c && c <= h) {
        return Err(invalid("ohlc", "low <= open, close <= high"));
    }
    Ok(w)
}

pub struct GoldApiNormalizer;

impl QuoteNormalizer for GoldApiNormalizer {
    type Quote = Price;

    fn normalize_quotes(
        &self,
        p: &Price,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        if !symbols.iter().any(|s| s.as_str() == p.symbol) {
            return Ok(Vec::new());
        }
        if p.currency != "USD" {
            return Err(NormalizeError::Unsupported {
                field: "currency",
                value: p.currency.clone(),
            });
        }
        Ok(vec![NormalizedQuote {
            symbol: VenueSymbol::new(&p.symbol).map_err(|e| crate::invalid("symbol", e))?,
            price_type: PriceType::Reference,
            price: decimal("price", &p.price.0)?,
            bid_ask: None,
            observed_at: Some(timestamp("updatedAt", &p.updated_at)?),
        }])
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::JsonNumber;

    use super::*;

    fn price(currency: &str) -> Price {
        Price {
            symbol: "XAU".into(),
            currency: currency.into(),
            price: JsonNumber("4273.600098".into()),
            updated_at: "2026-09-25T03:51:57Z".into(),
        }
    }

    #[test]
    fn normalizes_reference_price_with_source_time() {
        let xau = VenueSymbol::new("XAU").unwrap();
        let q = GoldApiNormalizer
            .normalize_quotes(&price("USD"), std::slice::from_ref(&xau))
            .unwrap();
        assert_eq!(q[0].price.to_string(), "4273.600098");
        assert_eq!(q[0].price_type, PriceType::Reference);
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-25T03:51:57Z"
        );
        assert!(
            GoldApiNormalizer
                .normalize_quotes(&price("EUR"), &[xau])
                .is_err()
        );
        let mut exponent = price("USD");
        exponent.price = JsonNumber("4.2736e3".into());
        assert!(
            GoldApiNormalizer
                .normalize_quotes(&exponent, &[VenueSymbol::new("XAU").unwrap()])
                .is_err()
        );
    }

    #[test]
    fn ohlc_window_as_stated() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/gold-api/ohlc-XAU-24h.json"),
        )
        .unwrap();
        let o = undrly_provider::gold_api::GoldApiProvider::new()
            .decode_ohlc(&payload)
            .unwrap();
        let w = normalize_window(&o).unwrap();
        assert_eq!(w.high.to_string(), "4219.0");
        assert_eq!(
            (w.end.as_datetime() - w.start.as_datetime()).num_hours(),
            24
        );
        let bad = Ohlc {
            high: o.low.clone(),
            ..o
        };
        assert!(normalize_window(&bad).is_err());
    }
}
