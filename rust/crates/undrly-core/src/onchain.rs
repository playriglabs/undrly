//! Blockchain networks and the deployments of assets on them (V1.4).
//!
//! Identity follows the Chain Agnostic standards, which are chain-neutral:
//!
//! | Object | Standard | Example |
//! | --- | --- | --- |
//! | chain ([`Chain`]) | CAIP-2 `<namespace>:<reference>` | `eip155:1`, `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp` |
//! | deployment ([`Deployment`]) | CAIP-19 `<chain>/<asset namespace>:<asset reference>` | `eip155:1/erc20:0x6b17…`, `solana:5eyk…/token:EPjF…`, `eip155:1/slip44:60` |
//!
//! A deployment is identified by its chain **and** its asset reference: an
//! address means nothing without its chain, so the same EVM address on two
//! chains is two deployments, and a Solana mint can never equal an EVM
//! address (different namespaces, different formats). A deployment is not
//! an economic asset: what it is a form of is a `REPRESENTS` relationship
//! with provenance, so a deployment can exist while that is still unknown.
//!
//! Canonical ids stay generated; CAIP values are external identifiers and
//! never derive identity. Only namespaces whose address rules are implemented
//! here are accepted.

use std::fmt;

use sha3::{Digest, Keccak256};

use crate::id::{ChainId, DeploymentId};
use crate::reference::DisplayName;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {what} `{value}`: {reason}")]
pub struct OnchainError {
    pub what: &'static str,
    pub value: String,
    pub reason: &'static str,
}

fn invalid(what: &'static str, value: &str, reason: &'static str) -> OnchainError {
    OnchainError {
        what,
        value: value.to_owned(),
        reason,
    }
}

/// CAIP-2 namespaces Undrly models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChainNamespace {
    /// EVM chains identified by their EIP-155 chain id.
    Eip155,
    /// Solana clusters identified by their truncated genesis hash.
    Solana,
}

impl ChainNamespace {
    pub const ALL: [ChainNamespace; 2] = [ChainNamespace::Eip155, ChainNamespace::Solana];

    pub const fn as_str(self) -> &'static str {
        match self {
            ChainNamespace::Eip155 => "eip155",
            ChainNamespace::Solana => "solana",
        }
    }

    pub fn parse(s: &str) -> Result<Self, OnchainError> {
        Self::ALL
            .into_iter()
            .find(|n| n.as_str() == s)
            .ok_or_else(|| invalid("chain namespace", s, "not a supported CAIP-2 namespace"))
    }
}

impl fmt::Display for ChainNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn is_base58(s: &str) -> bool {
    s.bytes().all(|b| BASE58.contains(&b))
}

/// Decodes base58 (Bitcoin alphabet), or `None` for a character outside it.
fn base58_decode(s: &str) -> Option<Vec<u8>> {
    // Little-endian base-256 digits of the value.
    let mut bytes: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let mut carry = BASE58.iter().position(|&a| a == c)? as u32;
        for b in &mut bytes {
            carry += u32::from(*b) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    // Each leading '1' is a leading zero byte.
    let zeros = s.bytes().take_while(|&b| b == b'1').count();
    bytes.extend(std::iter::repeat_n(0, zeros));
    bytes.reverse();
    Some(bytes)
}

/// A CAIP-2 chain id, e.g. `eip155:1`. Case-sensitive; stored exactly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Caip2 {
    namespace: ChainNamespace,
    reference: Box<str>,
}

impl Caip2 {
    pub fn new(namespace: ChainNamespace, reference: &str) -> Result<Self, OnchainError> {
        let ok = match namespace {
            // EIP-155 chain ids are unsigned integers; CAIP-2 caps the
            // reference at 32 characters. No leading zeros: one spelling.
            ChainNamespace::Eip155 => {
                (1..=32).contains(&reference.len())
                    && reference.bytes().all(|b| b.is_ascii_digit())
                    && !reference.starts_with('0')
            }
            // The genesis hash (base58) truncated to its first 32 characters.
            ChainNamespace::Solana => reference.len() == 32 && is_base58(reference),
        };
        if !ok {
            return Err(invalid(
                "CAIP-2 reference",
                reference,
                match namespace {
                    ChainNamespace::Eip155 => "expected a decimal chain id without leading zeros",
                    ChainNamespace::Solana => "expected 32 base58 characters of the genesis hash",
                },
            ));
        }
        Ok(Self {
            namespace,
            reference: reference.into(),
        })
    }

