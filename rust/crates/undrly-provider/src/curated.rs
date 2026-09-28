//! Undrly's own curated reference data (`data/demo/universe.json`).
//!
//! Curated data covers what no in-scope source is authoritative for: crypto
//! assets, the gold commodity, the perpetual, crypto venues, search aliases,
//! and which source symbol prices what (quote feeds). It is a source like any
//! other (`undrly-curated`): the file is stored raw and every fact derived from
//! it traces back to it.
//!
//! Objects carry pinned canonical ids, generated once when the dataset was
//! written (never derived from an external identifier), plus a dataset-local
//! `key` used only for references inside the file.

use serde::{Deserialize, Serialize};
use undrly_core::SourceId;

use crate::{DecodeError, Provider, ReferenceDataProvider};

/// Source id of curated reference data.
pub const SOURCE_ID: &str = "undrly-curated";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Universe {
    pub dataset: String,
    pub version: u32,
    pub description: String,
    pub currencies: Vec<CurrencyRecord>,
    pub entities: Vec<EntityRecord>,
    pub venues: Vec<VenueRecord>,
    pub instruments: Vec<InstrumentRecord>,
    pub listings: Vec<ListingRecord>,
    /// `[subject key, RELATIONSHIP_TYPE, object key]`.
    pub relationships: Vec<(String, String, String)>,
    pub aliases: Vec<AliasRecord>,
    pub quote_feeds: Vec<QuoteFeedRecord>,
    /// Pairs aggregated with a method other than the default.
    #[serde(default)]
    pub quote_aggregations: Vec<QuoteAggregationRecord>,
    /// Universe memberships (generated snapshots only).
    #[serde(default)]
    pub universes: Vec<UniverseRecord>,
}

/// A universe snapshot: its members, and the upstream raw file that asserted
/// them (stored separately as a source record of `source`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UniverseRecord {
    pub key: String,
    pub source: String,
    pub record_key: String,
    pub sha256: String,
    pub as_of: String,
    pub members: Vec<UniverseMemberRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UniverseMemberRecord {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteAggregationRecord {
    pub subject: String,
    pub unit: String,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrencyRecord {
    pub key: String,
    pub id: String,
    pub name: String,
    pub iso4217: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityRecord {
    pub key: String,
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lei: Option<String>,
    /// SEC Central Index Key (issuer identity only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cik: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VenueRecord {
    pub key: String,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentRecord {
    pub key: String,
    pub id: String,
    pub class: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figi: Option<String>,
    /// Decimal text, e.g. `"1000"` for a 1,000-unit contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_multiplier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_of_measure: Option<String>,
    /// FX instruments only: the base currency (a key or canonical id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// FX instruments only: the quote currency (a key or canonical id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListingRecord {
    pub key: String,
    pub id: String,
    pub instrument: String,
    pub venue: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figi: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AliasRecord {
    pub node: String,
    pub alias: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteFeedRecord {
    pub source: String,
    pub symbol: String,
    pub subject: String,
    pub unit: String,
    pub basis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    pub price_type: String,
    /// Feed cadence; defaults to the V1 window (300 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_seconds: Option<u32>,
    /// What `staleAfterSeconds` counts: `continuous` (default) or `weekdays`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_clock: Option<String>,
    /// The source publishes the inverse pair; observations are inverted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inverted: bool,
}

pub struct CuratedProvider {
    source_id: SourceId,
}

impl CuratedProvider {
    pub fn new() -> Self {
        Self {
            source_id: SourceId::parse(SOURCE_ID).expect("valid source id"),
        }
    }

    /// The same format under another source (e.g. `undrly-universe` for
    /// generated universe snapshots).
    pub fn with_source(source_id: SourceId) -> Self {
        Self { source_id }
    }
}

impl Default for CuratedProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CuratedProvider {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }
}

impl ReferenceDataProvider for CuratedProvider {
    type Record = Universe;

    fn decode_reference(&self, payload: &[u8]) -> Result<Universe, DecodeError> {
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

    #[test]
    fn decodes_the_demo_universe() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/demo/universe.json");
        let u = CuratedProvider::new()
            .decode_reference(&std::fs::read(path).unwrap())
            .unwrap();
        assert_eq!(u.dataset, "undrly-demo-universe");
        assert_eq!(u.instruments.len(), 7, "incl. Tether (V1.4.1)");
        assert_eq!(u.quote_feeds.len(), 7);
        assert_eq!(u.quote_aggregations.len(), 1);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = CuratedProvider::new()
            .decode_reference(br#"{"dataset": "x", "extra": 1}"#)
            .unwrap_err();
        assert_eq!(err.source_id.as_str(), SOURCE_ID);
    }
}
