//! A Solana cluster's identity from the cluster itself (V1.5).
//!
//! `POST <cluster RPC> {"jsonrpc":"2.0","id":1,"method":"getGenesisHash"}`
//! returns the cluster's genesis hash (base58). The CAIP-2 chain id of a
//! Solana cluster is that hash truncated to 32 characters. Solana's
//! documentation names `https://api.mainnet.solana.com` as the mainnet
//! endpoint; which cluster a response belongs to is decided by the hash it
//! returns, never by the URL.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider};

pub const SOURCE_ID: &str = "solana-mainnet-rpc";
pub const MAINNET_RPC_URL: &str = "https://api.mainnet.solana.com";
pub const GET_GENESIS_HASH: &str = r#"{"jsonrpc":"2.0","id":1,"method":"getGenesisHash"}"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: String,
}

pub struct SolanaRpcProvider {
    source_id: SourceId,
}

impl SolanaRpcProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    /// The genesis hash, verbatim. An RPC error response is rejected.
    pub fn decode_genesis_hash(&self, payload: &[u8]) -> Result<String, DecodeError> {
        serde_json::from_slice::<Response>(payload)
            .map(|r| r.result)
            .map_err(|e| DecodeError {
                source_id: self.source_id.clone(),
                reason: e.to_string(),
            })
    }
}

impl Default for SolanaRpcProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for SolanaRpcProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

/// Asks the cluster at `rpc_url` for its genesis hash (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_genesis_hash(
    client: &crate::http::HttpClient,
    rpc_url: &str,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client.post_json(rpc_url, GET_GENESIS_HASH).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/solana")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn decodes_the_genesis_hash_of_each_cluster() {
        let p = SolanaRpcProvider::new();
        assert_eq!(
            p.decode_genesis_hash(&fixture("getGenesisHash-mainnet.json"))
                .unwrap(),
            "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
        );
        assert_eq!(
            p.decode_genesis_hash(&fixture("getGenesisHash-devnet.json"))
                .unwrap(),
            "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG"
        );
    }

    #[test]
    fn rejects_rpc_errors() {
        let err =
            br#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;
        assert!(SolanaRpcProvider::new().decode_genesis_hash(err).is_err());
    }
}