    /// Parses `<namespace>:<reference>`.
    pub fn parse(s: &str) -> Result<Self, OnchainError> {
        let (namespace, reference) = s
            .split_once(':')
            .ok_or_else(|| invalid("CAIP-2 chain id", s, "expected <namespace>:<reference>"))?;
        Self::new(ChainNamespace::parse(namespace)?, reference)
    }

    pub fn namespace(&self) -> ChainNamespace {
        self.namespace
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }
}

impl fmt::Display for Caip2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.reference)
    }
}

/// CAIP-19 asset namespaces Undrly models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssetNamespace {
    /// An ERC-20 token contract on an EIP-155 chain; the reference is its address.
    Erc20,
    /// An SPL token on Solana; the reference is its mint address.
    Token,
    /// A chain's native asset by SLIP-44 coin type (`eip155:1/slip44:60` is ether).
    Slip44,
}

impl AssetNamespace {
    pub const ALL: [AssetNamespace; 3] = [
        AssetNamespace::Erc20,
        AssetNamespace::Token,
        AssetNamespace::Slip44,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            AssetNamespace::Erc20 => "erc20",
            AssetNamespace::Token => "token",
            AssetNamespace::Slip44 => "slip44",
        }
    }

    pub fn parse(s: &str) -> Result<Self, OnchainError> {
        Self::ALL
            .into_iter()
            .find(|n| n.as_str() == s)
            .ok_or_else(|| invalid("asset namespace", s, "not a supported CAIP-19 namespace"))
    }

    /// Whether assets of this namespace exist on chains of `chain`.
    pub const fn exists_on(self, chain: ChainNamespace) -> bool {
        matches!(
            (self, chain),
            (AssetNamespace::Erc20, ChainNamespace::Eip155)
                | (AssetNamespace::Token, ChainNamespace::Solana)
                | (AssetNamespace::Slip44, _)
        )
    }
}

impl fmt::Display for AssetNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical (lowercase) EVM address text. Accepts all-lowercase or
/// all-uppercase hex, or mixed case only when it is a valid EIP-55 checksum,
/// so a mistyped checksummed address is rejected rather than normalized.
pub fn evm_address(s: &str) -> Result<String, OnchainError> {
    let hex = s
        .strip_prefix("0x")
        .filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| invalid("EVM address", s, "expected 0x and 40 hexadecimal digits"))?;
    let lower = hex.to_ascii_lowercase();
    let has_lower = hex.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper = hex.bytes().any(|b| b.is_ascii_uppercase());
    if has_lower && has_upper {
        let hash = Keccak256::digest(lower.as_bytes());
        for (i, c) in hex.bytes().enumerate() {
            let nibble = (hash[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 0x0f;
            let upper = c.is_ascii_uppercase();
            if c.is_ascii_alphabetic() && upper != (nibble >= 8) {
                return Err(invalid(
                    "EVM address",
                    s,
                    "mixed case is not a valid EIP-55 checksum",
                ));
            }
        }
    }
    Ok(format!("0x{lower}"))
}

/// Validates a Solana address (a base58 32-byte public key) and returns it
/// unchanged: base58 is case-sensitive and has exactly one spelling per key.
pub fn solana_address(s: &str) -> Result<String, OnchainError> {
    let bytes = (32..=44)
        .contains(&s.len())
        .then(|| base58_decode(s))
        .flatten()
        .ok_or_else(|| invalid("Solana address", s, "expected base58"))?;
    if bytes.len() != 32 {
        return Err(invalid("Solana address", s, "does not decode to 32 bytes"));
    }
    Ok(s.to_owned())
}

/// An asset on a chain, without the chain's reference: the CAIP-19 asset
/// namespace and reference, validated for the chain's namespace. With the
/// chain's [`Caip2`] it forms the full CAIP-19 asset type ([`ChainAsset::caip19`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainAsset {
    chain_namespace: ChainNamespace,
    namespace: AssetNamespace,
    reference: Box<str>,
}

