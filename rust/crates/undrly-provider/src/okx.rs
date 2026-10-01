//! OKX public market data (V1.9), for its stablecoin/fiat books (USDT/SGD,
//! USDC/SGD, USDG/SGD, USDT/AED, USDT/BRL, ...). No authentication.
//!
//! - `GET /api/v5/market/ticker?instId=`: `{code, data: [{instId, bidPx,
//!   askPx, ts}]}`, the market named, `ts` in Unix milliseconds.
//! - `GET /api/v5/market/history-candles?instId=&bar=`: `{code, data:
//!   [[ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm]]}`, newest first;
//!   the payload names no market. `confirm = "0"` is the bar in progress.
//!
//! A non-zero `code` is an error. Values are decimal strings, verbatim.

use serde::Deserialize;

pub const SOURCE_ID: &str = "okx";
pub const BASE_URL: &str = "https://www.okx.com";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticker {
    pub inst_id: String,
    pub bid_px: String,
    pub ask_px: String,
    /// Unix milliseconds, as text.
    pub ts: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tickers(pub Vec<Ticker>);

/// One candle: the fields Undrly uses, verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candle {
    /// Bar start, Unix milliseconds.
    pub ts: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    /// Base-currency volume.
    pub volume: String,
}

/// One market's candles, newest first as served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candles(pub Vec<Candle>);

#[derive(Deserialize)]
struct Envelope<T> {
    code: String,
    #[serde(default)]
    msg: String,
    data: T,
}

fn envelope<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, String> {
    let e: Envelope<T> = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    if e.code != "0" {
        return Err(format!("OKX error {}: {}", e.code, e.msg));
    }
    Ok(e.data)
}

fn decode_tickers(payload: &[u8]) -> Result<Tickers, String> {
    envelope(payload).map(Tickers)
}

fn decode_candles(payload: &[u8]) -> Result<Candles, String> {
    let rows: Vec<Vec<String>> = envelope(payload)?;
    rows.into_iter()
        .map(|r| {
            let get = |i: usize, field: &str| {
                r.get(i)
                    .cloned()
                    .ok_or_else(|| format!("candle: no `{field}`"))
            };
            Ok(Candle {
                ts: get(0, "ts")?
                    .parse()
                    .map_err(|_| "candle: `ts` is not an integer".to_owned())?,
                open: get(1, "open")?,
                high: get(2, "high")?,
                low: get(3, "low")?,
                close: get(4, "close")?,
                volume: get(5, "vol")?,
            })
        })
        .collect::<Result<_, String>>()
        .map(Candles)
}

crate::quote_provider!(OkxProvider, Tickers, decode_tickers);
crate::bars_provider!(OkxProvider, Candles, decode_candles);

impl OkxProvider {
    pub fn ticker_url(inst_id: &str) -> String {
        format!("{BASE_URL}/api/v5/market/ticker?instId={inst_id}")
    }

    /// `bar` is OKX's own (`1H`, `1Dutc`); `after_ms` pages back in time
    /// (bars strictly older than it); at most 100 bars per request.
    pub fn candles_url(inst_id: &str, bar: &str, after_ms: Option<i64>) -> String {
        let after = after_ms.map(|t| format!("&after={t}")).unwrap_or_default();
        format!(
            "{BASE_URL}/api/v5/market/history-candles?instId={inst_id}&bar={bar}&limit=100{after}"
        )
    }
}

/// Fetches one market's ticker (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ticker(
    client: &crate::http::HttpClient,
    inst_id: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&OkxProvider::ticker_url(inst_id), &[]).await
}

/// Fetches one page of candles (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_candles(
    client: &crate::http::HttpClient,
    inst_id: &str,
    bar: &str,
    after_ms: Option<i64>,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&OkxProvider::candles_url(inst_id, bar, after_ms), &[])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarsProvider, QuoteProvider};

    #[test]
    fn decodes_captured_ticker_and_candles() {
        let t = OkxProvider::new()
            .decode_quote(&crate::fixture("okx/ticker-USDT-SGD.json"))
            .unwrap();
        assert_eq!(t.0[0].inst_id, "USDT-SGD");
        assert!(t.0[0].bid_px.contains('.'));
        let c = OkxProvider::new()
            .decode_bars(&crate::fixture("okx/candles-USDT-SGD-1H.json"))
            .unwrap();
        assert_eq!(c.0.len(), 3);
        assert!(c.0[0].ts > c.0[1].ts, "newest first");
        assert!(
            OkxProvider::new()
                .decode_quote(
                    b"{\"code\":\"51001\",\"msg\":\"Instrument ID does not exist\",\"data\":[]}"
                )
                .is_err()
        );
    }
}
