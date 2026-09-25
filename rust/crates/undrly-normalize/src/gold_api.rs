//! Normalizer for [`undrly_provider::gold_api`] prices.
//!
//! A **reference** price (aggregated, no venue) with the source's update time.
//! The feed's unit is USD, so a response stating another currency is rejected
//! rather than mislabeled.

use undrly_core::{PriceType, VenueSymbol};
use undrly_provider::gold_api::Price;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, timestamp};

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
}
