//! Bank of Canada Valet API, daily exchange rates
//! (`GET /valet/observations/{series,…}/json?recent=5`). No authentication.
//!
//! Series `FX{CCY}CAD` are "daily average exchange rate[s] … of one unit of
//! foreign currency expressed in Canadian dollars", published once each
//! business day by 16:30 ET; the Bank calls them "indicative rates only".
//! Reference rates, not market quotes: no bid/ask. The canonical pair for a
//! series is `{CCY}/CAD`; `CAD/{CCY}` is its explicit inverse. Values are
//! decimal strings, kept verbatim (four significant digits).

use std::collections::BTreeMap;

use serde::Deserialize;

pub const SOURCE_ID: &str = "bank-of-canada";
pub const BASE_URL: &str = "https://www.bankofcanada.ca/valet/observations";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Observations {
    pub observations: Vec<BTreeMap<String, ObservationValue>>,
}

/// Either the date (`d`) or a series value (`{"v": "1.6122"}`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ObservationValue {
    Date(String),
    Value { v: String },
}

fn decode(payload: &[u8]) -> Result<Observations, String> {
    let o: Observations = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    for row in &o.observations {
        if !matches!(row.get("d"), Some(ObservationValue::Date(_))) {
            return Err("an observation without a date".into());
        }
    }
    Ok(o)
}

crate::quote_provider!(BankOfCanadaProvider, Observations, decode);

impl BankOfCanadaProvider {
    /// The last five observations of `series`.
    pub fn observations_url(series: &[&str]) -> String {
        Self::recent_url(series, 5)
    }

    /// The last `n` observations of `series` (history backfill).
    pub fn recent_url(series: &[&str], n: u32) -> String {
        format!("{BASE_URL}/{}/json?recent={n}", series.join(","))
    }
}

/// Fetches `url` (a [`BankOfCanadaProvider::recent_url`]; feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_url(
    client: &crate::http::HttpClient,
    url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(url, &[]).await
}

/// Fetches the recent observations of `series` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_observations(
    client: &crate::http::HttpClient,
    series: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get(&BankOfCanadaProvider::observations_url(series), &[])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_observations_verbatim() {
        let o = BankOfCanadaProvider::new()
            .decode_quote(&crate::fixture("bank-of-canada/observations.json"))
            .unwrap();
        assert_eq!(o.observations.len(), 5);
        let has = |d: &str, s: &str, v: &str| {
            o.observations.iter().any(|r| {
                r.get("d") == Some(&ObservationValue::Date(d.into()))
                    && r.get(s) == Some(&ObservationValue::Value { v: v.into() })
            })
        };
        assert!(has("2026-09-22", "FXJPYCAD", "0.008940"));
        assert_eq!(
            BankOfCanadaProvider::observations_url(&["FXEURCAD", "FXJPYCAD"]),
            "https://www.bankofcanada.ca/valet/observations/FXEURCAD,FXJPYCAD/json?recent=5"
        );
        assert!(
            BankOfCanadaProvider::new()
                .decode_quote(br#"{"observations":[{"FXEURCAD":{"v":"1"}}]}"#)
                .is_err()
        );
    }
}
