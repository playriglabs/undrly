//! Central Bank of Myanmar reference exchange rates
//! (`GET https://forex.cbm.gov.mm/api/latest`). No authentication.
//!
//! Kyat (MMK) reference rates set by the Central Bank, with the rates' time
//! as Unix seconds (`timestamp`). Official reference rates, not market
//! quotes: no bid/ask. The payload does not state how many units of each
//! currency a rate is for (USD is per 1 USD; several others are per 100), so
//! the normalizer accepts only currencies whose unit is known. Values are
//! decimal strings, kept verbatim.

use std::collections::BTreeMap;

use serde::Deserialize;

pub const SOURCE_ID: &str = "cbm";
pub const LATEST_URL: &str = "https://forex.cbm.gov.mm/api/latest";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Latest {
    /// Unix seconds: text in `/latest`, a number in `/history/…`.
    #[serde(deserialize_with = "text_or_number")]
    pub timestamp: String,
    /// An empty JSON array (not an object) on a day without rates.
    #[serde(deserialize_with = "rates_or_empty")]
    pub rates: BTreeMap<String, String>,
}

fn rates_or_empty<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum R {
        Map(BTreeMap<String, String>),
        Empty([(); 0]),
    }
    Ok(match R::deserialize(d)? {
        R::Map(m) => m,
        R::Empty(_) => BTreeMap::new(),
    })
}

fn text_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum T {
        Text(String),
        Number(u64),
    }
    Ok(match T::deserialize(d)? {
        T::Text(s) => s,
        T::Number(n) => n.to_string(),
    })
}

/// One day's reference rates (`DD-MM-YYYY`), in `/latest`'s format (history).
pub fn history_url(day: &str) -> String {
    format!("https://forex.cbm.gov.mm/api/history/{day}")
}

fn decode(payload: &[u8]) -> Result<Latest, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(CbmProvider, Latest, decode);

/// Fetches one day's rates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_history_day(
    client: &crate::http::HttpClient,
    day: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&history_url(day), &[]).await
}

/// Fetches the latest reference rates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_latest(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(LATEST_URL, &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_rates_verbatim() {
        let l = CbmProvider::new()
            .decode_quote(&crate::fixture("cbm/latest.json"))
            .unwrap();
        assert_eq!(l.timestamp, "1790323200");
        assert_eq!(l.rates["USD"], "2100.00");
        assert!(CbmProvider::new().decode_quote(br#"{"rates":{}}"#).is_err());
        let h = CbmProvider::new()
            .decode_quote(br#"{"timestamp":1790208000,"rates":{"USD":"2100.00"}}"#)
            .unwrap();
        assert_eq!(h.timestamp, "1790208000");
        let none = CbmProvider::new()
            .decode_quote(br#"{"timestamp":1790208000,"rates":[]}"#)
            .unwrap();
        assert!(none.rates.is_empty(), "a day without rates");
        assert!(
            CbmProvider::new()
                .decode_quote(br#"{"timestamp":1,"rates":[1]}"#)
                .is_err()
        );
    }
}
