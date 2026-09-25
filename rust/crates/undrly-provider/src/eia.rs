//! U.S. Energy Information Administration API v2: daily spot prices for WTI
//! (`RWTC`), Brent (`RBRTE`) and Henry Hub natural gas (`RNGWHHD`).
//!
//! Reference prices (no venue), published with a lag of days. The API key
//! travels only in the request URL; the stored record key is the URL
//! without it, and a response that echoes the key is discarded
//! ([`crate::http::FetchError::CredentialEchoed`]).

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "eia";
pub const API: &str = "https://api.eia.gov/v2";

/// The API route that serves each supported series.
pub fn route_of(series: &str) -> Option<&'static str> {
    match series {
        "RWTC" | "RBRTE" => Some("petroleum/pri/spt"),
        "RNGWHHD" => Some("natural-gas/pri/fut"),
        _ => None,
    }
}

/// The request URL for `series` (all on `route`), newest first, without the
/// API key. The key is appended by [`fetch_series`].
pub fn series_url(route: &str, series: &[&str]) -> String {
    let facets: String = series
        .iter()
        .map(|s| format!("&facets[series][]={s}"))
        .collect();
    format!(
        "{API}/{route}/data/?frequency=daily&data[0]=value{facets}\
         &sort[0][column]=period&sort[0][direction]=desc&length={}",
        series.len() * 10
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Response {
    pub response: Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Body {
    pub data: Vec<Point>,
}

/// One daily value. `value` is kept as its exact text; EIA sends it as a
/// JSON number or a string.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Point {
    /// `YYYY-MM-DD`.
    pub period: String,
    pub series: String,
    pub value: Option<NumberText>,
    pub units: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberText(pub String);

impl<'de> Deserialize<'de> for NumberText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw: Box<serde_json::value::RawValue> = Deserialize::deserialize(d)?;
        let text = raw.get();
        match text.bytes().next() {
            Some(b'"') => serde_json::from_str::<String>(text)
                .map(NumberText)
                .map_err(serde::de::Error::custom),
            Some(b'-' | b'0'..=b'9') => Ok(NumberText(text.to_owned())),
            _ => Err(serde::de::Error::custom("expected a number or a string")),
        }
    }
}

pub struct EiaProvider {
    source_id: SourceId,
}

impl EiaProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }
}

impl Default for EiaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for EiaProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for EiaProvider {
    type Quote = Response;

    fn decode_quote(&self, payload: &[u8]) -> Result<Response, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// Fetches `series` (all on one route) with `api_key` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_series(
    client: &crate::http::HttpClient,
    api_key: &str,
    route: &str,
    series: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    let key = series_url(route, series);
    let url = format!("{key}&api_key={api_key}");
    client.get_secret_query(&url, &key, api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_numbers_and_strings_exactly() {
        let r = EiaProvider::new()
            .decode_quote(
                br#"{"response":{"total":"2","data":[
                    {"period":"2026-09-15","series":"RWTC","value":65.12,"units":"$/BBL"},
                    {"period":"2026-09-15","series":"RNGWHHD","value":"2.770","units":"$/MMBTU"},
                    {"period":"2026-09-14","series":"RBRTE","value":null,"units":"$/BBL"}]},
                    "apiVersion":"2.1.0"}"#,
            )
            .unwrap();
        let values: Vec<Option<&str>> = r
            .response
            .data
            .iter()
            .map(|p| p.value.as_ref().map(|v| v.0.as_str()))
            .collect();
        assert_eq!(values, vec![Some("65.12"), Some("2.770"), None]);
        assert_eq!(route_of("RWTC"), Some("petroleum/pri/spt"));
        assert_eq!(route_of("XYZ"), None);
        let url = series_url("petroleum/pri/spt", &["RWTC", "RBRTE"]);
        assert!(url.contains("facets[series][]=RBRTE") && !url.contains("api_key"));
    }
}
