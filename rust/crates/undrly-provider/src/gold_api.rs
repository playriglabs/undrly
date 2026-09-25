//! gold-api.com spot price (`GET /price/XAU`).
//!
//! An aggregated reference price (no venue) in the stated currency, with the
//! source's update time. The price is a JSON number and is kept as its exact
//! text.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, JsonNumber, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "gold-api";
pub const BASE_URL: &str = "https://api.gold-api.com/price";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub symbol: String,
    pub currency: String,
    pub price: JsonNumber,
    pub updated_at: String,
}

pub struct GoldApiProvider {
    source_id: SourceId,
}

impl GoldApiProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    pub fn price_url(symbol: &str) -> String {
        format!("{BASE_URL}/{symbol}")
    }
}

impl Default for GoldApiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GoldApiProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for GoldApiProvider {
    type Quote = Price;

    fn decode_quote(&self, payload: &[u8]) -> Result<Price, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// Fetches the price of `symbol` (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_price(
    client: &crate::http::HttpClient,
    symbol: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&GoldApiProvider::price_url(symbol), &[]).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn keeps_the_json_number_text() {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/gold-api/price-XAU.json"),
        )
        .unwrap();
        let p = GoldApiProvider::new().decode_quote(&payload).unwrap();
        assert_eq!(
            (p.symbol.as_str(), p.currency.as_str(), p.price.0.as_str()),
            ("XAU", "USD", "4273.600098")
        );
    }

    #[test]
    fn rejects_a_string_price() {
        let payload =
            br#"{"symbol":"XAU","currency":"USD","price":"4273.6","updatedAt":"2026-09-25T03:51:57Z"}"#;
        assert!(GoldApiProvider::new().decode_quote(payload).is_err());
    }
}
