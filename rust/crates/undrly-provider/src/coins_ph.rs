//! Coins.ph (Coins Pro) public market data (V1.9), for its PHP books
//! (USDT/PHP, USDC/PHP, PYUSD/PHP). No authentication.
//!
//! Binance-compatible layouts: `GET /openapi/quote/v1/ticker/bookTicker`
//! (one market per request, named by symbol, no time) and
//! `GET /openapi/quote/v1/klines` (arrays, the payload names no market).

use crate::binance::{BookTickers, Klines, decode_book_tickers, decode_klines};

pub const SOURCE_ID: &str = "coins-ph";
pub const BASE_URL: &str = "https://api.pro.coins.ph";

crate::quote_provider!(CoinsPhProvider, BookTickers, decode_book_tickers);
crate::bars_provider!(CoinsPhProvider, Klines, decode_klines);

impl CoinsPhProvider {
    pub fn book_ticker_url(symbol: &str) -> String {
        format!("{BASE_URL}/openapi/quote/v1/ticker/bookTicker?symbol={symbol}")
    }

    /// `interval` is the venue's own (`1h`, `1d`).
    pub fn klines_url(symbol: &str, interval: &str, start_ms: i64, limit: u32) -> String {
        format!(
            "{BASE_URL}/openapi/quote/v1/klines?symbol={symbol}&interval={interval}&startTime={start_ms}&limit={limit}"
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
        .get(&CoinsPhProvider::book_ticker_url(symbol), &[])
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
            &CoinsPhProvider::klines_url(symbol, interval, start_ms, limit),
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
        let t = CoinsPhProvider::new()
            .decode_quote(&crate::fixture("coins-ph/book-ticker-USDTPHP.json"))
            .unwrap();
        assert_eq!(t.0.len(), 1);
        assert_eq!(t.0[0].symbol, "USDTPHP");
        let k = CoinsPhProvider::new()
            .decode_bars(&crate::fixture("coins-ph/klines-USDTPHP-1h.json"))
            .unwrap();
        assert_eq!(k.0.len(), 3);
        assert_eq!(
            CoinsPhProvider::book_ticker_url("USDTPHP"),
            "https://api.pro.coins.ph/openapi/quote/v1/ticker/bookTicker?symbol=USDTPHP"
        );
    }
}
