//! An EVM chain's identity from the chain itself (V1.6).
//!
//! `POST <rpc> {"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}`
//! returns the chain's EIP-155 chain id as a hex quantity (`"0x1237"`). The
//! CAIP-2 id is `eip155:<decimal chain id>`. Which chain a response describes
//! is decided by the id it returns, never by the URL.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider};

pub const ETH_CHAIN_ID: &str = r#"{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}"#;

/// Robinhood Chain's public mainnet RPC (docs.robinhood.com/chain/connecting).
pub const ROBINHOOD_CHAIN_SOURCE_ID: &str = "robinhood-chain-rpc";
pub const ROBINHOOD_CHAIN_MAINNET_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: String,
}

/// `eth_chainId` of one chain's RPC, under that chain's source id.
pub struct EvmRpcProvider {
    source_id: SourceId,
}

impl EvmRpcProvider {
    pub fn new(source_id: &str) -> Result<Self, undrly_core::source::SourceIdError> {
        Ok(Self {
            source_id: SourceId::parse(source_id)?,
        })
    }

    /// The chain id. A quantity is `0x` and lowercase hex without leading
    /// zeros (JSON-RPC); anything else, or an RPC error, is rejected.
    pub fn decode_chain_id(&self, payload: &[u8]) -> Result<u64, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let r: Response = serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
        let hex = r
            .result
            .strip_prefix("0x")
            .filter(|h| {
                !h.is_empty()
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && (h.len() == 1 || !h.starts_with('0'))
            })
            .ok_or_else(|| reject(format!("not a hex quantity: {}", r.result)))?;
        u64::from_str_radix(hex, 16).map_err(|e| reject(e.to_string()))
    }
}

/// Tempo Mainnet's public RPC (tempo.xyz/developers/docs/quickstart/connection-details).
pub const BNB_CHAIN_SOURCE_ID: &str = "bnb-chain-rpc";
/// BNB Chain's public mainnet RPC (docs.bnbchain.org, JSON-RPC endpoints).
pub const BNB_CHAIN_MAINNET_RPC: &str = "https://bsc-dataseed.bnbchain.org";

pub const TEMPO_SOURCE_ID: &str = "tempo-rpc";
pub const TEMPO_MAINNET_RPC: &str = "https://rpc.tempo.xyz";

/// Function selectors (Keccak-256 of the signature, first 4 bytes).
const NAME: &str = "0x06fdde03";
const SYMBOL: &str = "0x95d89b41";
/// `currency()`: TIP-20's reference-asset declaration.
const CURRENCY: &str = "0xe5a6b10f";
const DECIMALS: &str = "0x313ce567";

/// Reads a TIP-20 token's metadata and the chain id in one batch (feature
/// `http`).
#[cfg(feature = "http")]
pub async fn fetch_tip20_metadata(
    client: &crate::http::HttpClient,
    rpc_url: &str,
    address: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .post_json_keyed(
            rpc_url,
            &tip20_metadata_body(address),
            &tip20_metadata_key(rpc_url, address),
        )
        .await
}

/// The record key of [`tip20_metadata_body`] for `address`: the batch is
/// fully determined by it (chain id, then name, symbol, currency, decimals).
/// The body itself is too long to be a record key.
pub fn tip20_metadata_key(rpc_url: &str, address: &str) -> String {
    format!("POST {rpc_url} tip20-metadata[chainId,name,symbol,currency,decimals] {address}")
}

/// A TIP-20 token's identity metadata read from the chain, together with the
/// chain id of the node that answered (one batched request, one record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip20Metadata {
    pub chain_id: u64,
    pub name: String,
    pub symbol: String,
    pub currency: String,
    pub decimals: u8,
}