impl ChainAsset {
    /// Validates and canonicalizes `reference` (EVM addresses to lowercase).
    pub fn new(
        chain_namespace: ChainNamespace,
        namespace: AssetNamespace,
        reference: &str,
    ) -> Result<Self, OnchainError> {
        if !namespace.exists_on(chain_namespace) {
            return Err(invalid(
                "asset namespace",
                namespace.as_str(),
                "does not exist on this chain namespace",
            ));
        }
        let reference = match namespace {
            AssetNamespace::Erc20 => evm_address(reference)?,
            AssetNamespace::Token => solana_address(reference)?,
            AssetNamespace::Slip44 => {
                // SLIP-44 coin types are 31-bit, decimal, no leading zeros.
                let ok = !reference.is_empty()
                    && reference.len() <= 10
                    && reference.bytes().all(|b| b.is_ascii_digit())
                    && (reference == "0" || !reference.starts_with('0'))
                    && reference.parse::<u64>().is_ok_and(|n| n < 1 << 31);
                if !ok {
                    return Err(invalid("SLIP-44 coin type", reference, "expected 0..2^31"));
                }
                reference.to_owned()
            }
        };
        Ok(Self {
            chain_namespace,
            namespace,
            reference: reference.into(),
        })
    }

    pub fn chain_namespace(&self) -> ChainNamespace {
        self.chain_namespace
    }

    pub fn namespace(&self) -> AssetNamespace {
        self.namespace
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// The CAIP-19 asset type on `chain`, which must be of this asset's
    /// chain namespace.
    pub fn caip19(&self, chain: &Caip2) -> Result<String, OnchainError> {
        if chain.namespace() != self.chain_namespace {
            return Err(invalid(
                "CAIP-2 chain id",
                &chain.to_string(),
                "is not of this asset's chain namespace",
            ));
        }
        Ok(format!("{chain}/{}:{}", self.namespace, self.reference))
    }

    /// Parses a CAIP-19 asset type `<chain>/<asset namespace>:<reference>`
    /// into the chain id and the asset.
    pub fn parse_caip19(s: &str) -> Result<(Caip2, ChainAsset), OnchainError> {
        let (chain, asset) = s.split_once('/').ok_or_else(|| {
            invalid(
                "CAIP-19 asset type",
                s,
                "expected <chain>/<namespace>:<reference>",
            )
        })?;
        let chain = Caip2::parse(chain)?;
        let (namespace, reference) = asset
            .split_once(':')
            .ok_or_else(|| invalid("CAIP-19 asset type", s, "expected <namespace>:<reference>"))?;
        let asset = ChainAsset::new(
            chain.namespace(),
            AssetNamespace::parse(namespace)?,
            reference,
        )?;
        Ok((chain, asset))
    }
}

/// A blockchain network. Its CAIP-2 chain id is its primary external
/// identifier (one chain node per id); its name is display data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub id: ChainId,
    pub name: DisplayName,
    pub caip2: Caip2,
}

/// An asset's existence on one chain. Identified by (chain, asset); what it
/// represents is a `REPRESENTS` relationship, and its chain is projected as
/// `DEPLOYED_ON`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub id: DeploymentId,
    pub chain_id: ChainId,
    pub asset: ChainAsset,
}

