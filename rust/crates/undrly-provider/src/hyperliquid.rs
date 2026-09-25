//! Hyperliquid perpetuals (`POST /info {"type":"metaAndAssetCtxs"}`).
//!
//! The response is `[meta, contexts]`: `meta.universe[i]` names the market
//! whose context is `contexts[i]`. Every listed perpetual is returned; feeds
//! select what Undrly uses. No timestamp is stated.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "hyperliquid";
pub const INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const META_AND_ASSET_CTXS: &str = r#"{"type":"metaAndAssetCtxs"}"#;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Meta {
    pub universe: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    /// Delisted markets stay in `meta` but are not live.
    #[serde(default, rename = "isDelisted")]
    pub is_delisted: bool,
}

/// One market's context. Prices are decimal strings, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetContext {
    pub mark_px: Option<String>,
    pub mid_px: Option<String>,
    pub oracle_px: Option<String>,
    pub funding: Option<String>,
}

/// `(meta, contexts)`, with one context per universe entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaAndAssetCtxs {
    pub universe: Vec<Asset>,
    pub contexts: Vec<AssetContext>,
}

impl MetaAndAssetCtxs {
    pub fn context(&self, name: &str) -> Option<&AssetContext> {
        let i = self.universe.iter().position(|a| a.name == name)?;
        self.contexts.get(i)
    }
}

pub struct HyperliquidProvider {
    source_id: SourceId,
}

impl HyperliquidProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }
}

impl Default for HyperliquidProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for HyperliquidProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl QuoteProvider for HyperliquidProvider {
    type Quote = MetaAndAssetCtxs;

    fn decode_quote(&self, payload: &[u8]) -> Result<MetaAndAssetCtxs, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let (meta, contexts): (Meta, Vec<AssetContext>) =
            serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
        if meta.universe.len() != contexts.len() {
            return Err(reject(format!(
                "{} markets but {} contexts",
                meta.universe.len(),
                contexts.len()
            )));
        }
        Ok(MetaAndAssetCtxs {
            universe: meta.universe,
            contexts,
        })
    }
}

/// Fetches all perpetual contexts (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_meta_and_asset_ctxs(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.post_json(INFO_URL, META_AND_ASSET_CTXS).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn decodes_captured_contexts() {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/hyperliquid/metaAndAssetCtxs.json"),
        )
        .unwrap();
        let d = HyperliquidProvider::new().decode_quote(&payload).unwrap();
        assert_eq!(d.universe[0].name, "BTC");
        assert!(d.context("BTC").unwrap().mark_px.is_some());
        assert!(d.context("NOPE").is_none());
    }

    #[test]
    fn rejects_mismatched_or_malformed_payloads() {
        for payload in [
            &br#"[{"universe":[{"name":"BTC"}]},[]]"#[..],
            br#"{"universe":[]}"#,
            b"null",
        ] {
            assert!(HyperliquidProvider::new().decode_quote(payload).is_err());
        }
    }
}
