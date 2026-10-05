//! GeckoTerminal public API (V1.10, docs/v1.10-tokenized-stocks.md §13):
//! onchain pool OHLC for Solana tokens that have no venue bars.
//!
//! - `GET /api/v2/networks/solana/tokens/{mint}/pools?page=1`: a token's
//!   pools (up to 20), each with its DEX, base and quote token and USD
//!   reserve (`universe fetch`). (`tokens/multi` names one top pool, by
//!   volume, and often none: not used.)
//! - `GET /api/v2/networks/solana/dexes?page=N`: DEX ids and names.
//! - `GET /api/v2/networks/solana/pools/{pool}/ohlcv/{hour|day}?token=…`:
//!   one pool's bars, newest first, priced in USD for the named side
//!   (`history bars`). Prices are JSON numbers, kept as written.
//!
//! Free tier, no key: documented as 30 requests a minute, answered with
//! HTTP 429 above about 10 (observed 2026-10-05).

use serde::Deserialize;

use crate::JsonNumber;

pub const SOURCE_ID: &str = "geckoterminal";
const BASE_URL: &str = "https://api.geckoterminal.com/api/v2/networks/solana";

/// Spacing between requests: 10 a minute.
pub const REQUEST_SPACING_MS: u64 = 6_000;

pub fn pools_url(mint: &str) -> String {
    format!("{BASE_URL}/tokens/{mint}/pools?page=1")
}

pub fn dexes_url(page: u32) -> String {
    format!("{BASE_URL}/dexes?page={page}")
}

/// Which side of the pool the priced token is: `b:<pool>` or `q:<pool>`
/// (the feed symbol; at most 46 characters).
pub fn feed_symbol(pool: &str, mint_is_base: bool) -> String {
    format!("{}:{pool}", if mint_is_base { "b" } else { "q" })
}

/// `(pool, side)` from a feed symbol; `None` if it is not one.
pub fn parse_feed_symbol(symbol: &str) -> Option<(&str, &'static str)> {
    let (side, pool) = symbol.split_once(':')?;
    match side {
        "b" => Some((pool, "base")),
        "q" => Some((pool, "quote")),
        _ => None,
    }
}

/// `timeframe` is `hour` or `day`; up to `limit` bars (at most 1,000)
/// ending before `before` (Unix seconds), newest first.
pub fn ohlcv_url(symbol: &str, timeframe: &str, limit: u32, before: Option<i64>) -> Option<String> {
    let (pool, side) = parse_feed_symbol(symbol)?;
    let before = before.map_or(String::new(), |b| format!("&before_timestamp={b}"));
    Some(format!(
        "{BASE_URL}/pools/{pool}/ohlcv/{timeframe}?aggregate=1&limit={}&currency=usd&token={side}{before}",
        limit.min(1000)
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ref {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct One {
    pub data: Option<Ref>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PoolAttributes {
    pub address: String,
    /// USD liquidity, decimal text (`null` when unknown).
    pub reserve_in_usd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PoolRelationships {
    pub dex: One,
    pub base_token: One,
    pub quote_token: One,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pool {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub attributes: PoolAttributes,
    pub relationships: PoolRelationships,
}

/// One token's pools.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pools {
    #[serde(default)]
    pub data: Vec<Pool>,
}

pub fn decode_pools(payload: &[u8]) -> Result<Pools, String> {
    serde_json::from_slice(payload).map_err(|e| format!("pools: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DexAttributes {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Dex {
    pub id: String,
    pub attributes: DexAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Dexes {
    #[serde(default)]
    pub data: Vec<Dex>,
}

pub fn decode_dexes(payload: &[u8]) -> Result<Dexes, String> {
    serde_json::from_slice(payload).map_err(|e| format!("dexes: {e}"))
}

/// One bar: start (Unix seconds), open, high, low, close, USD volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub open_time: i64,
    pub open: JsonNumber,
    pub high: JsonNumber,
    pub low: JsonNumber,
    pub close: JsonNumber,
}

/// One pool's bars, newest first as served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ohlcv(pub Vec<Bar>);

pub fn decode_ohlcv(payload: &[u8]) -> Result<Ohlcv, String> {
    #[derive(Deserialize)]
    struct Attributes {
        ohlcv_list: Vec<(
            i64,
            JsonNumber,
            JsonNumber,
            JsonNumber,
            JsonNumber,
            JsonNumber,
        )>,
    }
    #[derive(Deserialize)]
    struct Data {
        attributes: Attributes,
    }
    #[derive(Deserialize)]
    struct Response {
        data: Data,
    }
    let r: Response = serde_json::from_slice(payload).map_err(|e| format!("ohlcv: {e}"))?;
    Ok(Ohlcv(
        r.data
            .attributes
            .ohlcv_list
            .into_iter()
            .map(|(t, o, h, l, c, _)| Bar {
                open_time: t,
                open: o,
                high: h,
                low: l,
                close: c,
            })
            .collect(),
    ))
}

pub struct GeckoTerminalProvider {
    source_id: undrly_core::SourceId,
}

impl GeckoTerminalProvider {
    pub fn new() -> Self {
        Self {
            source_id: undrly_core::SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }
}

impl Default for GeckoTerminalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Provider for GeckoTerminalProvider {
    fn source_id(&self) -> &undrly_core::SourceId {
        &self.source_id
    }
}

crate::bars_provider!(GeckoTerminalProvider, Ohlcv, decode_ohlcv);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_pools_and_bars_as_written() {
        let t = decode_pools(
            br#"{"data":[{"id":"solana_CXio","type":"pool","attributes":{"address":"CXio","reserve_in_usd":"61878.6817"},
                   "relationships":{"dex":{"data":{"id":"raydium-clmm","type":"dex"}},"base_token":{"data":{"id":"solana_AMD8","type":"token"}},"quote_token":{"data":{"id":"solana_So11","type":"token"}}}}]}"#,
        )
        .unwrap();
        assert_eq!(t.data[0].attributes.address, "CXio");
        assert_eq!(
            t.data[0].relationships.dex.data.as_ref().unwrap().id,
            "raydium-clmm"
        );
        let o = decode_ohlcv(
            br#"{"data":{"id":"x","type":"ohlcv_request_response","attributes":{"ohlcv_list":[[1791129600,630.0442851396282,633.55,626.9,628.5667555873118,1206.03]]}}}"#,
        )
        .unwrap();
        assert_eq!(o.0[0].open_time, 1791129600);
        assert_eq!(o.0[0].open.0, "630.0442851396282");
        assert_eq!(
            ohlcv_url("b:CXio", "hour", 5000, None).unwrap(),
            "https://api.geckoterminal.com/api/v2/networks/solana/pools/CXio/ohlcv/hour?aggregate=1&limit=1000&currency=usd&token=base"
        );
        assert_eq!(parse_feed_symbol("q:CXio"), Some(("CXio", "quote")));
        assert_eq!(parse_feed_symbol("CXio"), None);
    }
}
