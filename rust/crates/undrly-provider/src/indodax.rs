//! Indodax public market data (V1.9), for its IDR books (USDT/IDR,
//! USDC/IDR). No authentication.
//!
//! - `GET /api/ticker/{pair}`: `{ticker: {buy, sell, server_time, ...}}` —
//!   `buy` is the best bid, `sell` the best ask, `server_time` Unix seconds.
//!   The payload names no market, so one request covers one market.
//! - `GET /tradingview/history_v2?symbol=&tf=&from=&to=`: `[{Time, Open,
//!   High, Low, Close, Volume}]`, prices as JSON **numbers** (kept as
//!   written, never through a float), `Time` in Unix seconds; no market
//!   named.
//!
//! Feed symbols are the ticker's lower-case pair (`usdtidr`); the chart
//! endpoint takes it upper-case.

use serde::Deserialize;

use crate::JsonNumber;

pub const SOURCE_ID: &str = "indodax";
pub const BASE_URL: &str = "https://indodax.com";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ticker {
    /// Best bid.
    pub buy: String,
    /// Best ask.
    pub sell: String,
    /// Unix seconds.
    pub server_time: i64,
}

#[derive(Deserialize)]
struct TickerEnvelope {
    ticker: Ticker,
}

fn decode_ticker(payload: &[u8]) -> Result<Ticker, String> {
    serde_json::from_slice::<TickerEnvelope>(payload)
        .map(|e| e.ticker)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Bar {
    /// Bar start, Unix seconds.
    pub time: i64,
    pub open: JsonNumber,
    pub high: JsonNumber,
    pub low: JsonNumber,
    pub close: JsonNumber,
    /// Base-asset volume, decimal text.
    pub volume: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bars(pub Vec<Bar>);

fn decode_bars(payload: &[u8]) -> Result<Bars, String> {
    serde_json::from_slice(payload)
        .map(Bars)
        .map_err(|e| e.to_string())
}

crate::quote_provider!(IndodaxProvider, Ticker, decode_ticker);
crate::bars_provider!(IndodaxProvider, Bars, decode_bars);

impl IndodaxProvider {
    pub fn ticker_url(pair: &str) -> String {
        format!("{BASE_URL}/api/ticker/{pair}")
    }

    /// `tf` in minutes (`60`) or `1D`; `from`/`to` Unix seconds.
    pub fn history_url(pair: &str, tf: &str, from: i64, to: i64) -> String {
        format!(
            "{BASE_URL}/tradingview/history_v2?symbol={}&tf={tf}&from={from}&to={to}",
            pair.to_uppercase()
        )
    }
}

/// Fetches one market's ticker (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ticker(
    client: &crate::http::HttpClient,
    pair: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&IndodaxProvider::ticker_url(pair), &[]).await
}

/// Fetches one market's bars between `from` and `to` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_history(
    client: &crate::http::HttpClient,
    pair: &str,
    tf: &str,
    from: i64,
    to: i64,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&IndodaxProvider::history_url(pair, tf, from, to), &[])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarsProvider, QuoteProvider};

    #[test]
    fn decodes_captured_ticker_and_bars_verbatim() {
        let t = IndodaxProvider::new()
            .decode_quote(&crate::fixture("indodax/ticker-usdtidr.json"))
            .unwrap();
        assert!(t.server_time > 1_700_000_000);
        assert!(!t.buy.is_empty() && !t.sell.is_empty());
        let b = IndodaxProvider::new()
            .decode_bars(&crate::fixture("indodax/history-USDTIDR-60.json"))
            .unwrap();
        assert!(!b.0.is_empty());
        assert_eq!(b.0[0].time % 3600, 0, "hour-aligned bar start");
        assert!(
            b.0[0]
                .open
                .0
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
        );
        assert_eq!(
            IndodaxProvider::history_url("usdtidr", "60", 1, 2),
            "https://indodax.com/tradingview/history_v2?symbol=USDTIDR&tf=60&from=1&to=2"
        );
    }
}
