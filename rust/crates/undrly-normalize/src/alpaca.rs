//! Normalizer for [`undrly_provider::alpaca`] IEX snapshots.
//!
//! Price is IEX's **last trade** with its trade time. A trade that did not
//! execute on IEX (exchange code other than `V`) is rejected: the feed is
//! declared as an IEX venue quote and must not silently become anything else.

use undrly_core::{PriceType, VenueSymbol};
use undrly_provider::alpaca::{IEX_EXCHANGE_CODE, Snapshots};

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, timestamp};

pub struct AlpacaNormalizer;

impl QuoteNormalizer for AlpacaNormalizer {
    type Quote = Snapshots;

    fn normalize_quotes(
        &self,
        snapshots: &Snapshots,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(trade) = snapshots
                .get(symbol.as_str())
                .and_then(|s| s.latest_trade.as_ref())
            else {
                continue;
            };
            if trade.x != IEX_EXCHANGE_CODE {
                return Err(NormalizeError::Unsupported {
                    field: "latestTrade.x",
                    value: trade.x.clone(),
                });
            }
            out.push(NormalizedQuote {
                symbol: symbol.clone(),
                price_type: PriceType::Last,
                price: decimal("latestTrade.p", &trade.p.0)?,
                bid_ask: None,
                observed_at: Some(timestamp("latestTrade.t", &trade.t)?),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;
    use undrly_provider::alpaca::AlpacaProvider;

    use super::*;

    #[test]
    fn normalizes_iex_last_trade_with_truncated_time() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/alpaca/snapshots-NVDA.json"),
        )
        .unwrap();
        let s = AlpacaProvider::new().decode_quote(&payload).unwrap();
        let q = AlpacaNormalizer
            .normalize_quotes(&s, &[VenueSymbol::new("NVDA").unwrap()])
            .unwrap();
        assert_eq!(q[0].price.to_string(), "223.71");
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-24T20:45:15.183009Z"
        );
    }

    #[test]
    fn rejects_trades_from_other_exchanges() {
        // Synthetic: a non-IEX exchange code (`Q`, Nasdaq).
        let payload =
            br#"{"NVDA":{"latestTrade":{"t":"2026-09-24T19:59:59Z","x":"Q","p":224.58}}}"#;
        let s = AlpacaProvider::new().decode_quote(payload).unwrap();
        assert!(matches!(
            AlpacaNormalizer.normalize_quotes(&s, &[VenueSymbol::new("NVDA").unwrap()]),
            Err(NormalizeError::Unsupported { .. })
        ));
    }
}
