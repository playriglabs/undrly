//! Kraken spot REST ticker (`/0/public/Ticker`), for BTC/USD and EUR/USD.
//!
//! One request covers every configured pair. Kraken keys the response by its
//! pair name (`XXBTZUSD`, `ZEURZUSD`), which is also accepted in the request,
//! so feed symbols are pair names. The ticker states no timestamp.

use std::collections::BTreeMap;

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "kraken";
pub const TICKER_URL: &str = "https://api.kraken.com/0/public/Ticker";
pub const ASSET_PAIRS_URL: &str = "https://api.kraken.com/0/public/AssetPairs";

/// Kraken's tradable pairs (`/0/public/AssetPairs`), keyed by pair name, the
/// name the ticker uses. Universe building only.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetPairs {
    pub error: Vec<String>,
    #[serde(default)]
    pub result: BTreeMap<String, AssetPair>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetPair {
    pub altname: String,
    /// `XBT/USD`: the spelling CoinGecko's Kraken tickers use.
    pub wsname: Option<String>,
    pub status: Option<String>,
}

pub fn decode_asset_pairs(payload: &[u8]) -> Result<AssetPairs, DecodeError> {
    let reject = |reason: String| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason,
    };
    let pairs: AssetPairs = serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
    if !pairs.error.is_empty() {
        return Err(reject(format!("Kraken error: {}", pairs.error.join("; "))));
    }
    Ok(pairs)
}

/// Kraken's ticker response. Only the fields Undrly uses are decoded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ticker {
    pub error: Vec<String>,
    #[serde(default)]
    pub result: BTreeMap<String, Tick>,
}

/// One pair's ticker: `[price, whole lot volume, lot volume]` for ask/bid,
/// `[price, lot volume]` for the last trade. Values verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tick {
    pub a: Vec<String>,
    pub b: Vec<String>,
    pub c: Vec<String>,
}

pub struct KrakenProvider {
    source_id: SourceId,
}

impl KrakenProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    /// The ticker URL for `pairs` (Kraken pair names).
    pub fn ticker_url(pairs: &[&str]) -> String {
        format!("{TICKER_URL}?pair={}", pairs.join(","))
    }
}

/// Kraken OHLC (`/0/public/OHLC?pair=…&interval=…`): for one pair,
/// `[time, open, high, low, close, vwap, volume, count]` per bar (time in
/// Unix seconds, the bar's start; prices and volume as decimal strings;
/// volume in the pair's base asset). At most the 720 most recent bars. The
/// last bar is the one still in progress (`last` is the start of the last
/// committed bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ohlc {
    pub pair: String,
    pub bars: Vec<OhlcBar>,
    /// Start time of the last committed (complete) bar, Unix seconds.
    pub last: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OhlcBar {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub count: u64,
}

pub const OHLC_URL: &str = "https://api.kraken.com/0/public/OHLC";

impl KrakenProvider {
    /// OHLC bars of `pair` at `interval_minutes` (60, 1440), since a Unix
    /// time when given.
    pub fn ohlc_url(pair: &str, interval_minutes: u32, since: Option<i64>) -> String {
        match since {
            Some(s) => format!("{OHLC_URL}?pair={pair}&interval={interval_minutes}&since={s}"),
            None => format!("{OHLC_URL}?pair={pair}&interval={interval_minutes}"),
        }
    }
}

