//! Normalizer for [`undrly_provider::coinbase`] level-1 books.
//!
//! One observation per book: price type **mid**, `price = (bid + ask) / 2`
//! (exact, see [`undrly_core::quote::mid_price`]), with the best bid and ask
//! and the book time as source time. A book is attributed to the single
//! requested symbol; the payload itself names no product, so more than one
//! requested symbol is an error rather than a guess. An empty side is an
//! error (no mid exists).

use undrly_core::quote::mid_price;
use undrly_core::{BidAsk, PriceType, VenueSymbol};
use undrly_provider::coinbase::Book;

use crate::{NormalizeError, NormalizedQuote, QuoteNormalizer, decimal, invalid, timestamp};

pub struct CoinbaseNormalizer;

impl QuoteNormalizer for CoinbaseNormalizer {
    type Quote = Book;

    fn normalize_quotes(
        &self,
        book: &Book,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError> {
        let symbol = match symbols {
            [] => return Ok(Vec::new()),
            [one] => one,
            _ => {
                return Err(invalid(
                    "product",
                    "a book names no product; request one product per record",
                ));
            }
        };
        let (Some(bid), Some(ask)) = (book.bids.first(), book.asks.first()) else {
            return Err(invalid("bids/asks", "empty side: no mid price"));
        };
        let (bid, ask) = (decimal("bids", &bid.0)?, decimal("asks", &ask.0)?);
        Ok(vec![NormalizedQuote {
            symbol: symbol.clone(),
            price_type: PriceType::Mid,
            price: mid_price(bid, ask).ok_or_else(|| invalid("bids/asks", "overflow"))?,
            bid_ask: Some(BidAsk { bid, ask }),
            observed_at: Some(timestamp("time", &book.time)?),
        }])
    }
}

#[cfg(test)]
mod tests {
    use undrly_provider::QuoteProvider;
    use undrly_provider::coinbase::CoinbaseProvider;

    use super::*;

    #[test]
    fn normalizes_book_to_exact_mid_with_book_time() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/coinbase/book-BTC-USD-level1.json"),
        )
        .unwrap();
        let book = CoinbaseProvider::new().decode_quote(&payload).unwrap();
        let btc = VenueSymbol::new("BTC-USD").unwrap();
        let q = CoinbaseNormalizer
            .normalize_quotes(&book, std::slice::from_ref(&btc))
            .unwrap();
        assert_eq!(q[0].price_type, PriceType::Mid);
        assert_eq!(q[0].price.to_string(), "84006.455");
        let ba = q[0].bid_ask.unwrap();
        assert_eq!(
            (ba.bid.to_string(), ba.ask.to_string()),
            ("84006.45".into(), "84006.46".into())
        );
        assert_eq!(
            q[0].observed_at.unwrap().to_string(),
            "2026-09-25T07:28:06.386017Z"
        );
        let eth = VenueSymbol::new("ETH-USD").unwrap();
        assert!(
            CoinbaseNormalizer
                .normalize_quotes(&book, &[btc, eth])
                .is_err()
        );
    }

    #[test]
    fn rejects_an_empty_side() {
        let book = Book {
            bids: vec![],
            asks: vec![("1.0".into(), "1".into(), 1)],
            time: "2026-09-25T07:28:06Z".into(),
        };
        assert!(
            CoinbaseNormalizer
                .normalize_quotes(&book, &[VenueSymbol::new("BTC-USD").unwrap()])
                .is_err()
        );
    }
}
