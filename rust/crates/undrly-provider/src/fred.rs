//! FRED (Federal Reserve Bank of St. Louis) release calendar:
//! `GET /fred/release/dates?release_id=…` with future scheduled dates
//! (`include_release_dates_with_no_data=true`). Needs `FRED_API_KEY`, which
//! is sent only in the request URL and never stored or used as a record key.
//!
//! Each date is when the release is scheduled or was published (US), a date
//! without a time of day. Terms: applications must state "This product uses
//! the FRED® API but is not endorsed or certified by the Federal Reserve Bank
//! of St. Louis."

use serde::Deserialize;

pub const SOURCE_ID: &str = "fred";
pub const API: &str = "https://api.stlouisfed.org/fred";
/// The notice FRED's terms require wherever its data is shown.
pub const NOTICE: &str = "This product uses the FRED® API but is not endorsed or certified by the Federal Reserve Bank of St. Louis.";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseDates {
    pub release_dates: Vec<ReleaseDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseDate {
    pub release_id: u32,
    /// `YYYY-MM-DD`.
    pub date: String,
}

fn decode(payload: &[u8]) -> Result<ReleaseDates, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(FredProvider, ReleaseDates, decode);

/// Release dates of `release_id` between two dates, without the API key.
pub fn release_dates_url(release_id: &str, start: &str, end: &str) -> String {
    format!(
        "{API}/release/dates?release_id={release_id}&file_type=json&realtime_start={start}\
         &realtime_end={end}&include_release_dates_with_no_data=true&sort_order=asc"
    )
}

/// Fetches release dates (feature `http`); the key is appended to the URL only.
#[cfg(feature = "http")]
pub async fn fetch_release_dates(
    client: &crate::http::HttpClient,
    api_key: &str,
    release_id: &str,
    start: &str,
    end: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    let key = release_dates_url(release_id, start, end);
    let url = format!("{key}&api_key={api_key}");
    client.get_secret_query(&url, &key, api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_release_dates() {
        let r = FredProvider::new()
            .decode_quote(&crate::fixture("fred/release-dates-10.json"))
            .unwrap();
        assert_eq!(r.release_dates.len(), 5);
        assert_eq!(r.release_dates[0].release_id, 10);
        assert_eq!(r.release_dates[0].date, "2026-08-12");
        assert!(!release_dates_url("10", "a", "b").contains("api_key"));
        assert!(
            FredProvider::new()
                .decode_quote(br#"{"error_code":400}"#)
                .is_err()
        );
    }
}