fn decode_ohlc(payload: &[u8]) -> Result<Ohlc, String> {
    use serde_json::Value;
    let v: Value = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    if let Some(errors) = v["error"].as_array()
        && !errors.is_empty()
    {
        return Err(format!("Kraken error: {v}", v = v["error"]));
    }
    let result = v["result"].as_object().ok_or("no result")?;
    let last = result
        .get("last")
        .and_then(Value::as_i64)
        .ok_or("no `last`")?;
    let mut pairs = result.iter().filter(|(k, _)| *k != "last");
    let (pair, rows) = pairs.next().ok_or("no pair")?;
    if pairs.next().is_some() {
        return Err("more than one pair".into());
    }
    let text = |x: &Value| {
        x.as_str()
            .map(str::to_owned)
            .ok_or("expected a decimal string")
    };
    let bars = rows
        .as_array()
        .ok_or("bars are not an array")?
        .iter()
        .map(|r| {
            let r = r
                .as_array()
                .filter(|r| r.len() == 8)
                .ok_or("a bar is not 8 fields")?;
            Ok(OhlcBar {
                time: r[0].as_i64().ok_or("time is not an integer")?,
                open: text(&r[1])?,
                high: text(&r[2])?,
                low: text(&r[3])?,
                close: text(&r[4])?,
                volume: text(&r[6])?,
                count: r[7].as_u64().ok_or("count is not an integer")?,
            })
        })
        .collect::<Result<Vec<_>, &str>>()?;
    Ok(Ohlc {
        pair: pair.clone(),
        bars,
        last,
    })
}

impl crate::BarsProvider for KrakenProvider {
    type Bars = Ohlc;

    fn decode_bars(&self, payload: &[u8]) -> Result<Ohlc, DecodeError> {
        decode_ohlc(payload).map_err(|reason| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        })
    }
}

/// Fetches OHLC bars (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ohlc(
    client: &crate::http::HttpClient,
    pair: &str,
    interval_minutes: u32,
    since: Option<i64>,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(
            &KrakenProvider::ohlc_url(pair, interval_minutes, since),
            &[],
        )
        .await
}

/// Fetches the ticker for `pairs` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_ticker(
    client: &crate::http::HttpClient,
    pairs: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&KrakenProvider::ticker_url(pairs), &[]).await
}

impl Default for KrakenProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for KrakenProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for KrakenProvider {
    type Quote = Ticker;

    fn decode_quote(&self, payload: &[u8]) -> Result<Ticker, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let ticker: Ticker = serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
        if !ticker.error.is_empty() {
            return Err(reject(format!("Kraken error: {}", ticker.error.join("; "))));
        }
        Ok(ticker)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn captured() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/kraken/ticker.json"),
        )
        .unwrap()
    }

    #[test]
    fn decodes_captured_ticker_verbatim() {
        let t = KrakenProvider::new().decode_quote(&captured()).unwrap();
        let keys: Vec<&str> = t.result.keys().map(String::as_str).collect();
        assert_eq!(keys, ["XXBTZUSD", "ZEURZUSD"]);
        assert_eq!(t.result["ZEURZUSD"].c[0], "1.13680");
    }

    #[test]
    fn decodes_captured_ohlc_verbatim() {
        use crate::BarsProvider;
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/kraken/ohlc-XXBTZUSD-60.json"),
        )
        .unwrap();
        let o = KrakenProvider::new().decode_bars(&payload).unwrap();
        assert_eq!(o.pair, "XXBTZUSD");
        assert_eq!(o.bars.len(), 6);
        assert_eq!(o.last, 1790391600);
        assert!(o.bars.last().unwrap().time > o.last, "last bar in progress");
        assert!(
            KrakenProvider::new()
                .decode_bars(br#"{"error":["EGeneral:Invalid arguments"]}"#)
                .is_err()
        );
        assert_eq!(
            KrakenProvider::ohlc_url("XXBTZUSD", 60, Some(1)),
            "https://api.kraken.com/0/public/OHLC?pair=XXBTZUSD&interval=60&since=1"
        );
    }

    #[test]
    fn rejects_error_responses_and_malformed_payloads() {
        for payload in [
            &br#"{"error":["EQuery:Unknown asset pair"]}"#[..],
            b"<html>",
            br#"{"error":[],"result":{"XXBTZUSD":{"a":"x"}}}"#,
        ] {
            assert!(KrakenProvider::new().decode_quote(payload).is_err());
        }
        assert_eq!(
            KrakenProvider::ticker_url(&["XXBTZUSD", "ZEURZUSD"]),
            "https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD"
        );
    }
}
