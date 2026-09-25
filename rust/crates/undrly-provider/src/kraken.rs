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
