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
use undrly_provider::rhj::Assets;

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

/// The CAIP-2 id of the EVM chain whose `eth_chainId` is `chain_id`.
pub fn evm_caip2(chain_id: u64) -> Result<Caip2, NormalizeError> {
    Caip2::new(ChainNamespace::Eip155, &chain_id.to_string()).map_err(|e| invalid("chainId", e))
}

/// The contract of the issuer-registry asset whose Underlying ISIN is
/// `underlying_isin`, on `chain` (an EIP-155 chain): exactly one active
/// asset, with exactly one deployment on that chain, its address valid
/// (EIP-55 checked). Nothing else (symbol, name) selects the asset.
pub fn registry_deployment(
    assets: &Assets,
    underlying_isin: &str,
    chain: &Caip2,
) -> Result<ChainAsset, NormalizeError> {
    if chain.namespace() != ChainNamespace::Eip155 {
        return Err(invalid("chain", format!("{chain} is not an EIP-155 chain")));
    }
    let chain_id: u64 = chain
        .reference()
        .parse()
        .map_err(|_| invalid("chain", format!("{chain}: chain id out of range")))?;
    let mut matching = assets
        .assets
        .iter()
        .filter(|a| a.isin.as_deref() == Some(underlying_isin));
    let asset = matching.next().ok_or_else(|| {
        invalid(
            "isin",
            format!("no registry asset with Underlying {underlying_isin}"),
        )
    })?;
    if matching.next().is_some() {
        return Err(invalid(
            "isin",
            format!("several registry assets with Underlying {underlying_isin}"),
        ));
    }
    if asset.status != "ASSET_STATUS_ACTIVE" {
        return Err(invalid("status", format!("{}: {}", asset.id, asset.status)));
    }
    let mut on_chain = asset.deployments.iter().filter(|d| d.chain_id == chain_id);
    let deployment = on_chain
        .next()
        .ok_or_else(|| invalid("deployments", format!("{}: none on {chain}", asset.id)))?;
    if on_chain.next().is_some() {
        return Err(invalid(
            "deployments",
            format!("{}: several on {chain}", asset.id),
        ));
    }
    ChainAsset::new(
        ChainNamespace::Eip155,
        AssetNamespace::Erc20,
        &deployment.contract_address,
    )
    .map_err(|e| invalid("contractAddress", e))
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

    #[test]
    fn an_evm_chain_is_its_chain_id() {
        assert_eq!(evm_caip2(4663).unwrap().to_string(), "eip155:4663");
        assert_eq!(evm_caip2(46630).unwrap().to_string(), "eip155:46630");
        assert!(evm_caip2(0).is_err());
    }

    #[test]
    fn the_registry_asset_is_selected_by_its_underlying_isin_on_its_chain() {
        use undrly_provider::rhj::RhjProvider;
        let payload = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/rhj/assets.json"),
        )
        .unwrap();
        let assets = RhjProvider::new().decode_assets(&payload).unwrap();
        let chain = evm_caip2(4663).unwrap();
        let asset = registry_deployment(&assets, "US67066G1040", &chain).unwrap();
        // Stored lowercase; the published EIP-55 checksum was verified.
        assert_eq!(
            asset.caip19(&chain).unwrap(),
            "eip155:4663/erc20:0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec"
        );
        // Not on another chain; not by an unknown ISIN.
        assert!(registry_deployment(&assets, "US67066G1040", &evm_caip2(1).unwrap()).is_err());
        assert!(registry_deployment(&assets, "US0000000000", &chain).is_err());
        // A tampered checksum is rejected, not normalized.
        let mut bad = assets.clone();
        for a in &mut bad.assets {
            if a.isin.as_deref() == Some("US67066G1040") {
                a.deployments[0].contract_address =
                    "0xD0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC".into();
            }
        }
        assert!(registry_deployment(&bad, "US67066G1040", &chain).is_err());
    }
}
