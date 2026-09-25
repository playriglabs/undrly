//! Rust side of the shared-fixture drift guard. TypeScript runs the same
//! `tests/fixtures/shared` files; `undrly-store` checks the database against
//! `vocabulary.json` and `identifiers.json`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use undrly_core::{
    CanonicalId, Category, Cik, CurrencyCode, EntityKind, Figi, InstrumentClass, Isin, Lei, Mic,
    Namespace, ObservationBasis, RelationshipType, SourceId, Timestamp, VenueSymbol, decimal,
};
use uuid::Uuid;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected array, got {value}"))
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

fn set<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    items.into_iter().map(str::to_owned).collect()
}

#[test]
fn canonical_ids_match_fixture() {
    let fixture = fixture("shared/primitives.json");
    for case in fixture["canonicalIds"]["valid"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let category: Category = case["category"].as_str().unwrap().parse().unwrap();
        let uuid = Uuid::parse_str(case["uuid"].as_str().unwrap()).unwrap();
        let from_parts = CanonicalId::from_parts(category, uuid).unwrap();
        assert_eq!(from_parts.to_string(), text, "encode {uuid}");
        assert_eq!(
            CanonicalId::parse(text).unwrap(),
            from_parts,
            "decode {text}"
        );
    }
    for text in strings(&fixture["canonicalIds"]["invalid"]) {
        assert!(
            CanonicalId::parse(text).is_err(),
            "expected invalid {text:?}"
        );
    }
}

#[test]
fn scalar_primitives_match_fixture() {
    let fixture = fixture("shared/primitives.json");
    type Check = fn(&str) -> bool;
    let checks: [(&str, Check); 3] = [
        ("decimals", |s| decimal::parse_canonical(s).is_ok()),
        ("timestamps", |s| Timestamp::parse(s).is_ok()),
        ("sourceIds", |s| SourceId::parse(s).is_ok()),
    ];
    for (group, accepts) in checks {
        for case in strings(&fixture[group]["valid"]) {
            assert!(accepts(case), "{group}: expected valid {case:?}");
        }
        for case in strings(&fixture[group]["invalid"]) {
            assert!(!accepts(case), "{group}: expected invalid {case:?}");
        }
    }
}

#[test]
fn identifiers_match_fixture() {
    let fixture = fixture("identifiers.json");
    type Check = fn(&str) -> bool;
    let checks: [(&str, Check); 7] = [
        ("isin", |s| Isin::parse(s).is_ok()),
        ("figi", |s| Figi::parse(s).is_ok()),
        ("lei", |s| Lei::parse(s).is_ok()),
        ("mic", |s| Mic::parse(s).is_ok()),
        ("iso4217", |s| CurrencyCode::parse(s).is_ok()),
        ("cik", |s| Cik::parse(s).is_ok()),
        ("venueSymbols", |s| VenueSymbol::new(s).is_ok()),
    ];
    for (group, accepts) in checks {
        for case in strings(&fixture[group]["valid"]) {
            assert!(accepts(case), "{group}: expected valid {case:?}");
        }
        for case in strings(&fixture[group]["invalid"]) {
            assert!(!accepts(case), "{group}: expected invalid {case:?}");
        }
    }
}

#[test]
fn vocabulary_matches_fixture() {
    let fixture = fixture("shared/vocabulary.json");

    let categories: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(categories, strings(&fixture["categories"]));

    let kinds: Vec<&str> = EntityKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(kinds, strings(&fixture["entityKinds"]));

    let classes: Vec<&str> = InstrumentClass::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(classes, strings(&fixture["instrumentClasses"]));

    let types: Vec<&str> = RelationshipType::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(types, strings(&fixture["relationshipTypes"]));

    let rules: BTreeSet<Vec<String>> = RelationshipType::ALL
        .iter()
        .flat_map(|t| {
            t.allowed_endpoints()
                .iter()
                .map(|(s, o)| vec![t.as_str().to_owned(), s.to_string(), o.to_string()])
        })
        .collect();
    let fixture_rules: BTreeSet<Vec<String>> =
        serde_json::from_value(fixture["relationshipRules"].clone()).unwrap();
    assert_eq!(rules, fixture_rules);

    assert_eq!(
        set(ObservationBasis::NAMES),
        set(strings(&fixture["observationBases"]))
    );

    let namespaces = fixture["identifierNamespaces"].as_object().unwrap();
    assert_eq!(namespaces.len(), Namespace::ALL.len());
    for ns in Namespace::ALL {
        let categories: Vec<&str> = ns.categories().iter().map(|c| c.as_str()).collect();
        assert_eq!(categories, strings(&namespaces[ns.as_str()]), "{ns}");
    }
}
