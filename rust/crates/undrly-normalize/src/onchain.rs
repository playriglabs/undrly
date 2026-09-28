//! Chain and deployment identity from authoritative sources (V1.5).
//!
//! - A Solana cluster's CAIP-2 id from its own genesis hash
//!   ([`solana_caip2`]).
//! - An issuer's deployment of an asset on a chain from the issuer's own
//!   address list ([`issuer_deployment`]): the row is selected by the label
//!   an explicit binding names, and the address is validated for the chain's
//!   namespace. Symbols and token metadata play no part.

use undrly_core::onchain::solana_address;
use undrly_core::{AssetNamespace, Caip2, ChainAsset, ChainNamespace};
use undrly_provider::circle::UsdcAddress;

use crate::{NormalizeError, invalid};

/// The CAIP-2 chain id of the Solana cluster whose genesis hash is
/// `genesis`: `solana:` and the hash's first 32 characters (CAIP-2 Solana
/// profile). The full hash must be a valid base58 32-byte hash.
pub fn solana_caip2(genesis: &str) -> Result<Caip2, NormalizeError> {
    solana_address(genesis).map_err(|e| invalid("genesis hash", e))?;
    let reference = genesis
        .get(..32)
        .ok_or_else(|| invalid("genesis hash", "shorter than 32 characters"))?;
    Caip2::new(ChainNamespace::Solana, reference).map_err(|e| invalid("genesis hash", e))
}

/// The address the issuer lists for `blockchain` (its own label, matched
/// exactly), as an asset of `namespace` on a chain of `chain`'s namespace.
pub fn issuer_deployment(
    rows: &[UsdcAddress],
    blockchain: &str,
    chain: &Caip2,
    namespace: AssetNamespace,
) -> Result<ChainAsset, NormalizeError> {
    let mut matching = rows.iter().filter(|r| r.blockchain == blockchain);
    let row = matching
        .next()
        .ok_or_else(|| invalid("blockchain", format!("no mainnet row `{blockchain}`")))?;
    if matching.next().is_some() {
        return Err(invalid(
            "blockchain",
            format!("`{blockchain}` listed twice"),
        ));
    }
    ChainAsset::new(chain.namespace(), namespace, &row.address).map_err(|e| invalid("address", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
    const DEVNET_GENESIS: &str = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";

    #[test]
    fn a_cluster_is_its_truncated_genesis_hash() {
        assert_eq!(
            solana_caip2(MAINNET_GENESIS).unwrap().to_string(),
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
        );
        // Devnet is another chain, never mainnet.
        assert_eq!(
            solana_caip2(DEVNET_GENESIS).unwrap().to_string(),
            "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"
        );
        assert!(solana_caip2("not-a-hash").is_err());
        assert!(solana_caip2("5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").is_err());
    }

    #[test]
    fn selects_the_bound_row_and_validates_it_for_the_chain() {
        let rows = vec![
            UsdcAddress {
                blockchain: "Solana".into(),
                address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            },
            UsdcAddress {
                blockchain: "Ethereum".into(),
                address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
            },
        ];
        let mainnet = solana_caip2(MAINNET_GENESIS).unwrap();
        let asset = issuer_deployment(&rows, "Solana", &mainnet, AssetNamespace::Token).unwrap();
        assert_eq!(
            asset.caip19(&mainnet).unwrap(),
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        );
        // Labels match exactly; an EVM address is not a Solana mint.
        assert!(issuer_deployment(&rows, "solana", &mainnet, AssetNamespace::Token).is_err());
        assert!(issuer_deployment(&rows, "Ethereum", &mainnet, AssetNamespace::Token).is_err());
    }
}
