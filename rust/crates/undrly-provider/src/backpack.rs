//! Backpack Exchange public API: Backpack Securities' tokenized securities on
//! Solana (docs/v1.10-tokenized-stocks.md §5).
//!
//! - `GET /api/v1/securities`: the securities Backpack Securities offers, per
//!   asset (`AAPL.US`) its name and, for some, a CUSIP.
//! - `GET /api/v1/assets`: every asset with its tokens; a security's token
//!   on Solana is its `contractAddress` (the mint).
//!
//! The CUSIP is a build-local matching key only; it is never stored or
//! exposed as an identifier.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "backpack";
pub const SECURITIES_URL: &str = "https://api.backpack.exchange/api/v1/securities";
pub const ASSETS_URL: &str = "https://api.backpack.exchange/api/v1/assets";
/// The API's blockchain name for Solana mainnet.
pub const SOLANA_BLOCKCHAIN: &str = "Solana";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Security {
    pub asset: String,
    pub name: String,
    pub cusip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub symbol: String,
    pub display_name: Option<String>,
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub blockchain: String,
    pub contract_address: Option<String>,
}

fn reject(what: &str, e: impl std::fmt::Display) -> DecodeError {
    DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: format!("{what}: {e}"),
    }
}

pub fn decode_securities(payload: &[u8]) -> Result<Vec<Security>, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| reject("securities", e))
}

pub fn decode_assets(payload: &[u8]) -> Result<Vec<Asset>, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| reject("assets", e))
}

/// The listing symbol a security's asset names: `AAPL.US` → `AAPL`,
/// `BRK.B.US` → `BRK.B`. Used to check a CUSIP match, never to identify.
pub fn us_symbol(asset: &str) -> Option<&str> {
    asset.strip_suffix(".US").filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_securities_and_assets() {
        let s = decode_securities(
            br#"[{"asset":"AAPL.US","cusip":"037833100","name":"Apple Inc.","sessions":[]},
                 {"asset":"SPCX.US","cusip":null,"name":"SpaceX","sessions":[]}]"#,
        )
        .unwrap();
        assert_eq!(s[0].cusip.as_deref(), Some("037833100"));
        assert_eq!(s[1].cusip, None);
        let a = decode_assets(
            br#"[{"coingeckoId":null,"displayName":"Apple Inc.","symbol":"AAPL.US","tokens":[
                 {"blockchain":"Solana","contractAddress":"AAPLEDt8RpzPgXyhvFzkMBofvFSQw9gpeMCoUdPdLnB8",
                  "depositEnabled":false,"nativeDecimals":6}]}]"#,
        )
        .unwrap();
        assert_eq!(a[0].tokens[0].blockchain, "Solana");
        assert_eq!(us_symbol("BRK.B.US"), Some("BRK.B"));
        assert_eq!(us_symbol("BTC"), None);
        assert!(decode_securities(b"{}").is_err());
    }
}
