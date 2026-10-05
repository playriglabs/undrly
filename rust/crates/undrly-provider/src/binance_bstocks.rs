//! Binance bStocks (V1.10, docs/v1.10-tokenized-stocks.md §5): tokenized
//! certificates issued by BTech Holdings Ltd (ADGM) on BNB Chain, each with
//! a Binance spot market.
//!
//! `GET https://www.binance.com/bapi/defi/v1/public/wallet-direct/buw/wallet/market/token/rwa/stock/detail/list/ai?type=3`:
//! Binance's public, unauthenticated list of its tokenized stocks (type 3
//! is bStocks), per token: chain id, contract address, token symbol
//! (`AAPLB`), the share's ticker (`AAPL`), and the spot market (`cs`,
//! `AAPLBUSDT`). It is the web backend's list, not part of Binance's
//! documented API. It states no ISIN or CUSIP, so a bStock is **never**
//! tied to a share from it (the ticker is not an identifier).

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "binance-bstocks";
pub const LIST_URL: &str = "https://www.binance.com/bapi/defi/v1/public/wallet-direct/buw/wallet/market/token/rwa/stock/detail/list/ai?type=3";
/// The list's `type` for bStocks.
pub const BSTOCK_TYPE: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct List {
    pub code: String,
    pub data: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// EIP-155 chain id as text (`"56"`).
    pub chain_id: String,
    pub contract_address: String,
    pub symbol: String,
    /// The share's ticker as Binance states it; not used as identity.
    pub ticker: String,
    #[serde(rename = "type")]
    pub kind: u32,
    /// The spot market's base asset.
    pub asset: Option<String>,
    /// The spot market symbol.
    pub cs: Option<String>,
}

pub fn decode_list(payload: &[u8]) -> Result<List, DecodeError> {
    let reject = |reason: String| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason,
    };
    let list: List = serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
    if list.code != "000000" {
        return Err(reject(format!("response code {}", list.code)));
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_list() {
        let l = decode_list(
            br#"{"code":"000000","message":null,"data":[{"chainId":"56",
                 "contractAddress":"0xcdf2f3e0fa43c47a6662a91c9e4a7c5f69762699","symbol":"MUB","ticker":"MU",
                 "type":3,"assetType":1,"multiplier":"1","cs":"MUBUSDT","asset":"MUB","d":18}]}"#,
        )
        .unwrap();
        assert_eq!(l.data[0].cs.as_deref(), Some("MUBUSDT"));
        assert_eq!(l.data[0].kind, BSTOCK_TYPE);
        assert!(decode_list(br#"{"code":"100001","data":[]}"#).is_err());
    }
}
