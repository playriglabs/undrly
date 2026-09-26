//! Finnhub earnings calendar (`GET /api/v1/calendar/earnings?from=&to=`).
//! Needs `FINNHUB_API_KEY`, sent only in the request URL, never stored or
//! used as a record key.
//!
//! Per report: `symbol` (a US ticker), `date`, `hour` (`bmo` before the
//! open, `amc` after the close, `dmh` during market hours, empty when not
//! stated), fiscal `quarter`/`year`, and Finnhub's EPS / revenue estimates
//! and actuals (JSON numbers, kept as their text; no currency stated).
//! Terms: the free plan is for personal use and forbids redistribution, so
//! this source is for local/private use only.

use serde::Deserialize;

use crate::JsonNumber;

pub const SOURCE_ID: &str = "finnhub";
pub const EARNINGS_URL: &str = "https://finnhub.io/api/v1/calendar/earnings";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EarningsCalendar {
    #[serde(rename = "earningsCalendar")]
    pub earnings: Vec<Earnings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Earnings {
    pub symbol: String,
    pub date: String,
    #[serde(default)]
    pub hour: Option<String>,
    pub quarter: u32,
    pub year: i32,
    pub eps_estimate: Option<JsonNumber>,
    pub eps_actual: Option<JsonNumber>,
    pub revenue_estimate: Option<JsonNumber>,
    pub revenue_actual: Option<JsonNumber>,
}

fn decode(payload: &[u8]) -> Result<EarningsCalendar, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(FinnhubProvider, EarningsCalendar, decode);

/// Earnings between two dates (inclusive), without the API key.
pub fn earnings_url(from: &str, to: &str) -> String {
    format!("{EARNINGS_URL}?from={from}&to={to}")
}

/// Fetches the earnings calendar (feature `http`); the key is in the URL only.
#[cfg(feature = "http")]
pub async fn fetch_earnings(
    client: &crate::http::HttpClient,
    api_key: &str,
    from: &str,
    to: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    let key = earnings_url(from, to);
    let url = format!("{key}&token={api_key}");
    client.get_secret_query(&url, &key, api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_the_earnings_format() {
        let c = FinnhubProvider::new()
            .decode_quote(&crate::fixture("finnhub/earnings-calendar.json"))
            .unwrap();
        assert_eq!(c.earnings.len(), 4);
        let e = &c.earnings[0];
        assert_eq!((e.symbol.as_str(), e.quarter, e.year), ("NVDA", 3, 2027));
        assert_eq!(e.eps_estimate, Some(JsonNumber("1.2501".into())));
        assert_eq!(e.hour.as_deref(), Some("amc"));
        assert!(!earnings_url("a", "b").contains("token"));
        assert!(FinnhubProvider::new().decode_quote(b"{}").is_err());
    }
}
