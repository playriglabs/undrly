//! SEC EDGAR, the first real source.
//!
//! Scope: one endpoint, EDGAR's company submissions document
//! (`https://data.sec.gov/submissions/CIK##########.json`), decoded only for
//! the filer's identity. Filing history, XBRL financials, and full-text
//! search are out of scope.
//!
//! Decoding ([`SecProvider`]) is pure and works on the exact response bytes.
//! Transport ([`http::SecClient`], feature `http`) is the only code in the
//! workspace that performs network access for this source.
//!
//! The decoded [`CompanySubmissions`] is SEC's own model: SEC field names and
//! unvalidated values. It never leaves the provider/normalization boundary.

#[cfg(feature = "http")]
pub mod http;

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, ReferenceDataProvider};

/// Source id under which SEC EDGAR records are stored.
pub const SOURCE_ID: &str = "sec-edgar";

/// The filer-identity part of an EDGAR submissions document, as SEC states
/// it.
///
/// Fields not listed here (`filings`, addresses, `sic`, `ein`, ...) are
/// ignored by decoding. They remain in the stored raw record. Unknown fields
/// are accepted because SEC adds fields over time; a missing or mistyped
/// required field is a decode error.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanySubmissions {
    /// The filer's CIK as SEC writes it here (10 digits, zero-padded).
    pub cik: String,
    /// EDGAR entity type, e.g. `operating`.
    pub entity_type: String,
    /// EDGAR conformed company name, e.g. `NVIDIA CORP`.
    pub name: String,
    /// Legal Entity Identifier, when SEC has one (often `null`).
    #[serde(default)]
    pub lei: Option<String>,
    /// Trading symbols SEC associates with the filer. Decoded for
    /// inspection only: SEC is not authoritative for listings.
    #[serde(default)]
    pub tickers: Vec<String>,
    /// Exchange names SEC associates with the filer. Decoded for inspection
    /// only, as for `tickers`.
    #[serde(default)]
    pub exchanges: Vec<String>,
}

/// Decodes EDGAR company submissions documents.
pub struct SecProvider {
    source_id: SourceId,
}

impl SecProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }
}

impl Default for SecProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for SecProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl ReferenceDataProvider for SecProvider {
    type Record = CompanySubmissions;

    fn decode_reference(&self, payload: &[u8]) -> Result<CompanySubmissions, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn captured() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sources/sec-edgar/CIK0001045810.json");
        std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
    }

    #[test]
    fn decodes_captured_nvidia_submissions() {
        let record = SecProvider::new().decode_reference(&captured()).unwrap();
        assert_eq!(
            record,
            CompanySubmissions {
                cik: "0001045810".into(),
                entity_type: "operating".into(),
                name: "NVIDIA CORP".into(),
                lei: None,
                tickers: vec!["NVDA".into()],
                exchanges: vec!["Nasdaq".into()],
            }
        );
    }

    #[test]
    fn keeps_values_verbatim_and_ignores_unlisted_fields() {
        let payload = br#"{"cik": "1045810", "entityType": "Operating", "name": " x ",
            "lei": "not-validated", "filings": {"recent": {}}, "newField": 1}"#;
        let record = SecProvider::new().decode_reference(payload).unwrap();
        assert_eq!(record.cik, "1045810");
        assert_eq!(record.entity_type, "Operating");
        assert_eq!(record.name, " x ");
        assert_eq!(record.lei.as_deref(), Some("not-validated"));
        assert!(record.tickers.is_empty());
    }

    #[test]
    fn rejects_malformed_or_unsupported_payloads() {
        let truncated = &captured()[..1000];
        for payload in [
            &b""[..],
            b"<html>Request Rate Threshold Exceeded</html>",
            truncated,
            br#"{"entityType": "operating", "name": "X"}"#,
            br#"{"cik": 1045810, "entityType": "operating", "name": "X"}"#,
            br#"{"cik": "0001045810", "entityType": "operating", "name": null}"#,
            br#"[{"cik": "0001045810"}]"#,
        ] {
            let err = SecProvider::new().decode_reference(payload).unwrap_err();
            assert_eq!(err.source_id.as_str(), SOURCE_ID);
        }
    }
}
