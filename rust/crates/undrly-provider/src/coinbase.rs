//! Coinbase Exchange public order book, level 1
//! (`GET /products/{product}/book?level=1`): best bid and ask with the
//! book's timestamp. No authentication.
//!
//! The response does not name its product, so one request covers exactly one
//! product; the normalizer attributes it to the single requested symbol.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "coinbase";
pub const BASE_URL: &str = "https://api.exchange.coinbase.com/products";

/// Level-1 book: at most one `[price, size, order count]` per side, prices
/// and sizes as decimal strings, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Book {
    pub bids: Vec<(String, String, u64)>,
    pub asks: Vec<(String, String, u64)>,
    /// Book time, RFC 3339 with nanoseconds.
    pub time: String,
}

pub struct CoinbaseProvider {
    source_id: SourceId,
}

impl CoinbaseProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    pub fn book_url(product: &str) -> String {
        format!("{BASE_URL}/{product}/book?level=1")
    }
}

impl Default for CoinbaseProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CoinbaseProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for CoinbaseProvider {
    type Quote = Book;

    fn decode_quote(&self, payload: &[u8]) -> Result<Book, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// Fetches the level-1 book of `product` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_book(
    client: &crate::http::HttpClient,
    product: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&CoinbaseProvider::book_url(product), &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_captured_book_verbatim() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/coinbase/book-BTC-USD-level1.json"),
        )
        .unwrap();
        let b = CoinbaseProvider::new().decode_quote(&payload).unwrap();
        assert_eq!(b.bids[0].0, "84006.45");
        assert_eq!(b.asks[0].0, "84006.46");
        assert_eq!(b.time, "2026-09-25T07:28:06.386017046Z");
        assert_eq!(
            CoinbaseProvider::book_url("BTC-USD"),
            "https://api.exchange.coinbase.com/products/BTC-USD/book?level=1"
        );
    }

    #[test]
    fn rejects_error_and_malformed_payloads() {
        for payload in [
            &br#"{"message":"NotFound"}"#[..],
            b"<html>",
            br#"{"bids":[[84006.45,"1",1]],"asks":[],"time":"x"}"#,
        ] {
            assert!(CoinbaseProvider::new().decode_quote(payload).is_err());
        }
    }
}
