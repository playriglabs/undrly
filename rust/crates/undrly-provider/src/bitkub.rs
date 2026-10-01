//! Bitkub public market data (V1.9), for its THB books (USDT/THB,
//! USDC/THB). No authentication.
//!
//! - `GET /api/v3/market/ticker?sym=`: `[{symbol, highest_bid, lowest_ask,
//!   ...}]`, the market named; no time is stated.
//! - `GET /tradingview/history?symbol=&resolution=&from=&to=`: columnar
//!   `{s, t[], o[], h[], l[], c[], v[]}` with prices as JSON **numbers**,
//!   kept as written; `t` is the bar start in Unix seconds; no market named.

use serde::Deserialize;

use crate::JsonNumber;

pub const SOURCE_ID: &str = "bitkub";
pub const BASE_URL: &str = "https://api.bitkub.com";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ticker {
    pub symbol: String,
    pub highest_bid: String,
    pub lowest_ask: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tickers(pub Vec<Ticker>);

fn decode_tickers(payload: &[u8]) -> Result<Tickers, String> {
    serde_json::from_slice(payload)
        .map(Tickers)
        .map_err(|e| e.to_string())
}

/// One market's bars, columnar as served.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct History {
    /// `ok`, or `no_data` for an empty window.
    pub s: String,
    #[serde(default)]
    pub t: Vec<i64>,
    #[serde(default)]
    pub o: Vec<JsonNumber>,
    #[serde(default)]
    pub h: Vec<JsonNumber>,
    #[serde(default)]
    pub l: Vec<JsonNumber>,
    #[serde(default)]
    pub c: Vec<JsonNumber>,
    #[serde(default)]
    pub v: Vec<JsonNumber>,
}

fn decode_history(payload: &[u8]) -> Result<History, String> {
    let h: History = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    let n = h.t.len();
    if [h.o.len(), h.h.len(), h.l.len(), h.c.len(), h.v.len()]
        .iter()
        .any(|&len| len != n)
    {
        return Err("history: columns of different lengths".into());
    }
    Ok(h)
}

crate::quote_provider!(BitkubProvider, Tickers, decode_tickers);
crate::bars_provider!(BitkubProvider, History, decode_history);

impl BitkubProvider {
    pub fn ticker_url(symbol: &str) -> String {
        format!("{BASE_URL}/api/v3/market/ticker?sym={symbol}")
    }

    /// `resolution` in minutes (`60`) or `1D`; `from`/`to` Unix seconds.
    pub fn history_url(symbol: &str, resolution: &str, from: i64, to: i64) -> String {
        format!(
            "{BASE_URL}/tradingview/history?symbol={symbol}&resolution={resolution}&from={from}&to={to}"
        )
    }
}

/// Fetches one market's ticker (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ticker(
    client: &crate::http::HttpClient,
    symbol: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&BitkubProvider::ticker_url(symbol), &[]).await
}

/// Fetches one market's bars between `from` and `to` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_history(
    client: &crate::http::HttpClient,
    symbol: &str,
    resolution: &str,
    from: i64,
    to: i64,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(
            &BitkubProvider::history_url(symbol, resolution, from, to),
            &[],
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarsProvider, QuoteProvider};

    #[test]
    fn decodes_captured_ticker_and_history_verbatim() {
        let t = BitkubProvider::new()
            .decode_quote(&crate::fixture("bitkub/ticker-USDT_THB.json"))
            .unwrap();
        assert_eq!(t.0[0].symbol, "USDT_THB");
        let h = BitkubProvider::new()
            .decode_bars(&crate::fixture("bitkub/history-USDT_THB-60.json"))
            .unwrap();
        assert_eq!(h.s, "ok");
        assert_eq!(h.t.len(), h.c.len());
        assert!(
            BitkubProvider::new()
                .decode_bars(
                    b"{\"s\":\"ok\",\"t\":[1],\"o\":[],\"h\":[],\"l\":[],\"c\":[],\"v\":[]}"
                )
                .is_err()
        );
    }
}
