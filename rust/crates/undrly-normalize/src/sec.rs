//! Normalizer for [`undrly_provider::sec`] submissions documents.
//!
//! | SEC field | Canonical claim |
//! | --- | --- |
//! | `cik` | `ExternalIdentifier::Cik` (zero-padded; namespace rules) |
//! | `lei` (when not `null`) | `ExternalIdentifier::Lei` (check digits verified) |
//! | `entityType` = `operating` | `EntityKind::Company` |
//! | `name` | entity display name, verbatim |
//!
//! Everything else is deliberately not normalized. In particular `tickers`
//! and `exchanges` do not become listings, venues, or symbols: SEC is not
//! authoritative for where securities trade. Other `entityType` values
//! (`other`, individuals, ...) are unsupported and rejected.

use undrly_core::{Cik, DisplayName, EntityKind, ExternalIdentifier, Lei};
use undrly_provider::sec::CompanySubmissions;

use crate::{EntityNormalizer, NormalizeError, NormalizedEntityRecord, invalid};

pub struct SecNormalizer;

impl EntityNormalizer for SecNormalizer {
    type Record = CompanySubmissions;

    fn normalize_entity(
        &self,
        record: &CompanySubmissions,
    ) -> Result<NormalizedEntityRecord, NormalizeError> {
        let kind = match record.entity_type.as_str() {
            "operating" => EntityKind::Company,
            other => {
                return Err(NormalizeError::Unsupported {
                    field: "entityType",
                    value: other.to_owned(),
                });
            }
        };
        let mut identifiers = vec![ExternalIdentifier::Cik(
            Cik::normalize(&record.cik).map_err(|e| invalid("cik", e))?,
        )];
        if let Some(lei) = &record.lei {
            identifiers.push(ExternalIdentifier::Lei(
                Lei::normalize(lei).map_err(|e| invalid("lei", e))?,
            ));
        }
        Ok(NormalizedEntityRecord {
            kind,
            name: DisplayName::new(&record.name).map_err(|e| invalid("name", e))?,
            identifiers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvidia() -> CompanySubmissions {
        CompanySubmissions {
            cik: "0001045810".into(),
            entity_type: "operating".into(),
            name: "NVIDIA CORP".into(),
            lei: None,
            tickers: vec!["NVDA".into()],
            exchanges: vec!["Nasdaq".into()],
        }
    }

    #[test]
    fn normalizes_nvidia_to_an_entity_with_its_cik_only() {
        let n = SecNormalizer.normalize_entity(&nvidia()).unwrap();
        assert_eq!(
            n,
            NormalizedEntityRecord {
                kind: EntityKind::Company,
                name: DisplayName::new("NVIDIA CORP").unwrap(),
                identifiers: vec![ExternalIdentifier::Cik(Cik::parse("0001045810").unwrap())],
            }
        );
    }

    #[test]
    fn applies_cik_and_lei_namespace_rules() {
        let mut r = nvidia();
        r.cik = "1045810".into();
        r.lei = Some(" 549300s4klftlo7gsq80 ".into());
        let n = SecNormalizer.normalize_entity(&r).unwrap();
        assert_eq!(
            n.identifiers,
            vec![
                ExternalIdentifier::Cik(Cik::parse("0001045810").unwrap()),
                ExternalIdentifier::Lei(Lei::parse("549300S4KLFTLO7GSQ80").unwrap()),
            ]
        );
    }

    #[test]
    fn rejects_invalid_values_and_unsupported_entity_types() {
        type Mutation = fn(&mut CompanySubmissions);
        let cases: [(Mutation, &str); 5] = [
            (|r| r.cik = "0000000000".into(), "cik"),
            (|r| r.cik = "CIK0001045810".into(), "cik"),
            (|r| r.lei = Some("549300S4KLFTLO7GSQ81".into()), "lei"),
            (|r| r.name = "NVIDIA CORP ".into(), "name"),
            (|r| r.name = String::new(), "name"),
        ];
        for (mutate, field) in cases {
            let mut r = nvidia();
            mutate(&mut r);
            assert!(
                matches!(
                    SecNormalizer.normalize_entity(&r),
                    Err(NormalizeError::Invalid { field: f, .. }) if f == field
                ),
                "{field}"
            );
        }
        for entity_type in ["other", "", "Operating", "investment"] {
            let mut r = nvidia();
            r.entity_type = entity_type.into();
            assert_eq!(
                SecNormalizer.normalize_entity(&r),
                Err(NormalizeError::Unsupported {
                    field: "entityType",
                    value: entity_type.into(),
                })
            );
        }
    }
}
