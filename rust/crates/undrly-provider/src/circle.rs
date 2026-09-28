//! Circle's official USDC contract address list (V1.5).
//!
//! `GET https://developers.circle.com/stablecoins/usdc-contract-addresses.md`:
//! Circle's developer documentation, as Markdown. It has a `## Mainnet`
//! section with a table of `| Blockchain | USDC Mainnet Address |` rows, the
//! address in backticks, and a separate testnet section. Circle issues USDC,
//! so this page is the authority for which contract or mint *is* USDC on a
//! chain.
//!
//! Decoding reads the mainnet table only, verbatim: a blockchain label as
//! Circle writes it and the address text. It never interprets labels as
//! chains or addresses as identity; normalization does, against an explicit
//! binding. A page whose layout changed fails to decode rather than yielding
//! a guess.

use undrly_core::SourceId;

use crate::{DecodeError, Provider};

pub const SOURCE_ID: &str = "circle";
pub const USDC_ADDRESSES_URL: &str =
    "https://developers.circle.com/stablecoins/usdc-contract-addresses.md";

/// One mainnet row: the blockchain label and address exactly as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsdcAddress {
    pub blockchain: String,
    pub address: String,
}

pub struct CircleProvider {
    source_id: SourceId,
}

impl CircleProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    /// The `## Mainnet` table of the USDC contract address page. Labels are
    /// unique; testnet rows are never returned.
    pub fn decode_usdc_mainnet(&self, payload: &[u8]) -> Result<Vec<UsdcAddress>, DecodeError> {
        let reject = |reason: String| DecodeError {
            source_id: self.source_id.clone(),
            reason,
        };
        let text = std::str::from_utf8(payload).map_err(|e| reject(e.to_string()))?;
        let mut lines = text.lines().skip_while(|l| l.trim() != "## Mainnet");
        if lines.next().is_none() {
            return Err(reject("no `## Mainnet` section".into()));
        }
        let mut rows: Vec<UsdcAddress> = Vec::new();
        let mut header_seen = false;
        for line in lines.take_while(|l| !l.starts_with("## ")) {
            let line = line.trim();
            if !line.starts_with('|') {
                continue;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if !header_seen {
                if cells.first() != Some(&"Blockchain") {
                    return Err(reject(format!("unexpected table header `{line}`")));
                }
                header_seen = true;
                continue;
            }
            if cells
                .iter()
                .all(|c| c.chars().all(|ch| matches!(ch, ':' | '-')))
            {
                continue; // the separator row
            }
            let [blockchain, address] = cells.as_slice() else {
                return Err(reject(format!("expected two cells in `{line}`")));
            };
            let address = address
                .split('`')
                .nth(1)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| reject(format!("no backticked address in `{line}`")))?;
            if rows.iter().any(|r| r.blockchain == *blockchain) {
                return Err(reject(format!("blockchain `{blockchain}` listed twice")));
            }
            rows.push(UsdcAddress {
                blockchain: (*blockchain).to_owned(),
                address: address.to_owned(),
            });
        }
        if rows.is_empty() {
            return Err(reject("the mainnet table has no rows".into()));
        }
        Ok(rows)
    }
}

impl Default for CircleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CircleProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

/// Fetches the USDC contract address page (feature `http`).
#[cfg(feature = "http")]
pub async fn fetch_usdc_addresses(
    client: &crate::http::HttpClient,
) -> Result<crate::http::FetchedRecord, crate::http::FetchError> {
    client
        .get_accepting(USDC_ADDRESSES_URL, "text/markdown", &[])
        .await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn page() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/circle/usdc-contract-addresses.md"),
        )
        .unwrap()
    }

    #[test]
    fn reads_the_mainnet_table_only() {
        let rows = CircleProvider::new().decode_usdc_mainnet(&page()).unwrap();
        let solana: Vec<&UsdcAddress> = rows.iter().filter(|r| r.blockchain == "Solana").collect();
        assert_eq!(
            solana,
            vec![&UsdcAddress {
                blockchain: "Solana".into(),
                address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            }]
        );
        // Testnet rows (e.g. `Solana Devnet`) are in another section.
        assert!(rows.iter().all(|r| !r.blockchain.contains("Devnet")));
        assert!(rows.iter().any(|r| r.blockchain == "Ethereum"));
    }

    #[test]
    fn a_changed_layout_is_rejected() {
        let p = CircleProvider::new();
        assert!(p.decode_usdc_mainnet(b"# USDC\n\nno table").is_err());
        assert!(
            p.decode_usdc_mainnet(
                b"## Mainnet\n| Chain | Address |\n| - | - |\n| Solana | `x` |\n"
            )
            .is_err()
        );
        assert!(
            p.decode_usdc_mainnet(
                b"## Mainnet\n| Blockchain | A |\n| - | - |\n| Solana | `a` |\n| Solana | `b` |\n"
            )
            .is_err()
        );
        assert!(
            p.decode_usdc_mainnet(b"## Mainnet\n| Blockchain | A |\n| - | - |\n| Solana | a |\n")
                .is_err()
        );
    }
}
