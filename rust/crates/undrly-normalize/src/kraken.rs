//! Normalizer for [`undrly_provider::kraken`] tickers.
//!
//! Price is the last trade (`c[0]`), with best bid (`b[0]`) and ask (`a[0]`).
//! Decimal strings are kept exactly (scale preserved). Kraken states no time,
//! so `observed_at` is `None`.

use undrly_core::{BidAsk, PriceType, VenueSymbol};
use undrly_provider::kraken::Ticker;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid};

pub struct KrakenNormalizer;

impl QuoteNormalizer for KrakenNormalizer {
    type Quote = Ticker;

    fn normalize_quotes(
        &self,
        ticker: &Ticker,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(tick) = ticker.result.get(symbol.as_str()) else {
                continue;
            };
            let first = |field: &'static str, v: &[String]| {
                v.first()
                    .ok_or_else(|| invalid(field, "missing price"))
                    .and_then(|p| decimal(field, p))
            };
            out.push(NormalizedQuote {
                symbol: symbol.clone(),
                price_type: PriceType::Last,
                price: first("c", &tick.c)?,
                bid_ask: Some(BidAsk {
                    bid: first("b", &tick.b)?,
                    ask: first("a", &tick.a)?,
                }),
                observed_at: None,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use undrly_provider::QuoteProvider;
    use undrly_provider::kraken::KrakenProvider;

    use super::*;

    #[test]
    fn normalizes_requested_pairs_with_scale() {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/kraken/ticker.json"),
        )
        .unwrap();
        let ticker = KrakenProvider::new().decode_quote(&payload).unwrap();
        let sym = |s| VenueSymbol::new(s).unwrap();
        let quotes = KrakenNormalizer
            .normalize_quotes(&ticker, &[sym("ZEURZUSD"), sym("NOTREQUESTED")])
            .unwrap();
        assert_eq!(quotes.len(), 1);
        let eur = &quotes[0];
        assert_eq!(eur.price.to_string(), "1.13680");
        let ba = eur.bid_ask.unwrap();
        assert_eq!(
            (ba.bid.to_string(), ba.ask.to_string()),
            ("1.13679".into(), "1.13680".into())
        );
        assert_eq!(eur.observed_at, None);
    }
}