impl Deployment {
    /// A deployment on `chain`; the asset must be of the chain's namespace.
    pub fn on(chain: &Chain, id: DeploymentId, asset: ChainAsset) -> Result<Self, OnchainError> {
        if asset.chain_namespace() != chain.caip2.namespace() {
            return Err(invalid(
                "deployment",
                &chain.caip2.to_string(),
                "asset is not of the chain's namespace",
            ));
        }
        Ok(Self {
            id,
            chain_id: chain.id,
            asset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Examples from the CAIP-2/CAIP-19 specifications and their namespace
    // profiles (ChainAgnostic/CAIPs, ChainAgnostic/namespaces). Fixtures only.
    const DAI: &str = "0x6b175474e89094c44da98b954eedeac495271d0f";
    const SOLANA_MAINNET: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
    const SPL_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    #[test]
    fn caip2_rules_per_namespace() {
        assert_eq!(Caip2::parse("eip155:1").unwrap().to_string(), "eip155:1");
        assert_eq!(
            Caip2::parse(&format!("solana:{SOLANA_MAINNET}"))
                .unwrap()
                .reference(),
            SOLANA_MAINNET
        );
        for bad in [
            "eip155:01",
            "eip155:0",
            "eip155:x1",
            "eip155:",
            "EIP155:1",
            "cosmos:cosmoshub-3",
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvd",
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvd0",
            "1",
        ] {
            assert!(Caip2::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn evm_addresses_are_lowercase_and_checksums_are_verified() {
        assert_eq!(evm_address(DAI).unwrap(), DAI);
        let upper = format!("0x{}", DAI[2..].to_ascii_uppercase());
        assert_eq!(evm_address(&upper).unwrap(), DAI);
        // EIP-55 test vectors.
        for checksummed in [
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ] {
            assert_eq!(
                evm_address(checksummed).unwrap(),
                checksummed.to_ascii_lowercase()
            );
        }
        // One letter's case flipped: not a checksum, rejected.
        assert!(evm_address("0x5aaeb6053F3E94C9b9A09f33669435E7Ef1BeAed").is_err());
        for bad in [
            "6b175474e89094c44da98b954eedeac495271d0f",
            "0x6b17",
            "0X6B175474E89094C44DA98B954EEDEAC495271D0F",
        ] {
            assert!(evm_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn solana_addresses_decode_to_32_bytes() {
        assert_eq!(solana_address(SPL_MINT).unwrap(), SPL_MINT);
        assert_eq!(
            base58_decode("11111111111111111111111111111111").unwrap(),
            vec![0; 32]
        );
        // `0`, `O`, `I`, `l` are not base58; other values do not decode to
        // exactly 32 bytes (31 zero bytes; a value above 2^256).
        let too_large = "z".repeat(44);
        for bad in [
            "0PjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "1111111111111111111111111111111",
            too_large.as_str(),
            DAI,
        ] {
            assert!(solana_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn asset_namespaces_belong_to_their_chains() {
        assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, DAI).is_ok());
        assert!(ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Token, SPL_MINT).is_ok());
        assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Slip44, "60").is_ok());
        assert!(ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Slip44, "501").is_ok());
        // An SPL mint is not an ERC-20 address, and vice versa.
        assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, SPL_MINT).is_err());
        assert!(ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Token, DAI).is_err());
        assert!(ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Erc20, DAI).is_err());
        assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Token, SPL_MINT).is_err());
        for bad in ["060", "-1", "2147483648", ""] {
            assert!(
                ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Slip44, bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn caip19_round_trips() {
        for text in [
            format!("eip155:1/erc20:{DAI}"),
            format!("solana:{SOLANA_MAINNET}/token:{SPL_MINT}"),
            "eip155:1/slip44:60".to_owned(),
        ] {
            let (chain, asset) = ChainAsset::parse_caip19(&text).unwrap();
            assert_eq!(asset.caip19(&chain).unwrap(), text);
        }
        let upper = format!("eip155:1/erc20:0x{}", DAI[2..].to_ascii_uppercase());
        let (chain, asset) = ChainAsset::parse_caip19(&upper).unwrap();
        assert_eq!(
            asset.caip19(&chain).unwrap(),
            format!("eip155:1/erc20:{DAI}")
        );
        for bad in [
            "eip155:1",
            "eip155:1/erc20",
            "eip155:1/nft:1",
            "solana:x/token:y",
        ] {
            assert!(ChainAsset::parse_caip19(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_deployment_is_on_one_chain_of_its_namespace() {
        let ethereum = Chain {
            id: ChainId::generate(),
            name: DisplayName::new("Ethereum").unwrap(),
            caip2: Caip2::parse("eip155:1").unwrap(),
        };
        let base = Chain {
            id: ChainId::generate(),
            name: DisplayName::new("Base").unwrap(),
            caip2: Caip2::parse("eip155:8453").unwrap(),
        };
        let asset = ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, DAI).unwrap();
        let a = Deployment::on(&ethereum, DeploymentId::generate(), asset.clone()).unwrap();
        let b = Deployment::on(&base, DeploymentId::generate(), asset.clone()).unwrap();
        // The same address on two chains: two deployments, different CAIP-19 ids.
        assert_ne!(a.id, b.id);
        assert_ne!(a.chain_id, b.chain_id);
        assert_ne!(
            asset.caip19(&ethereum.caip2).unwrap(),
            asset.caip19(&base.caip2).unwrap()
        );
        let solana = Chain {
            id: ChainId::generate(),
            name: DisplayName::new("Solana").unwrap(),
            caip2: Caip2::parse(&format!("solana:{SOLANA_MAINNET}")).unwrap(),
        };
        assert!(Deployment::on(&solana, DeploymentId::generate(), asset.clone()).is_err());
        assert!(asset.caip19(&solana.caip2).is_err());
    }
}
