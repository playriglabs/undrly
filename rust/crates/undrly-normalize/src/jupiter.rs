//! Normalizer for [`undrly_provider::jupiter`] prices.
//!
//! One `last` price per requested mint, aggregated across Solana venues (the
//! feed declares no venue), with no bid/ask and no source time (Jupiter
//! states a block, not a time). A mint without `usdPrice`, or whose
//! `usdPrice` is not an exact decimal as written (an exponent), is not
//! returned; ingestion reports it missing. Nothing is rounded or repaired.

use undrly_core::{PriceType, VenueSymbol};
use undrly_provider::jupiter::Prices;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer};

pub struct JupiterNormalizer;

impl QuoteNormalizer for JupiterNormalizer {
    type Quote = Prices;

    fn normalize_quotes(
        &self,
        p: &Prices,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(text) = p.0.get(symbol.as_str()).and_then(|x| x.usd_price.as_ref()) else {
                continue;
            };
            let Ok(price) = undrly_core::decimal::parse_canonical(&text.0) else {
                continue;
            };
            if price.is_sign_negative() || price.is_zero() {
                continue;
            }
            out.push(NormalizedQuote {
                symbol: symbol.clone(),
                price_type: PriceType::Last,
                price,
                bid_ask: None,
                observed_at: None,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;
    use undrly_provider::jupiter::JupiterProvider;

    use super::*;

    #[test]
    fn prices_requested_mints_only_and_skips_inexact_numbers() {
        let p = JupiterProvider::new()
            .decode_quote(
                br#"{"MintA":{"usdPrice":332.47579840803337},"MintB":{"liquidity":1.0},
                     "MintC":{"usdPrice":1.5e-7},"MintD":{"usdPrice":9.5}}"#,
            )
            .unwrap();
        let sym = |s: &str| VenueSymbol::new(s).unwrap();
        let q = JupiterNormalizer
            .normalize_quotes(&p, &[sym("MintA"), sym("MintB"), sym("MintC")])
            .unwrap();
        assert_eq!(
            q.len(),
            1,
            "MintD not requested, MintB unpriced, MintC an exponent"
        );
        assert_eq!(q[0].symbol.as_str(), "MintA");
        assert_eq!(q[0].price.to_string(), "332.47579840803337");
        assert_eq!(q[0].price_type, PriceType::Last);
        assert_eq!(q[0].observed_at, None);
    }
}
