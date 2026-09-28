//! Hyperliquid perpetuals (`POST /info {"type":"metaAndAssetCtxs"}`).
//!
//! The response is `[meta, contexts]`: `meta.universe[i]` names the market
//! whose context is `contexts[i]`. Every listed perpetual is returned; feeds
//! select what Undrly uses. No timestamp is stated.
//!
//! Units (Hyperliquid's contract specification): prices (`markPx`,
//! `oraclePx`, `midPx`, `prevDayPx`, impact prices) are in the contract's
//! denomination: USDT for most perpetuals, USDC for the documented
//! exceptions. Margin, profit and loss and funding are paid in the dex's
//! collateral token (`collateralToken`; spot token 0, USDC, for the first
//! perp dex) without USDC/USDT conversion ("quanto").

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, QuoteProvider};

pub const SOURCE_ID: &str = "hyperliquid";
pub const INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const META_AND_ASSET_CTXS: &str = r#"{"type":"metaAndAssetCtxs"}"#;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Meta {
    pub universe: Vec<Asset>,
    /// The perp dex's collateral: an index into the spot token list
    /// (`spotMeta`), absent in older responses.
    #[serde(default, rename = "collateralToken")]
    pub collateral_token: Option<u32>,
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
    /// Hourly funding rate, a fraction (the venue pays funding every hour).
    pub funding: Option<String>,
    /// Open interest in the market's base units (contracts).
    pub open_interest: Option<String>,
    /// The price 24 hours ago.
    pub prev_day_px: Option<String>,
    /// Trailing 24-hour notional volume: contracts × price, in the
    /// contract's price denomination (not converted to the collateral).
    pub day_ntl_vlm: Option<String>,
    /// Trailing 24-hour volume in base units (contracts).
    pub day_base_vlm: Option<String>,
}

/// `(meta, contexts)`, with one context per universe entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaAndAssetCtxs {
    pub universe: Vec<Asset>,
    pub contexts: Vec<AssetContext>,
    /// See [`Meta::collateral_token`].
    pub collateral_token: Option<u32>,
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
            collateral_token: meta.collateral_token,
        })
    }
}

/// One candle of `candleSnapshot`: trade prices, `t` start and `T` end
/// (inclusive, ms), `v` volume in base units (contracts), `n` trades.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Candle {
    pub t: i64,
    #[serde(rename = "T")]
    pub end: i64,
    pub s: String,
    pub i: String,
    pub o: String,
    pub h: String,
    pub l: String,
    pub c: String,
    pub v: String,
    pub n: u64,
}

/// `candleSnapshot` request body (the record key names it).
pub fn candle_snapshot_body(coin: &str, interval: &str, start_ms: i64, end_ms: i64) -> String {
    format!(
        r#"{{"type":"candleSnapshot","req":{{"coin":"{coin}","interval":"{interval}","startTime":{start_ms},"endTime":{end_ms}}}}}"#
    )
}

impl crate::BarsProvider for HyperliquidProvider {
    type Bars = Vec<Candle>;

    fn decode_bars(&self, payload: &[u8]) -> Result<Vec<Candle>, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

/// Fetches candles of one coin (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_candles(
    client: &crate::http::HttpClient,
    coin: &str,
    interval: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .post_json(
            INFO_URL,
            &candle_snapshot_body(coin, interval, start_ms, end_ms),
        )
        .await
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
    fn decodes_candles_and_contexts() {
        use crate::BarsProvider;
        let read = |f: &str| {
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../tests/fixtures/sources/hyperliquid")
                    .join(f),
            )
            .unwrap()
        };
        let c = HyperliquidProvider::new()
            .decode_bars(&read("candles-BTC-1h.json"))
            .unwrap();
        assert_eq!(c.len(), 5);
        assert_eq!((c[0].s.as_str(), c[0].i.as_str()), ("BTC", "1h"));
        assert_eq!(c[0].end - c[0].t, 3_599_999);
        let ctx = HyperliquidProvider::new()
            .decode_quote(&read("metaAndAssetCtxs.json"))
            .unwrap();
        let btc = ctx.context("BTC").unwrap();
        assert_eq!(btc.open_interest.as_deref(), Some("39202.60446"));
        assert_eq!(btc.funding.as_deref(), Some("0.0000125"));
        assert_eq!(
            candle_snapshot_body("BTC", "1h", 1, 2),
            r#"{"type":"candleSnapshot","req":{"coin":"BTC","interval":"1h","startTime":1,"endTime":2}}"#
        );
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