/// The batched request for `address` (lowercase `0x` hex): `eth_chainId`,
/// then `name`, `symbol`, `currency`, `decimals` at the latest block.
pub fn tip20_metadata_body(address: &str) -> String {
    let call = |id: u8, data: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"eth_call","params":[{{"to":"{address}","data":"{data}"}},"latest"]}}"#
        )
    };
    format!(
        r#"[{{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}},{},{},{},{}]"#,
        call(2, NAME),
        call(3, SYMBOL),
        call(4, CURRENCY),
        call(5, DECIMALS)
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchItem {
    #[allow(dead_code)]
    jsonrpc: String,
    id: u8,
    result: String,
}

fn hex_bytes(h: &str) -> Option<Vec<u8>> {
    let h = h.strip_prefix("0x")?;
    if h.len() % 2 != 0 {
        return None;
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok())
        .collect()
}

/// An ABI-encoded `string` return value: offset 32, length, UTF-8 bytes.
fn abi_string(h: &str) -> Option<String> {
    let b = hex_bytes(h)?;
    let word = |i: usize| -> Option<usize> {
        let w = b.get(i..i + 32)?;
        if w[..24].iter().any(|&x| x != 0) {
            return None;
        }
        Some(u64::from_be_bytes(w[24..].try_into().ok()?) as usize)
    };
    let offset = word(0)?;
    let len = word(offset)?;
    let start = offset.checked_add(32)?;
    String::from_utf8(b.get(start..start.checked_add(len)?)?.to_vec()).ok()
}

/// An ABI-encoded `uint8` return value.
fn abi_u8(h: &str) -> Option<u8> {
    let b = hex_bytes(h)?;
    if b.len() != 32 || b[..31].iter().any(|&x| x != 0) {
        return None;
    }
    Some(b[31])
}

impl EvmRpcProvider {
    /// Decodes the batched TIP-20 metadata response. Every call must have
    /// succeeded; an error or a malformed value rejects the whole record.
    pub fn decode_tip20_metadata(&self, payload: &[u8]) -> Result<Tip20Metadata, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let items: Vec<BatchItem> =
            serde_json::from_slice(payload).map_err(|e| reject(e.to_string()))?;
        let result = |id: u8| -> Result<&str, DecodeError> {
            items
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.result.as_str())
                .ok_or_else(|| reject(format!("no result for request {id}")))
        };
        let chain_id = self.decode_chain_id(
            format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{}"}}"#, result(1)?).as_bytes(),
        )?;
        let text = |id: u8, what: &str| {
            result(id).and_then(|h| {
                abi_string(h).ok_or_else(|| reject(format!("{what}: not an ABI string")))
            })
        };
        Ok(Tip20Metadata {
            chain_id,
            name: text(2, "name")?,
            symbol: text(3, "symbol")?,
            currency: text(4, "currency")?,
            decimals: abi_u8(result(5)?).ok_or_else(|| reject("decimals: not a uint8".into()))?,
        })
    }
}

impl Provider for EvmRpcProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

/// Asks the chain at `rpc_url` for its chain id (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_chain_id(
    client: &crate::http::HttpClient,
    rpc_url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.post_json(rpc_url, ETH_CHAIN_ID).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/robinhood-chain")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn decodes_each_network_s_chain_id() {
        let p = EvmRpcProvider::new(ROBINHOOD_CHAIN_SOURCE_ID).unwrap();
        assert_eq!(
            p.decode_chain_id(&fixture("eth_chainId-mainnet.json"))
                .unwrap(),
            4663
        );
        assert_eq!(
            p.decode_chain_id(&fixture("eth_chainId-testnet.json"))
                .unwrap(),
            46630
        );
    }

    #[test]
    fn rejects_malformed_quantities_and_errors() {
        let p = EvmRpcProvider::new(ROBINHOOD_CHAIN_SOURCE_ID).unwrap();
        for bad in [
            br#"{"jsonrpc":"2.0","id":1,"result":"4663"}"#.as_slice(),
            br#"{"jsonrpc":"2.0","id":1,"result":"0x01237"}"#,
            br#"{"jsonrpc":"2.0","id":1,"result":"0x1G"}"#,
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#,
        ] {
            assert!(p.decode_chain_id(bad).is_err());
        }
    }

    #[test]
    fn decodes_tip20_metadata_with_its_chain() {
        let p = EvmRpcProvider::new(TEMPO_SOURCE_ID).unwrap();
        let read = |n: &str| {
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../tests/fixtures/sources/tempo")
                    .join(n),
            )
            .unwrap()
        };
        assert_eq!(
            p.decode_tip20_metadata(&read("tip20-pathUSD-mainnet.json"))
                .unwrap(),
            Tip20Metadata {
                chain_id: 4217,
                name: "pathUSD".into(),
                symbol: "pathUSD".into(),
                currency: "USD".into(),
                decimals: 6,
            }
        );
        // The testnet twin at the same address is another chain, another name.
        let testnet = p
            .decode_tip20_metadata(&read("tip20-pathUSD-testnet.json"))
            .unwrap();
        assert_eq!(
            (testnet.chain_id, testnet.name.as_str()),
            (42431, "PathUSD")
        );
        assert_eq!(
            p.decode_chain_id(&read("eth_chainId-mainnet.json"))
                .unwrap(),
            4217
        );
        // A reverted call is not metadata.
        let reverted = br#"[{"jsonrpc":"2.0","id":1,"result":"0x1079"},{"jsonrpc":"2.0","id":2,"error":{"code":3,"message":"execution reverted"}}]"#;
        assert!(p.decode_tip20_metadata(reverted).is_err());
        assert!(
            tip20_metadata_body("0x20c0000000000000000000000000000000000000").contains(CURRENCY)
        );
    }

    #[test]
    fn the_tip20_record_key_fits_where_the_body_does_not() {
        let address = "0x20c0000000000000000000000000000000000000";
        let key = tip20_metadata_key(TEMPO_MAINNET_RPC, address);
        assert!(key.chars().count() <= 256, "{key}");
        assert!(tip20_metadata_body(address).len() > 256);
    }
}
