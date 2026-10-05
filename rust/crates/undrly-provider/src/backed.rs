//! Backed Assets (JE) Limited, issuer of xStocks (docs/v1.10-tokenized-stocks.md
//! §4): `GET https://api.backed.fi/api/v1/token`, the issuer's own product
//! registry.
//!
//! Per product: its name and token symbol (`AAPLx`), the product's own ISIN
//! (`CH…`), the underlying's symbol and ISIN, and the product's deployments
//! (`address`, `network`). Solana addresses are published with an `svm:`
//! prefix; [`solana_mint`] removes it, nothing else is rewritten.
//!
//! Decoding keeps the registry verbatim and ignores fields Undrly does not
//! use (the registry adds fields over time).

use serde::Deserialize;
use undrly_core::SourceId;

use crate::DecodeError;

pub const SOURCE_ID: &str = "backed-api";
pub const TOKENS_URL: &str = "https://api.backed.fi/api/v1/token";
/// The registry's network name for Solana mainnet.
pub const SOLANA_NETWORK: &str = "Solana";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub nodes: Vec<Token>,
    pub page: Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub has_next_page: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub name: String,
    pub symbol: String,
    /// The product's own ISIN.
    pub isin: Option<String>,
    pub underlying_symbol: Option<String>,
    pub underlying_isin: Option<String>,
    pub deployments: Vec<Deployment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Deployment {
    pub address: String,
    pub network: String,
}

/// The mint of a Solana deployment (`svm:<mint>` → `<mint>`); `None` for
/// any other spelling.
pub fn solana_mint(d: &Deployment) -> Option<&str> {
    (d.network == SOLANA_NETWORK)
        .then(|| d.address.strip_prefix("svm:"))
        .flatten()
        .filter(|m| !m.is_empty())
}

pub fn decode_tokens(payload: &[u8]) -> Result<Tokens, DecodeError> {
    serde_json::from_slice(payload).map_err(|e| DecodeError {
        source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_products_and_solana_mints() {
        let t = decode_tokens(
            br#"{"nodes":[{"id":"x","name":"Apple xStock","symbol":"AAPLx","isin":"CH1234567890",
                 "underlyingSymbol":"AAPL","underlyingIsin":"US0378331005","logo":"l",
                 "deployments":[{"address":"0xabc","network":"Ethereum"},
                                {"address":"svm:XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp","network":"Solana"}]}],
                 "page":{"currentPage":0,"hasNextPage":false}}"#,
        )
        .unwrap();
        let a = &t.nodes[0];
        assert_eq!(a.underlying_isin.as_deref(), Some("US0378331005"));
        let mints: Vec<&str> = a.deployments.iter().filter_map(solana_mint).collect();
        assert_eq!(mints, ["XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp"]);
        assert!(!t.page.has_next_page);
        assert!(decode_tokens(br#"{"nodes":[]}"#).is_err());
    }

    #[test]
    fn a_solana_address_without_the_prefix_is_not_read() {
        let d = Deployment {
            address: "XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp".into(),
            network: "Solana".into(),
        };
        assert_eq!(solana_mint(&d), None);
    }
}
