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
    pub latest_quote: Option<Quote>,
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

/// IEX's top of book. A side with no IEX interest has price `0` and a
/// blank exchange code.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Quote {
    /// Quote time, RFC 3339 with nanoseconds.
    pub t: String,
    /// Bid price and exchange code.
    pub bp: JsonNumber,
    pub bx: String,
    /// Ask price and exchange code.
    pub ap: JsonNumber,
    pub ax: String,
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

pub const BARS_URL: &str = "https://data.alpaca.markets/v2/stocks/bars";
/// Alpaca's trading-calendar endpoint (US equity market sessions).
pub const CALENDAR_URL: &str = "https://paper-api.alpaca.markets/v2/calendar";

/// `/v2/stocks/bars` page: IEX-only bars per symbol (trade prices, IEX
/// volume in shares). `t` is the bar start (RFC 3339); a `1Day` bar starts
/// at 00:00 New York time. Numbers kept as their exact JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BarsPage {
    #[serde(default)]
    pub bars: std::collections::BTreeMap<String, Vec<StockBar>>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StockBar {
    pub t: String,
    pub o: JsonNumber,
    pub h: JsonNumber,
    pub l: JsonNumber,
    pub c: JsonNumber,
    pub v: JsonNumber,
    pub n: Option<u64>,
}

/// One trading date of Alpaca's calendar: New York local times (`HH:MM`
/// regular session; `HHMM` extended session).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CalendarDay {
    pub date: String,
    pub open: String,
    pub close: String,
    pub session_open: String,
    pub session_close: String,
}

impl AlpacaProvider {
    /// IEX bars for `symbols` at `timeframe` (`1Hour`, `1Day`) from `start`.
    pub fn bars_url(symbols: &[&str], timeframe: &str, start: &str, page: Option<&str>) -> String {
        let mut url = format!(
            "{BARS_URL}?symbols={}&timeframe={timeframe}&start={start}&limit=10000&feed=iex&sort=asc",
            symbols.join(",")
        );
        if let Some(p) = page {
            url.push_str("&page_token=");
            url.push_str(p);
        }
        url
    }

    pub fn calendar_url(start: &str, end: &str) -> String {
        format!("{CALENDAR_URL}?start={start}&end={end}")
    }
}

impl crate::BarsProvider for AlpacaProvider {
    type Bars = BarsPage;

    fn decode_bars(&self, payload: &[u8]) -> Result<BarsPage, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

pub fn decode_calendar(payload: &[u8]) -> Result<Vec<CalendarDay>, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
}

pub const CORPORATE_ACTIONS_URL: &str = "https://data.alpaca.markets/v1/corporate-actions";

/// `/v1/corporate-actions` page: actions by type (`cash_dividends`,
/// `forward_splits`, `stock_mergers`, …). Every field the types use is
/// optional here; which apply is the type's. Numbers keep their JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CorporateActionsPage {
    pub corporate_actions: std::collections::BTreeMap<String, Vec<CorporateAction>>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CorporateAction {
    pub id: String,
    pub symbol: Option<String>,
    pub ex_date: Option<String>,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
    pub process_date: Option<String>,
    pub effective_date: Option<String>,
    /// Cash per share (cash dividends, cash mergers) or shares per share
    /// (stock dividends).
    pub rate: Option<JsonNumber>,
    pub cash_rate: Option<JsonNumber>,
    pub old_rate: Option<JsonNumber>,
    pub new_rate: Option<JsonNumber>,
    pub special: Option<bool>,
    pub acquiree_symbol: Option<String>,
    pub acquiree_rate: Option<JsonNumber>,
    pub acquirer_symbol: Option<String>,
    pub acquirer_rate: Option<JsonNumber>,
    pub source_symbol: Option<String>,
    pub source_rate: Option<JsonNumber>,
    pub new_symbol: Option<String>,
    pub old_symbol: Option<String>,
}

impl AlpacaProvider {
    /// Corporate actions touching `symbols` between two dates (inclusive).
    pub fn corporate_actions_url(
        symbols: &[&str],
        start: &str,
        end: &str,
        page: Option<&str>,
    ) -> String {
        let mut url = format!(
            "{CORPORATE_ACTIONS_URL}?symbols={}&start={start}&end={end}&limit=1000",
            symbols.join(",")
        );
        if let Some(p) = page {
            url.push_str("&page_token=");
            url.push_str(p);
        }
        url
    }
}

pub fn decode_corporate_actions(payload: &[u8]) -> Result<CorporateActionsPage, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
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

/// GET `url` with the credentials as headers (feature `http`): bars pages
/// and the calendar.
#[cfg(feature = "http")]
pub async fn fetch_authenticated(
    client: &crate::http::HttpClient,
    credentials: &Credentials,
    url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(
            url,
            &[
                ("APCA-API-KEY-ID", credentials.key_id.as_str()),
                ("APCA-API-SECRET-KEY", credentials.secret_key.as_str()),
            ],
        )
        .await
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
    fn decodes_captured_bars_and_calendar() {
        use crate::BarsProvider;
        let read = |f: &str| {
            std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../tests/fixtures/sources/alpaca")
                    .join(f),
            )
            .unwrap()
        };
        let b = AlpacaProvider::new()
            .decode_bars(&read("bars-1Day.json"))
            .unwrap();
        assert_eq!(b.bars["AAPL"].len(), 5);
        assert_eq!(b.bars["AAPL"][0].t, "2026-09-21T04:00:00Z");
        assert_eq!(b.bars["AAPL"][0].o, JsonNumber("335.49".into()));
        assert!(b.next_page_token.is_none());
        let c = decode_calendar(&read("calendar.json")).unwrap();
        assert!(
            c.iter()
                .any(|d| d.date == "2026-11-27" && d.close == "13:00")
        );
        assert!(
            c.iter().all(|d| d.date != "2026-11-26"),
            "Thanksgiving: no session"
        );
    }

    #[test]
    fn decodes_captured_corporate_actions() {
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/alpaca/corporate-actions.json"),
        )
        .unwrap();
        let p = decode_corporate_actions(&payload).unwrap();
        let nvda: Vec<_> = p.corporate_actions["cash_dividends"]
            .iter()
            .filter(|a| a.symbol.as_deref() == Some("NVDA"))
            .collect();
        assert!(!nvda.is_empty());
        assert!(nvda.iter().all(|a| a.rate.is_some() && a.ex_date.is_some()));
        let split = &p.corporate_actions["forward_splits"][0];
        assert_eq!(
            (split.old_rate.clone(), split.new_rate.clone()),
            (Some(JsonNumber("1".into())), Some(JsonNumber("2".into())))
        );
        assert!(p.next_page_token.is_none());
        assert!(decode_corporate_actions(b"{}").is_err());
    }

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
