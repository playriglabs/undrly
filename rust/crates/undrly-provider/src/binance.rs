//! Binance spot public market data (V1.9, docs/v1.9-live-fx.md), for its
//! stablecoin/fiat books (USDT/IDR, USDC/IDR, USDT/AED, USDT/BRL, ...). No
//! authentication.
//!
//! - `GET /api/v3/ticker/bookTicker?symbols=[...]`: each market's best bid
//!   and ask, named by symbol (in Binance's order, not the request's); one
//!   request covers many markets. The payload states no time (receipt time
//!   is the observation time).
//! - `GET /api/v3/klines?symbol=&interval=`: one market's bars, as arrays
//!   `[openTime ms, open, high, low, close, volume, closeTime, quoteVolume,
//!   trades, ...]`. The payload names no market.
//!
//! The same book-ticker and kline layouts are served by other venues
//! (Coins.ph, HashKey), which reuse [`BookTicker`], [`Klines`] and their
//! decoders. Values are decimal strings, kept verbatim.

use serde::Deserialize;

pub const SOURCE_ID: &str = "binance";
pub const BASE_URL: &str = "https://api.binance.com";
/// Every trading spot market (V1.10: confirms bStock markets).
pub const EXCHANGE_INFO_URL: &str =
    "https://api.binance.com/api/v3/exchangeInfo?permissions=SPOT&symbolStatus=TRADING";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExchangeInfo {
    pub symbols: Vec<Market>,
}

/// One spot market's identity fields.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub symbol: String,
    pub status: String,
    pub base_asset: String,
    pub quote_asset: String,
}

pub fn decode_exchange_info(payload: &[u8]) -> Result<ExchangeInfo, crate::DecodeError> {
    serde_json::from_slice(payload).map_err(|e| crate::DecodeError {
        source_id: undrly_core::SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: format!("exchangeInfo: {e}"),
    })
}

/// One market's best bid and ask.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookTicker {
    pub symbol: String,
    pub bid_price: String,
    pub ask_price: String,
}

/// Book tickers of one response (an array, or one object for a single
/// requested symbol).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookTickers(pub Vec<BookTicker>);

/// One kline (bar): the fields Undrly uses, verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kline {
    /// Bar start, Unix milliseconds.
    pub open_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    /// Base-asset volume.
    pub volume: String,
    pub trades: Option<u64>,
}

/// One market's klines, oldest first as served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Klines(pub Vec<Kline>);

pub fn decode_book_tickers(payload: &[u8]) -> Result<BookTickers, String> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<BookTicker>),
        One(BookTicker),
    }
    match serde_json::from_slice(payload).map_err(|e| e.to_string())? {
        OneOrMany::Many(v) => Ok(BookTickers(v)),
        OneOrMany::One(t) => Ok(BookTickers(vec![t])),
    }
}

pub fn decode_klines(payload: &[u8]) -> Result<Klines, String> {
    use serde_json::Value;
    let rows: Vec<Vec<Value>> = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    let text = |row: &[Value], i: usize, field: &str| -> Result<String, String> {
        row.get(i)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("kline: `{field}` is not decimal text"))
    };
    rows.iter()
        .map(|row| {
            Ok(Kline {
                open_time: row
                    .first()
                    .and_then(Value::as_i64)
                    .ok_or("kline: no open time")?,
                open: text(row, 1, "open")?,
                high: text(row, 2, "high")?,
                low: text(row, 3, "low")?,
                close: text(row, 4, "close")?,
                volume: text(row, 5, "volume")?,
                trades: row.get(8).and_then(Value::as_u64),
            })
        })
        .collect::<Result<_, String>>()
        .map(Klines)
}

crate::quote_provider!(BinanceProvider, BookTickers, decode_book_tickers);
crate::bars_provider!(BinanceProvider, Klines, decode_klines);

impl BinanceProvider {
    /// Book tickers of several symbols in one request.
    pub fn book_ticker_url(symbols: &[&str]) -> String {
        let list = symbols
            .iter()
            .map(|s| format!("%22{s}%22"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{BASE_URL}/api/v3/ticker/bookTicker?symbols=%5B{list}%5D")
    }

    /// `interval` is Binance's own (`1h`, `1d`); `start_ms` the first bar.
    pub fn klines_url(symbol: &str, interval: &str, start_ms: i64, limit: u32) -> String {
        format!(
            "{BASE_URL}/api/v3/klines?symbol={symbol}&interval={interval}&startTime={start_ms}&limit={limit}"
        )
    }
}

/// Fetches the book tickers of `symbols` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_book_tickers(
    client: &crate::http::HttpClient,
    symbols: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&BinanceProvider::book_ticker_url(symbols), &[])
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
            &BinanceProvider::klines_url(symbol, interval, start_ms, limit),
            &[],
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarsProvider, QuoteProvider};

    #[test]
    fn decodes_captured_book_tickers_verbatim() {
        let t = BinanceProvider::new()
            .decode_quote(&crate::fixture("binance/book-ticker.json"))
            .unwrap();
        // Binance answers in its own order, not the request's.
        let mut symbols: Vec<&str> = t.0.iter().map(|b| b.symbol.as_str()).collect();
        symbols.sort_unstable();
        assert_eq!(symbols, ["USDCIDR", "USDTAED", "USDTBRL", "USDTIDR"]);
        assert!(
            t.0.iter().all(|b| b.bid_price.contains('.')),
            "decimal text kept"
        );
        assert!(
            BinanceProvider::new()
                .decode_quote(b"{\"symbol\":\"X\"}")
                .is_err()
        );
        assert_eq!(
            BinanceProvider::book_ticker_url(&["USDTIDR", "USDCIDR"]),
            "https://api.binance.com/api/v3/ticker/bookTicker?symbols=%5B%22USDTIDR%22,%22USDCIDR%22%5D"
        );
    }

    #[test]
    fn decodes_captured_klines_verbatim() {
        let k = BinanceProvider::new()
            .decode_bars(&crate::fixture("binance/klines-USDTIDR-1h.json"))
            .unwrap();
        assert_eq!(k.0.len(), 3);
        assert_eq!(k.0[0].open_time % 3_600_000, 0, "hour-aligned bar start");
        assert!(k.0[0].trades.is_some());
        assert!(decode_klines(b"[[1,2,\"3\",\"4\",\"5\",\"6\"]]").is_err());
    }
}
