//! HashKey Exchange (Hong Kong) public market data (V1.9), for its HKD book
//! (USDT/HKD). No authentication.
//!
//! - `GET /quote/v1/ticker/bookTicker?symbol=`: `[{s, b, a, t}]`, the
//!   market named, best bid `b` and ask `a`, time `t` in Unix milliseconds.
//! - `GET /quote/v1/klines`: Binance-compatible arrays (the payload names no
//!   market), decoded with [`crate::binance::decode_klines`].

use serde::Deserialize;

use crate::binance::{Klines, decode_klines};

pub const SOURCE_ID: &str = "hashkey";
pub const BASE_URL: &str = "https://api-pro.hashkey.com";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookTicker {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid: String,
    #[serde(rename = "a")]
    pub ask: String,
    /// Unix milliseconds.
    #[serde(rename = "t")]
    pub time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookTickers(pub Vec<BookTicker>);

fn decode(payload: &[u8]) -> Result<BookTickers, String> {
    serde_json::from_slice(payload)
        .map(BookTickers)
        .map_err(|e| e.to_string())
}

crate::quote_provider!(HashKeyProvider, BookTickers, decode);
crate::bars_provider!(HashKeyProvider, Klines, decode_klines);

impl HashKeyProvider {
    pub fn book_ticker_url(symbol: &str) -> String {
        format!("{BASE_URL}/quote/v1/ticker/bookTicker?symbol={symbol}")
    }

    /// `interval` is the venue's own (`1h`, `1d`).
    pub fn klines_url(symbol: &str, interval: &str, start_ms: i64, limit: u32) -> String {
        format!(
            "{BASE_URL}/quote/v1/klines?symbol={symbol}&interval={interval}&startTime={start_ms}&limit={limit}"
        )
    }
}

/// Fetches one market's best bid and ask (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_book_ticker(
    client: &crate::http::HttpClient,
    symbol: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&HashKeyProvider::book_ticker_url(symbol), &[])
        .await
}

/// Fetches up to `limit` klines from `start_ms` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_klines(
    client: &crate::http::HttpClient,
    symbol: &str,
    interval: &str,
    start_ms: i64,
    limit: u32,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(
            &HashKeyProvider::klines_url(symbol, interval, start_ms, limit),
            &[],
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarsProvider, QuoteProvider};

    #[test]
    fn decodes_captured_book_ticker_and_klines() {
        let t = HashKeyProvider::new()
            .decode_quote(&crate::fixture("hashkey/book-ticker-USDTHKD.json"))
            .unwrap();
        assert_eq!(t.0[0].symbol, "USDTHKD");
        assert!(t.0[0].time > 1_700_000_000_000);
        let k = HashKeyProvider::new()
            .decode_bars(&crate::fixture("hashkey/klines-USDTHKD-1h.json"))
            .unwrap();
        assert_eq!(k.0.len(), 3);
        assert!(
            HashKeyProvider::new()
                .decode_quote(b"[{\"s\":\"X\"}]")
                .is_err()
        );
    }
}
