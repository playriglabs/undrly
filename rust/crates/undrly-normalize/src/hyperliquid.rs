//! Normalizer for [`undrly_provider::hyperliquid`] asset contexts.
//!
//! Price is the perpetual's **mark price** (`markPx`), in the contract's
//! price denomination, which the feed declares as its unit (Tether USD for
//! USDT-denominated perpetuals, USD Coin for the USDC-denominated ones). The
//! margin asset is not the price unit. No bid/ask, no source time.

use undrly_core::quote::mid_price;
use undrly_core::{BidAsk, PriceType, Timestamp, VenueSymbol};
use undrly_provider::hyperliquid::{L2Book, MetaAndAssetCtxs};

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

/// Normalizer for [`undrly_provider::hyperliquid::L2Book`]: one observation
/// per book, price type **mid** (`(best bid + best ask) / 2`, exact), with
/// the best bid and ask and the book's own time. The book names its coin,
/// which must be the one requested. An empty side yields no observation
/// (no mid exists); the feed is then reported missing.
pub struct HyperliquidBookNormalizer;

impl QuoteNormalizer for HyperliquidBookNormalizer {
    type Quote = L2Book;

    fn normalize_quotes(
        &self,
        book: &L2Book,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let Some(symbol) = symbols.iter().find(|s| s.as_str() == book.coin) else {
            return Ok(Vec::new());
        };
        let (Some(bid), Some(ask)) = (book.levels.0.first(), book.levels.1.first()) else {
            return Ok(Vec::new());
        };
        let (bid, ask) = (
            decimal("levels[0].px", &bid.px)?,
            decimal("levels[1].px", &ask.px)?,
        );
        if bid > ask {
            return Err(invalid(
                "levels",
                format!("{symbol}: crossed book {bid} > {ask}"),
            ));
        }
        let time = chrono::DateTime::from_timestamp_millis(book.time)
            .ok_or_else(|| invalid("time", "out of range"))?;
        Ok(vec![NormalizedQuote {
            symbol: symbol.clone(),
            price_type: PriceType::Mid,
            price: mid_price(bid, ask).ok_or_else(|| invalid("levels", "overflow"))?,
            bid_ask: Some(BidAsk { bid, ask }),
            observed_at: Some(Timestamp::from_datetime(time).map_err(|e| invalid("time", e))?),
        }])
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

    #[test]
    fn normalizes_a_book_to_its_exact_mid_and_time() {
        use undrly_provider::hyperliquid::HyperliquidBookProvider;
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/hyperliquid/l2Book-BTC.json"),
        )
        .unwrap();
        let book = HyperliquidBookProvider::new()
            .decode_quote(&payload)
            .unwrap();
        let btc = VenueSymbol::new("BTC").unwrap();
        let q = HyperliquidBookNormalizer
            .normalize_quotes(&book, std::slice::from_ref(&btc))
            .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price_type, PriceType::Mid);
        assert_eq!(q[0].price.to_string(), "83520.50");
        let ba = q[0].bid_ask.unwrap();
        assert_eq!(
            (ba.bid.to_string(), ba.ask.to_string()),
            ("83520.0".into(), "83521.0".into())
        );
        assert_eq!(
            q[0].observed_at.unwrap(),
            Timestamp::parse("2026-09-28T13:31:46.444Z").unwrap()
        );
        // Another coin's book is never attributed to BTC.
        let eth = VenueSymbol::new("ETH").unwrap();
        assert!(
            HyperliquidBookNormalizer
                .normalize_quotes(&book, &[eth])
                .unwrap()
                .is_empty()
        );
    }
}
