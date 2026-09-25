//! Deterministic reference-data provider over captured fixture records.
//!
//! The payload format is Undrly's own fixture format (JSON), not any real
//! provider's API. Values are kept exactly as the record states them;
//! nothing here validates identifiers or maps anything to canonical types.

use serde::Deserialize;
use undrly_core::SourceId;

use crate::{DecodeError, Provider, ReferenceDataProvider};

/// A reference record describing one listed security, its issuer, and its
/// listing, as the fixture source states them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceRecord {
    pub record_id: String,
    pub issuer: Issuer,
    pub security: Security,
    pub listing: ListingRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Issuer {
    pub legal_name: String,
    pub entity_type: String,
    pub lei: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Security {
    pub name: String,
    pub security_type: String,
    pub isin: String,
    pub share_class_figi: Option<String>,
    pub denomination: CurrencyRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrencyRecord {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListingRecord {
    pub venue_name: String,
    pub mic: String,
    pub symbol: String,
    pub exchange_figi: Option<String>,
}

/// Decodes fixture reference records for one configured source.
pub struct FixtureProvider {
    source_id: SourceId,
}

impl FixtureProvider {
    pub fn new(source_id: SourceId) -> Self {
        Self { source_id }
    }
}

impl Provider for FixtureProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl ReferenceDataProvider for FixtureProvider {
    type Record = ReferenceRecord;

    fn decode_reference(&self, payload: &[u8]) -> Result<ReferenceRecord, DecodeError> {
        serde_json::from_slice(payload).map_err(|e| DecodeError {
            source_id: self.source_id.clone(),
            reason: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> FixtureProvider {
        FixtureProvider::new(SourceId::parse("reference-fixture").unwrap())
    }

    #[test]
    fn decodes_values_verbatim() {
        let payload = br#"{
            "recordId": "r1",
            "issuer": {"legalName": " Example ", "entityType": "company", "lei": "not-validated"},
            "security": {"name": "S", "securityType": "common-stock", "isin": "x",
                         "shareClassFigi": null, "denomination": {"code": "usd", "name": "D"}},
            "listing": {"venueName": "V", "mic": "m", "symbol": "s", "exchangeFigi": null}
        }"#;
        let record = provider().decode_reference(payload).unwrap();
        assert_eq!(record.issuer.legal_name, " Example ");
        assert_eq!(record.security.denomination.code, "usd");
    }

    #[test]
    fn rejects_malformed_payloads() {
        for payload in [
            &b"not json"[..],
            br#"{"recordId": "r1"}"#,
            br#"{"recordId": "r1", "extra": 1}"#,
        ] {
            let err = provider().decode_reference(payload).unwrap_err();
            assert_eq!(err.source_id.as_str(), "reference-fixture");
        }
    }
}
