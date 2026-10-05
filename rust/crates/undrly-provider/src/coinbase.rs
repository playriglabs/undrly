//! Coinbase Exchange public order book, level 1
//! (`GET /products/{product}/book?level=1`): best bid and ask with the
//! book's timestamp. No authentication.
//!
//! The response does not name its product, so one request covers exactly one
//! product; the normalizer attributes it to the single requested symbol.
//!
//! Bars (V1.10, `history bars`): `GET /products/{product}/candles?granularity=`
//! (3600 or 86400 s), at most 300 candles, newest first, each
//! `[time, low, high, open, close, volume]` with JSON numbers.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, JsonNumber, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "coinbase";
pub const BASE_URL: &str = "https://api.exchange.coinbase.com/products";

/// One product of `GET /products`. Universe building only.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Product {
    pub id: String,
    pub base_currency: String,
    pub quote_currency: String,
    pub status: String,
    #[serde(default)]
    pub trading_disabled: bool,
}

pub fn decode_products(payload: &[u8]) -> Result<Vec<Product>, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
}

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

    /// The newest 300 candles of `granularity` seconds (3600 or 86400).
    pub fn candles_url(product: &str, granularity: u32) -> String {
        format!("{BASE_URL}/{product}/candles?granularity={granularity}")
    }
}

/// One candle: start (Unix seconds), then prices and base-asset volume as
/// written in the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candle {
    pub time: i64,
    pub low: JsonNumber,
    pub high: JsonNumber,
    pub open: JsonNumber,
    pub close: JsonNumber,
    pub volume: JsonNumber,
}

/// One product's candles, newest first as served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candles(pub Vec<Candle>);

pub fn decode_candles(payload: &[u8]) -> Result<Candles, String> {
    let rows: Vec<(
        i64,
        JsonNumber,
        JsonNumber,
        JsonNumber,
        JsonNumber,
        JsonNumber,
    )> = serde_json::from_slice(payload).map_err(|e| format!("candles: {e}"))?;
    Ok(Candles(
        rows.into_iter()
            .map(|(time, low, high, open, close, volume)| Candle {
                time,
                low,
                high,
                open,
                close,
                volume,
            })
            .collect(),
    ))
}

crate::bars_provider!(CoinbaseProvider, Candles, decode_candles);

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

/// Fetches the newest candles of `product` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_candles(
    client: &crate::http::HttpClient,
    product: &str,
    granularity: u32,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&CoinbaseProvider::candles_url(product, granularity), &[])
        .await
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
