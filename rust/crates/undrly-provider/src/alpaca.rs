//! Alpaca market data, IEX feed (`GET /v2/stocks/snapshots?symbols=…&feed=iex`).
//!
//! The IEX feed carries trades and quotes **from the IEX exchange only**: an
//! IEX venue quote (`basis = venue`, `venue = IEX`, `source = alpaca`). The
//! response is keyed by symbol; prices are JSON numbers, kept as exact text.
//! Requests need an API key pair, sent as headers and never stored.

use std::collections::BTreeMap;

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, JsonNumber, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "alpaca";
pub const SNAPSHOTS_URL: &str = "https://data.alpaca.markets/v2/stocks/snapshots";

/// Alpaca's exchange code for IEX.
pub const IEX_EXCHANGE_CODE: &str = "V";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub latest_trade: Option<Trade>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Trade {
    /// Trade time, RFC 3339 with nanoseconds.
    pub t: String,
    /// Exchange code (`V` = IEX).
    pub x: String,
    /// Price.
    pub p: JsonNumber,
}

/// Snapshots keyed by symbol.
pub type Snapshots = BTreeMap<String, Snapshot>;

pub struct AlpacaProvider {
    source_id: SourceId,
}

impl AlpacaProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    pub fn snapshots_url(symbols: &[&str]) -> String {
        format!("{SNAPSHOTS_URL}?symbols={}&feed=iex", symbols.join(","))
    }
}

impl Default for AlpacaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AlpacaProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for AlpacaProvider {
    type Quote = Snapshots;

    fn decode_quote(&self, payload: &[u8]) -> Result<Snapshots, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// API key pair for Alpaca market data.
#[derive(Clone)]
pub struct Credentials {
    pub key_id: String,
    pub secret_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials { .. }")
    }
}

/// Fetches IEX snapshots for `symbols` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_snapshots(
    client: &crate::http::HttpClient,
    credentials: &Credentials,
    symbols: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(
            &AlpacaProvider::snapshots_url(symbols),
            &[
                ("APCA-API-KEY-ID", credentials.key_id.as_str()),
                ("APCA-API-SECRET-KEY", credentials.secret_key.as_str()),
            ],
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_captured_iex_snapshot() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/alpaca/snapshots-NVDA.json"),
        )
        .unwrap();
        let s = AlpacaProvider::new().decode_quote(&payload).unwrap();
        let t = s["NVDA"].latest_trade.as_ref().unwrap();
        assert_eq!(
            (t.x.as_str(), t.p.0.as_str(), t.t.as_str()),
            ("V", "223.71", "2026-09-24T20:45:15.183009877Z")
        );
        assert!(
            AlpacaProvider::new()
                .decode_quote(br#"{"message":"forbidden."}"#)
                .is_err()
        );
        assert_eq!(
            AlpacaProvider::snapshots_url(&["NVDA"]),
            "https://data.alpaca.markets/v2/stocks/snapshots?symbols=NVDA&feed=iex"
        );
        let redacted = format!(
            "{:?}",
            Credentials {
                key_id: "k".into(),
                secret_key: "s".into()
            }
        );
        assert!(!redacted.contains('s') || redacted == "Credentials { .. }");
    }
}
