//! Normalizer for [`undrly_provider::fixture`] reference records.

use undrly_core::{
    CurrencyCode, DisplayName, EntityKind, Figi, InstrumentClass, Isin, Lei, Mic, Validity,
    VenueSymbol,
};
use undrly_provider::fixture::ReferenceRecord;

use crate::{
    NormalizeError, NormalizedCurrency, NormalizedEntity, NormalizedInstrument, NormalizedListing,
    NormalizedReference, ReferenceNormalizer, invalid,
};

pub struct FixtureNormalizer;

fn name(field: &'static str, value: &str) -> Result<DisplayName, NormalizeError> {
    DisplayName::new(value).map_err(|e| invalid(field, e))
}

fn figi(field: &'static str, value: Option<&String>) -> Result<Option<Figi>, NormalizeError> {
    value
        .map(|v| Figi::normalize(v).map_err(|e| invalid(field, e)))
        .transpose()
}

impl ReferenceNormalizer for FixtureNormalizer {
    type Record = ReferenceRecord;

    fn normalize(&self, record: &ReferenceRecord) -> Result<NormalizedReference, NormalizeError> {
        let kind = match record.issuer.entity_type.as_str() {
            "company" => EntityKind::Company,
            other => {
                return Err(NormalizeError::Unsupported {
                    field: "issuer.entityType",
                    value: other.to_owned(),
                });
            }
        };
        let class = match record.security.security_type.as_str() {
            "common-stock" => InstrumentClass::Equity,
            other => {
                return Err(NormalizeError::Unsupported {
                    field: "security.securityType",
                    value: other.to_owned(),
                });
            }
        };
        Ok(NormalizedReference {
            issuer: NormalizedEntity {
                kind,
                name: name("issuer.legalName", &record.issuer.legal_name)?,
                lei: Lei::normalize(&record.issuer.lei).map_err(|e| invalid("issuer.lei", e))?,
            },
            instrument: NormalizedInstrument {
                class,
                name: name("security.name", &record.security.name)?,
                isin: Isin::normalize(&record.security.isin)
                    .map_err(|e| invalid("security.isin", e))?,
                share_class_figi: figi(
                    "security.shareClassFigi",
                    record.security.share_class_figi.as_ref(),
                )?,
            },
            denomination: NormalizedCurrency {
                name: name(
                    "security.denomination.name",
                    &record.security.denomination.name,
                )?,
                code: CurrencyCode::normalize(&record.security.denomination.code)
                    .map_err(|e| invalid("security.denomination.code", e))?,
            },
            listing: NormalizedListing {
                venue_name: name("listing.venueName", &record.listing.venue_name)?,
                mic: Mic::normalize(&record.listing.mic).map_err(|e| invalid("listing.mic", e))?,
                // Venue symbols are not normalized: case and punctuation are
                // significant and kept exactly as the source states them.
                symbol: VenueSymbol::new(&record.listing.symbol)
                    .map_err(|e| invalid("listing.symbol", e))?,
                exchange_figi: figi(
                    "listing.exchangeFigi",
                    record.listing.exchange_figi.as_ref(),
                )?,
                symbol_valid_during: Validity::UNBOUNDED,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use undrly_provider::fixture::{CurrencyRecord, Issuer, ListingRecord, Security};

    fn record() -> ReferenceRecord {
        ReferenceRecord {
            record_id: "r1".into(),
            issuer: Issuer {
                legal_name: "NVIDIA CORPORATION".into(),
                entity_type: "company".into(),
                lei: " 549300s4klftlo7gsq80 ".into(),
            },
            security: Security {
                name: "NVIDIA CORP".into(),
                security_type: "common-stock".into(),
                isin: "us67066g1040".into(),
                share_class_figi: Some("BBG001S5TZJ6".into()),
                denomination: CurrencyRecord {
                    code: "usd".into(),
                    name: "US Dollar".into(),
                },
            },
            listing: ListingRecord {
                venue_name: "Nasdaq".into(),
                mic: "xnas".into(),
                symbol: "NVDA".into(),
                exchange_figi: Some("BBG000BBK0R0".into()),
            },
        }
    }

    #[test]
    fn applies_namespace_specific_normalization() {
        let n = FixtureNormalizer.normalize(&record()).unwrap();
        assert_eq!(n.issuer.lei.as_str(), "549300S4KLFTLO7GSQ80");
        assert_eq!(n.instrument.isin.as_str(), "US67066G1040");
        assert_eq!(n.denomination.code.as_str(), "USD");
        assert_eq!(n.listing.mic.as_str(), "XNAS");
        assert_eq!(n.listing.symbol.as_str(), "NVDA");
    }

    #[test]
    fn venue_symbols_are_not_case_folded() {
        let mut r = record();
        r.listing.symbol = "nvda".into();
        assert_eq!(
            FixtureNormalizer
                .normalize(&r)
                .unwrap()
                .listing
                .symbol
                .as_str(),
            "nvda"
        );
    }

    #[test]
    fn rejects_invalid_identifiers_and_unknown_vocabulary() {
        let mut bad_isin = record();
        bad_isin.security.isin = "US67066G1041".into();
        assert!(matches!(
            FixtureNormalizer.normalize(&bad_isin),
            Err(NormalizeError::Invalid {
                field: "security.isin",
                ..
            })
        ));
        let mut bad_type = record();
        bad_type.security.security_type = "preferred".into();
        assert!(matches!(
            FixtureNormalizer.normalize(&bad_type),
            Err(NormalizeError::Unsupported {
                field: "security.securityType",
                ..
            })
        ));
    }
}
