//! Normalizer for [`undrly_provider::hyperliquid`] asset contexts.
//!
//! Price is the perpetual's **mark price** (`markPx`), in the market's quote
//! asset (USDC; the unit comes from the feed). No bid/ask, no source time.

use undrly_core::{PriceType, VenueSymbol};
use undrly_provider::hyperliquid::MetaAndAssetCtxs;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid};

pub struct HyperliquidNormalizer;

impl QuoteNormalizer for HyperliquidNormalizer {
    type Quote = MetaAndAssetCtxs;

    fn normalize_quotes(
        &self,
        d: &MetaAndAssetCtxs,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let mut out = Vec::new();
        for symbol in symbols {
            let Some(ctx) = d.context(symbol.as_str()) else {
                continue;
            };
            let mark = ctx
                .mark_px
                .as_deref()
                .ok_or_else(|| invalid("markPx", format!("{symbol} has no mark price")))?;
            out.push(NormalizedQuote {
                symbol: symbol.clone(),
                price_type: PriceType::Mark,
                price: decimal("markPx", mark)?,
                bid_ask: None,
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
    use undrly_provider::hyperliquid::HyperliquidProvider;

    use super::*;

    #[test]
    fn normalizes_btc_mark_price_only() {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/hyperliquid/metaAndAssetCtxs.json"),
        )
        .unwrap();
        let d = HyperliquidProvider::new().decode_quote(&payload).unwrap();
        let btc = VenueSymbol::new("BTC").unwrap();
        let q = HyperliquidNormalizer.normalize_quotes(&d, &[btc]).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price_type, PriceType::Mark);
        assert_eq!(
            q[0].price.to_string(),
            d.context("BTC").unwrap().mark_px.clone().unwrap()
        );
    }
}
