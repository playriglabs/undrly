//! Bank Negara Malaysia Open API, exchange rates
//! (`GET /public/exchange-rate?session=1700&quote=rm`, header
//! `Accept: application/vnd.BNM.API.v1+json`). No authentication.
//!
//! `quote=rm`: ringgit (MYR) per `unit` units of each currency (1, or 100
//! for currencies such as JPY and IDR), from the interbank market at the
//! stated session (`1700` = 17:00 Malaysia time, UTC+8) of each business
//! day. The middle rate is the reference; buying and selling rates are not
//! exposed as a bid/ask. Numbers are JSON doubles (`4.0700000000000003`),
//! kept as their exact text.

use serde::Deserialize;

use crate::JsonNumber;

pub const SOURCE_ID: &str = "bnm";
pub const RATES_URL: &str = "https://api.bnm.gov.my/public/exchange-rate?session=1700&quote=rm";
pub const ACCEPT: &str = "application/vnd.BNM.API.v1+json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rates {
    pub data: Vec<Rate>,
    pub meta: Meta,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Meta {
    /// `rm`: MYR per unit of the currency.
    pub quote: String,
    /// `HHMM` Malaysia time, e.g. `1700`.
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rate {
    pub currency_code: String,
    pub unit: u32,
    pub rate: RateValues,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RateValues {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub middle_rate: Option<JsonNumber>,
}

/// `/public/exchange-rate/{CCY}/year/{Y}/month/{M}?session=1700&quote=rm`:
/// one currency's rates for each business day of a month (history).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MonthRates {
    pub data: MonthData,
    pub meta: Meta,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MonthData {
    pub currency_code: String,
    pub unit: u32,
    pub rate: Vec<RateValues>,
}

pub fn month_url(currency: &str, year: i32, month: u32) -> String {
    format!(
        "https://api.bnm.gov.my/public/exchange-rate/{currency}/year/{year}/month/{month}?session=1700&quote=rm"
    )
}

fn decode_month(payload: &[u8]) -> Result<MonthRates, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(BnmMonthProvider, MonthRates, decode_month);

/// Fetches one currency-month (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_month(
    client: &crate::http::HttpClient,
    url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get_accepting(url, ACCEPT, &[]).await
}

fn decode(payload: &[u8]) -> Result<Rates, String> {
    serde_json::from_slice(payload).map_err(|e| e.to_string())
}

crate::quote_provider!(BnmProvider, Rates, decode);

/// Fetches the 17:00 session rates (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_rates(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get_accepting(RATES_URL, ACCEPT, &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuoteProvider;

    #[test]
    fn decodes_captured_rates_verbatim() {
        let r = BnmProvider::new()
            .decode_quote(&crate::fixture("bnm/exchange-rate-1700.json"))
            .unwrap();
        assert_eq!(
            (r.meta.quote.as_str(), r.meta.session.as_str()),
            ("rm", "1700")
        );
        let usd = r.data.iter().find(|x| x.currency_code == "USD").unwrap();
        assert_eq!(usd.unit, 1);
        assert_eq!(usd.rate.date, "2026-09-25");
        assert_eq!(
            usd.rate.middle_rate,
            Some(JsonNumber("4.0735000000000001".into()))
        );
        let idr = r.data.iter().find(|x| x.currency_code == "IDR").unwrap();
        assert_eq!(idr.unit, 100);
        assert!(BnmProvider::new().decode_quote(br#"{"data":[]}"#).is_err());
    }
}
