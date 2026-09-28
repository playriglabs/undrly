//! Robinhood Assets (Jersey) Limited ("RHJ"), issuer of Robinhood Stock
//! Tokens (V1.6, docs/v1.6-robinhood-chain.md).
//!
//! Two upstreams, two sources:
//!
//! - `rhj-api`: `GET https://api.robinhood.com/rhj/assets`, the issuer's asset
//!   registry: per asset its token symbol and name, its deployments
//!   (`contractAddress`, `chainId`) and, undocumented, `isin`, which for every
//!   asset checked is the **Underlying's** ISIN (as the asset's Final Terms
//!   state it), not the product's.
//! - `rhj-final-terms`: a product's Final Terms (PDF). Undrly does not parse
//!   PDFs: the bytes are stored raw and checked against a reviewed hash; see
//!   `undrly_ingest::tracker`.
//!
//! Decoding keeps the registry verbatim and ignores fields Undrly does not
//! use (the registry adds fields over time).

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider};

pub const API_SOURCE_ID: &str = "rhj-api";
pub const FINAL_TERMS_SOURCE_ID: &str = "rhj-final-terms";
pub const ASSETS_URL: &str = "https://api.robinhood.com/rhj/assets";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assets {
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// RHJ's onchain uid for the asset (same on every chain).
    pub id: String,
    pub token_symbol: String,
    pub token_name: String,
    pub deployments: Vec<Deployment>,
    pub status: String,
    /// Undocumented; the Underlying's ISIN (see the module docs).
    #[serde(default)]
    pub isin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    /// EIP-55 checksummed, as published.
    pub contract_address: String,
    /// EIP-155 chain id.
    pub chain_id: u64,
}

pub struct RhjProvider {
    source_id: SourceId,
}

impl RhjProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(API_SOURCE_ID).expect("valid source id"),
        }
    }

    pub fn decode_assets(&self, payload: &[u8]) -> Result<Assets, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

impl Default for RhjProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for RhjProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

/// Fetches the asset registry (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_assets(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get(ASSETS_URL, &[]).await
}

/// Fetches a product's Final Terms (PDF) exactly as served (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_final_terms(
    client: &crate::http::HttpClient,
    url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.get_accepting(url, "application/pdf", &[]).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn decodes_the_registry_with_underlying_isins() {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/rhj/assets.json"),
        )
        .unwrap();
        let a = RhjProvider::new().decode_assets(&payload).unwrap();
        assert_eq!(a.assets.len(), 195);
        let nvda: Vec<&Asset> = a
            .assets
            .iter()
            .filter(|x| x.isin.as_deref() == Some("US67066G1040"))
            .collect();
        assert_eq!(nvda.len(), 1);
        assert_eq!(nvda[0].token_symbol, "NVDA");
        assert_eq!(
            nvda[0].deployments,
            vec![Deployment {
                contract_address: "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC".into(),
                chain_id: 4663,
            }]
        );
        assert_eq!(nvda[0].status, "ASSET_STATUS_ACTIVE");
    }
}
