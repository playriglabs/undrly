//! Normalizer for [`undrly_provider::eia`] daily spot prices.
//!
//! A **reference** price per series: the newest period with a value. The
//! observation time is the price's date at 00:00 UTC (EIA states a date,
//! not a time). Each series must be in its expected unit, or the record is
//! rejected rather than mislabeled.

use undrly_core::{PriceType, VenueSymbol};
use undrly_provider::eia::Response;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid, timestamp};

pub struct EiaNormalizer;

/// The unit EIA states for each supported series.
fn expected_unit(series: &str) -> Option<&'static str> {
    match series {
        "RWTC" | "RBRTE" => Some("$/BBL"),
        "RNGWHHD" => Some("$/MMBTU"),
        _ => None,
    }
}

impl QuoteNormalizer for EiaNormalizer {
    type Quote = Response;

    /// The newest period with a value, per series.
    fn normalize_quotes(
        &self,
        r: &Response,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        Ok(crate::newest_per_symbol(
            crate::HistoryNormalizer::normalize_history(self, r, symbols)?,
        ))
    }
}

impl crate::HistoryNormalizer for EiaNormalizer {
    /// Every period with a value, per series.
    fn normalize_history(
        &self,
        r: &Response,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(unit) = expected_unit(symbol.as_str()) else {
                return Err(NormalizeError::Unsupported {
                    field: "series",
                    value: symbol.as_str().to_owned(),
                });
            };
            for point in r
                .response
                .data
                .iter()
                .filter(|p| p.series == symbol.as_str() && p.value.is_some())
            {
                if point.units.as_deref() != Some(unit) {
                    return Err(invalid(
                        "units",
                        format!("{} is in {:?}, expected {unit}", point.series, point.units),
                    ));
                }
                let value = &point.value.as_ref().expect("filtered").0;
                out.push(NormalizedQuote {
                    symbol: symbol.clone(),
                    price_type: PriceType::Reference,
                    price: decimal("value", value)?,
                    bid_ask: None,
                    observed_at: Some(timestamp("period", &format!("{}T00:00:00Z", point.period))?),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;
    use undrly_provider::eia::EiaProvider;

    use super::*;

    fn response(units: &str) -> Response {
        EiaProvider::new()
            .decode_quote(
                format!(
                    r#"{{"response":{{"data":[
                    {{"period":"2026-09-14","series":"RWTC","value":64.90,"units":"$/BBL"}},
                    {{"period":"2026-09-15","series":"RWTC","value":"65.12","units":"{units}"}},
                    {{"period":"2026-09-16","series":"RWTC","value":null,"units":"$/BBL"}}]}}}}"#
                )
                .as_bytes(),
            )
            .unwrap()
    }

    #[test]
    fn newest_value_with_its_date() {
        let wti = VenueSymbol::new("RWTC").unwrap();
        let q = EiaNormalizer
            .normalize_quotes(&response("$/BBL"), std::slice::from_ref(&wti))
            .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price.to_string(), "65.12");
        assert_eq!(q[0].price_type, PriceType::Reference);
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-15T00:00:00Z"
        );
        assert!(
            EiaNormalizer
                .normalize_quotes(&response("$/GAL"), std::slice::from_ref(&wti))
                .is_err()
        );
        // Brent requested but absent: not returned (ingestion reports it).
        let brent = VenueSymbol::new("RBRTE").unwrap();
        assert!(
            EiaNormalizer
                .normalize_quotes(&response("$/BBL"), &[brent])
                .unwrap()
                .is_empty()
        );
    }
}
