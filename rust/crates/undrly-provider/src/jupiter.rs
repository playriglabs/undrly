//! Jupiter Price API v3 (`GET https://lite-api.jup.ag/price/v3?ids=<mints>`):
//! USD prices of Solana tokens, keyed by **mint address**
//! (docs/v1.10-tokenized-stocks.md §6).
//!
//! `usdPrice` is Jupiter's price of the token itself, derived from onchain
//! trading across Solana venues: an aggregated price with no venue. It is
//! absent for tokens without enough liquidity; those are simply not
//! returned. Other fields are not read:
//!
//! - `stockData.price` is the **underlying** stock's price as the issuer
//!   reports it, not the token's market;
//! - `scaledUiConfig.usdPricePrescaled` is a price per prescaled unit of a
//!   scaled-UI token; Undrly records `usdPrice` only, as Jupiter states it.
//!
//! The feed symbol is the mint; a mint is case-sensitive base58 and is
//! never rewritten.

use std::collections::BTreeMap;

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, JsonNumber, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "jupiter";
pub const PRICE_URL: &str = "https://lite-api.jup.ag/price/v3";
/// Jupiter's maximum number of ids per request.
pub const MAX_IDS: usize = 50;

/// The response: mint → price, for the mints Jupiter prices.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Prices(pub BTreeMap<String, Price>);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    /// Absent without enough liquidity.
    pub usd_price: Option<JsonNumber>,
    pub block_id: Option<u64>,
}

pub struct JupiterProvider {
    source_id: SourceId,
}

impl JupiterProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    pub fn price_url(mints: &[&str]) -> String {
        format!("{PRICE_URL}?ids={}", mints.join(","))
    }
}

impl Default for JupiterProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for JupiterProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for JupiterProvider {
    type Quote = Prices;

    fn decode_quote(&self, payload: &[u8]) -> Result<Prices, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// Fetches the prices of up to [`MAX_IDS`] mints (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_prices(
    client: &crate::http::HttpClient,
    mints: &[&str],
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(&JupiterProvider::price_url(mints), &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_usd_price_text_and_skips_unpriced_mints() {
        let p = JupiterProvider::new()
            .decode_quote(
                br#"{"XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp":{"createdAt":"2025-06-10T11:06:56Z",
                     "liquidity":535159.97,"usdPrice":332.47579840803337,"blockId":453280210,"decimals":8,
                     "stockData":{"id":"xstocks","price":333.6}},
                     "NVDAVuiB7hwd3m5Wa1JuHNovPaPG6BH1QNztbKFxNjv":{"liquidity":255.4,"decimals":6}}"#,
            )
            .unwrap();
        let a = &p.0["XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp"];
        assert_eq!(
            a.usd_price.as_ref().map(|n| n.0.as_str()),
            Some("332.47579840803337")
        );
        assert_eq!(
            p.0["NVDAVuiB7hwd3m5Wa1JuHNovPaPG6BH1QNztbKFxNjv"].usd_price,
            None
        );
        assert_eq!(
            JupiterProvider::price_url(&["a", "b"]),
            "https://lite-api.jup.ag/price/v3?ids=a,b"
        );
    }
}
