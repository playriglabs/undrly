//! V1.2 FX universe builder (docs/v1.2-fx.md).
//!
//! ```text
//! data/reference/fx-spec.json (Undrly-authored) + id map + V1 curated universe
//!   → build (pure) → data/reference/fx.json (curated format) + updated id map + report
//! ```
//!
//! The spec names currencies by ISO 4217 code, pairs as `BASE/QUOTE`, and the
//! approved source feeds that publish each pair. The build:
//!
//! - models every currency as a **currency node** (V1's USD and EUR are
//!   reused) and every pair as one **FX instrument** (class `fx`) with its base
//!   and quote currency; a pair in both universes is one instrument;
//! - declares each feed for the pair, priced in the pair's quote currency,
//!   with the source's price type and freshness policy; a feed that publishes
//!   the inverse pair is declared `inverted` (ingestion inverts it);
//! - never mixes live venue feeds and reference-rate feeds on one pair, so a
//!   reference rate is never aggregated with a market quote;
//! - declares `mean-venue-mid-v1` for pairs with two or more venue feeds;
//! - emits both universes' memberships, asserted by the spec file itself.
//!
//! Same inputs, same bytes: everything follows the spec's order or is sorted,
//! and ids come only from the id map (`iso4217:<CODE>`, `fx:<BASE>/<QUOTE>`,
//! `venue:<key>`), minted once for new keys.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Deserialize;
use undrly_core::{CanonicalId, Category, CurrencyCode, UniverseKey};
use undrly_provider::curated::{
    AliasRecord, CurrencyRecord, InstrumentRecord, QuoteAggregationRecord, QuoteFeedRecord,
    Universe, UniverseMemberRecord, UniverseRecord, VenueRecord,
};

use crate::{BuildError, IdMap, sha256_hex};

/// Where the spec lives; also its record key when seeded.
pub const SPEC_PATH: &str = "data/reference/fx-spec.json";
/// Source of the spec record (Undrly-authored).
pub const SPEC_SOURCE: &str = "undrly-curated";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Spec {
    pub dataset: String,
    pub version: u32,
    /// When the pair list and its sources were researched.
    pub as_of: String,
    pub description: String,
    pub sources: BTreeMap<String, SourceDefaults>,
    pub venues: Vec<SpecVenue>,
    pub currencies: Vec<SpecCurrency>,
    pub pairs: Vec<SpecPair>,
    pub excluded: Vec<SpecExclusion>,
}

/// How a source's feeds are declared.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDefaults {
    /// The venue key for an execution venue's own book; absent for a
    /// reference-rate publisher (basis `aggregated`).
    #[serde(default)]
    pub venue: Option<String>,
    pub price_type: String,
    pub stale_after_seconds: u32,
    #[serde(default)]
    pub freshness_clock: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecVenue {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecCurrency {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecPair {
    pub pair: String,
    pub universes: Vec<String>,
    pub feeds: Vec<SpecFeed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecFeed {
    pub source: String,
    pub symbol: String,
    /// The source publishes the inverse pair.
    #[serde(default)]
    pub inverted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecExclusion {
    pub pair: String,
    pub universe: String,
    pub reason: String,
}

/// V1 objects the FX build reuses, by build key.
const V1_PINS: [(&str, &str); 4] = [
    ("iso4217:USD", "usd"),
    ("iso4217:EUR", "eur"),
    ("venue:kraken", "kraken"),
    ("fx:EUR/USD", "eur-usd"),
];

pub struct FxBuild {
    pub universe: Universe,
    pub ids: IdMap,
    pub report: String,
    pub minted: usize,
}

/// One pair's classification in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Live,
    Reference,
    Unsupported,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Live => "live market quote",
            Kind::Reference => "reference rate",
            Kind::Unsupported => "unsupported (no approved source)",
        }
    }
}

fn invalid(message: impl Into<String>) -> BuildError {
    BuildError::Invalid(format!("fx spec: {}", message.into()))
}

/// `BASE/QUOTE` of two upper-case ISO 4217 codes.
fn split_pair(pair: &str) -> Result<(String, String), BuildError> {
    let (base, quote) = pair
        .split_once('/')
        .ok_or_else(|| invalid(format!("`{pair}` is not BASE/QUOTE")))?;
    for code in [base, quote] {
        let parsed = CurrencyCode::parse(code).map_err(|e| invalid(format!("`{pair}`: {e}")))?;
        if parsed.as_str() != code {
            return Err(invalid(format!("`{pair}`: codes are upper case")));
        }
    }
    if base == quote {
        return Err(invalid(format!("`{pair}`: same currency")));
    }
    Ok((base.to_owned(), quote.to_owned()))
}

/// Builds the FX universes from `spec_bytes` (the file at [`SPEC_PATH`]).
/// `mint` supplies ids for keys the id map does not have.
pub fn build(
    spec_bytes: &[u8],
    v1: &Universe,
    ids: IdMap,
    mint: &mut dyn FnMut(Category) -> CanonicalId,
) -> Result<FxBuild, BuildError> {
    let spec: Spec =
        serde_json::from_slice(spec_bytes).map_err(|e| invalid(format!("decode: {e}")))?;
    if spec.dataset != "undrly-fx-spec" || spec.version != 1 {
        return Err(invalid("unexpected dataset or version"));
    }
    let v1_ids: BTreeMap<&str, &str> = v1
        .currencies
        .iter()
        .map(|x| (x.key.as_str(), x.id.as_str()))
        .chain(v1.venues.iter().map(|x| (x.key.as_str(), x.id.as_str())))
        .chain(
            v1.instruments
                .iter()
                .map(|x| (x.key.as_str(), x.id.as_str())),
        )
        .collect();
    let v1_feeds: BTreeSet<(&str, &str)> = v1
        .quote_feeds
        .iter()
        .map(|f| (f.source.as_str(), f.symbol.as_str()))
        .collect();

    let mut ids = ids;
    ids.version = 1;
    let mut pinned: BTreeSet<String> = BTreeSet::new();
    for (key, v1_key) in V1_PINS {
        let id = *v1_ids
            .get(v1_key)
            .ok_or_else(|| BuildError::V1(format!("no object with key `{v1_key}`")))?;
        match ids.ids.get(key) {
            Some(existing) if existing != id => {
                return Err(BuildError::IdMap(format!(
                    "{key} is pinned to V1 `{v1_key}` ({id}), map has {existing}"
                )));
            }
            _ => {
                ids.ids.insert(key.to_owned(), id.to_owned());
            }
        }
        pinned.insert(id.to_owned());
    }
    let mut minted = 0;
    let mut id_of =
        |ids: &mut IdMap, key: &str, category: Category| -> Result<String, BuildError> {
            if let Some(id) = ids.ids.get(key) {
                let parsed =
                    CanonicalId::parse(id).map_err(|e| BuildError::IdMap(format!("{key}: {e}")))?;
                if parsed.category() != category {
                    return Err(BuildError::IdMap(format!("{key} is not a {category}")));
                }
                return Ok(id.clone());
            }
            let id = mint(category).to_string();
            minted += 1;
            ids.ids.insert(key.to_owned(), id.clone());
            Ok(id)
        };
    // V1 objects are referenced by canonical id and never redeclared; new
    // objects by their build key.
    let node_ref = |key: &str, id: &str| {
        if pinned.contains(id) {
            id.to_owned()
        } else {
            key.to_owned()
        }
    };

    let mut out = Universe {
        dataset: "undrly-fx".into(),
        version: 1,
        description: "Generated by `undrly-collect fx build` from data/reference/fx-spec.json (docs/v1.2-fx.md). Undrly-authored reference data: currencies, FX instruments, feed declarations and universe memberships; no upstream prices.".into(),
        currencies: Vec::new(),
        entities: Vec::new(),
        venues: Vec::new(),
        instruments: Vec::new(),
        listings: Vec::new(),
        relationships: Vec::new(),
        aliases: Vec::new(),
        quote_feeds: Vec::new(),
        quote_aggregations: Vec::new(),
        universes: Vec::new(),
    };
    let mut aliases: BTreeSet<(String, String, String)> = BTreeSet::new();

    // Currencies.
    let mut currency_ref: BTreeMap<String, String> = BTreeMap::new();
    for c in &spec.currencies {
        let key = format!("iso4217:{}", c.code);
        if currency_ref.contains_key(&c.code) {
            return Err(invalid(format!("duplicate currency {}", c.code)));
        }
        CurrencyCode::parse(&c.code).map_err(|e| invalid(format!("{}: {e}", c.code)))?;
        let id = id_of(&mut ids, &key, Category::Currency)?;
        let r = node_ref(&key, &id);
        if r == key {
            out.currencies.push(CurrencyRecord {
                key: key.clone(),
                id,
                name: c.name.clone(),
                iso4217: c.code.clone(),
            });
            aliases.insert((key.clone(), c.name.clone(), "name".into()));
        }
        currency_ref.insert(c.code.clone(), r);
    }

    // Venues.
    let mut venue_ref: BTreeMap<String, String> = BTreeMap::new();
    venue_ref.insert(
        "kraken".into(),
        ids.ids.get("venue:kraken").cloned().expect("pinned"),
    );
    for v in &spec.venues {
        let key = format!("venue:{}", v.key);
        let id = id_of(&mut ids, &key, Category::Venue)?;
        let r = node_ref(&key, &id);
        if r == key {
            out.venues.push(VenueRecord {
                key: key.clone(),
                id,
                name: v.name.clone(),
                mic: None,
            });
            aliases.insert((key.clone(), v.name.clone(), "name".into()));
        }
        venue_ref.insert(v.key.clone(), r);
    }

    // Pairs.
    let universe_keys = [UniverseKey::FxMajor, UniverseKey::FxSoutheastAsia];
    let mut members: BTreeMap<&str, Vec<UniverseMemberRecord>> = BTreeMap::new();
    let mut seen_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut seen_feeds: BTreeSet<(String, String)> = BTreeSet::new();
    let mut rows: Vec<(String, Vec<String>, Kind, String)> = Vec::new();
    for p in &spec.pairs {
        let (base, quote) = split_pair(&p.pair)?;
        if seen_pairs.contains(&(quote.clone(), base.clone())) {
            return Err(invalid(format!(
                "{} is the inverse of a pair already listed; one orientation per market",
                p.pair
            )));
        }
        if !seen_pairs.insert((base.clone(), quote.clone())) {
            return Err(invalid(format!("duplicate pair {}", p.pair)));
        }
        let (Some(base_ref), Some(quote_ref)) = (currency_ref.get(&base), currency_ref.get(&quote))
        else {
            return Err(invalid(format!("{}: undeclared currency", p.pair)));
        };
        let key = format!("fx:{}", p.pair);
        let id = id_of(&mut ids, &key, Category::Instrument)?;
        let pair_ref = node_ref(&key, &id);
        if pair_ref == key {
            out.instruments.push(InstrumentRecord {
                key: key.clone(),
                id,
                class: "fx".into(),
                name: p.pair.clone(),
                isin: None,
                figi: None,
                contract_multiplier: None,
                unit_of_measure: None,
                base: Some(base_ref.clone()),
                quote: Some(quote_ref.clone()),
            });
            // The compact form, from two known ISO codes only.
            aliases.insert((key.clone(), format!("{base}{quote}"), "symbol".into()));
        }
        if p.universes.is_empty() {
            return Err(invalid(format!("{}: in no universe", p.pair)));
        }
        for u in &p.universes {
            let k = universe_keys
                .iter()
                .find(|k| k.as_str() == u)
                .ok_or_else(|| invalid(format!("{}: unknown universe `{u}`", p.pair)))?;
            members
                .entry(k.as_str())
                .or_default()
                .push(UniverseMemberRecord {
                    node: pair_ref.clone(),
                    rank: None,
                    source_symbol: Some(p.pair.clone()),
                });
        }

        let mut venues = 0;
        let mut references = 0;
        let mut described = Vec::new();
        for f in &p.feeds {
            let d = spec
                .sources
                .get(&f.source)
                .ok_or_else(|| invalid(format!("{}: unknown source `{}`", p.pair, f.source)))?;
            if !seen_feeds.insert((f.source.clone(), f.symbol.clone())) {
                return Err(invalid(format!(
                    "feed {}:{} declared twice",
                    f.source, f.symbol
                )));
            }
            let venue = match &d.venue {
                Some(v) => {
                    venues += 1;
                    Some(
                        venue_ref
                            .get(v)
                            .cloned()
                            .ok_or_else(|| invalid(format!("unknown venue `{v}`")))?,
                    )
                }
                None => {
                    references += 1;
                    None
                }
            };
            described.push(format!(
                "{} `{}`{}",
                f.source,
                f.symbol,
                if f.inverted { " (inverted)" } else { "" }
            ));
            // V1 declares Kraken's ZEURZUSD itself.
            if v1_feeds.contains(&(f.source.as_str(), f.symbol.as_str())) {
                continue;
            }
            out.quote_feeds.push(QuoteFeedRecord {
                source: f.source.clone(),
                symbol: f.symbol.clone(),
                subject: pair_ref.clone(),
                unit: quote_ref.clone(),
                basis: if venue.is_some() {
                    "venue"
                } else {
                    "aggregated"
                }
                .into(),
                venue,
                price_type: d.price_type.clone(),
                stale_after_seconds: Some(d.stale_after_seconds),
                freshness_clock: d.freshness_clock.clone(),
                inverted: f.inverted,
            });
        }
        if venues > 0 && references > 0 {
            return Err(invalid(format!(
                "{}: live venue feeds and reference rates are never combined on one pair",
                p.pair
            )));
        }
        let kind = match (venues, references) {
            (0, 0) => Kind::Unsupported,
            (0, _) => Kind::Reference,
            _ => Kind::Live,
        };
        let method = if venues >= 2 {
            out.quote_aggregations.push(QuoteAggregationRecord {
                subject: pair_ref.clone(),
                unit: quote_ref.clone(),
                method: "mean-venue-mid-v1".into(),
            });
            "mean-venue-mid-v1"
        } else if p.feeds.is_empty() {
            "—"
        } else {
            "latest-observation-v1"
        };
        rows.push((p.pair.clone(), p.universes.clone(), kind, {
            let mut s = described.join(", ");
            if s.is_empty() {
                s = "—".into();
            }
            format!("{s} | {method}")
        }));
    }
    for e in &spec.excluded {
        let (base, quote) = split_pair(&e.pair)?;
        if seen_pairs.contains(&(base, quote)) {
            return Err(invalid(format!("{} is both a member and excluded", e.pair)));
        }
        if !universe_keys.iter().any(|k| k.as_str() == e.universe) {
            return Err(invalid(format!(
                "{}: unknown universe `{}`",
                e.pair, e.universe
            )));
        }
    }

    let sha256 = sha256_hex(spec_bytes);
    for k in universe_keys {
        if let Some(m) = members.remove(k.as_str()) {
            out.universes.push(UniverseRecord {
                key: k.as_str().into(),
                source: SPEC_SOURCE.into(),
                record_key: SPEC_PATH.into(),
                sha256: sha256.clone(),
                as_of: spec.as_of.clone(),
                members: m,
            });
        }
    }
    out.aliases = aliases
        .into_iter()
        .map(|(node, alias, kind)| AliasRecord { node, alias, kind })
        .collect();

    let report = report(&spec, &out, &rows, &sha256, minted);
    Ok(FxBuild {
        universe: out,
        ids,
        report,
        minted,
    })
}

fn report(
    spec: &Spec,
    out: &Universe,
    rows: &[(String, Vec<String>, Kind, String)],
    sha256: &str,
    minted: usize,
) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "# FX universe build report\n");
    let _ = writeln!(
        r,
        "Generated by `undrly-collect fx build` from `{SPEC_PATH}` (SHA-256 `{}…`, as of {}). docs/v1.2-fx.md.\n",
        &sha256[..16],
        spec.as_of
    );
    let _ = writeln!(
        r,
        "- currencies: {} ({} new, V1's USD and EUR reused)",
        spec.currencies.len(),
        out.currencies.len()
    );
    let _ = writeln!(
        r,
        "- FX instruments: {} pairs ({} new; V1 EUR/USD reused)",
        rows.len(),
        out.instruments.len()
    );
    let _ = writeln!(
        r,
        "- quote feeds declared: {} (plus V1's Kraken ZEURZUSD); aggregation declarations: {}",
        out.quote_feeds.len(),
        out.quote_aggregations.len()
    );
    let _ = writeln!(r, "- ids minted this build: {minted}");
    for u in &out.universes {
        let _ = writeln!(r, "\n## {} ({} members)\n", u.key, u.members.len());
        let in_u: Vec<_> = rows.iter().filter(|x| x.1.contains(&u.key)).collect();
        for kind in [Kind::Live, Kind::Reference, Kind::Unsupported] {
            let n = in_u.iter().filter(|x| x.2 == kind).count();
            let _ = writeln!(r, "- {}: {n}", kind.as_str());
        }
        let _ = writeln!(r, "\n| Pair | Class | Feeds | Method |");
        let _ = writeln!(r, "| --- | --- | --- | --- |");
        for (pair, _, kind, feeds) in &in_u {
            let _ = writeln!(r, "| {pair} | {} | {feeds} |", kind.as_str());
        }
        let excluded: Vec<_> = spec
            .excluded
            .iter()
            .filter(|e| e.universe == u.key)
            .collect();
        if !excluded.is_empty() {
            let _ = writeln!(r, "\nRequested but not included ({}):\n", excluded.len());
            for e in excluded {
                let _ = writeln!(r, "- {}: {}", e.pair, e.reason);
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use undrly_core::{CurrencyId, InstrumentId, VenueId};

    use super::*;
    use crate::to_json;

    fn read(path: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(path),
        )
        .unwrap()
    }

    fn v1() -> Universe {
        serde_json::from_slice(&read("data/demo/universe.json")).unwrap()
    }

    fn fresh_mint() -> impl FnMut(Category) -> CanonicalId {
        |c| match c {
            Category::Currency => CurrencyId::generate().canonical(),
            Category::Instrument => InstrumentId::generate().canonical(),
            Category::Venue => VenueId::generate().canonical(),
            other => panic!("unexpected {other}"),
        }
    }

    fn never_mint(c: Category) -> CanonicalId {
        panic!("minted a {c} on a rebuild")
    }

    #[test]
    fn rebuild_is_byte_identical_and_mints_nothing() {
        let spec = read(SPEC_PATH);
        let first = build(&spec, &v1(), IdMap::default(), &mut fresh_mint()).unwrap();
        assert!(first.minted > 0);
        let second = build(&spec, &v1(), first.ids.clone(), &mut never_mint).unwrap();
        assert_eq!(second.minted, 0);
        assert_eq!(to_json(&first.universe), to_json(&second.universe));
        assert_eq!(first.ids, second.ids);
        // The report states how many ids a build minted; from the same id map
        // it is identical too.
        let third = build(&spec, &v1(), second.ids.clone(), &mut never_mint).unwrap();
        assert_eq!(second.report, third.report);
        assert_eq!(to_json(&second.universe), to_json(&third.universe));
    }

    #[test]
    fn the_committed_build_is_current() {
        // data/reference/fx.json and fx-ids.json are what the spec builds.
        let ids: IdMap = serde_json::from_slice(&read("data/reference/fx-ids.json")).unwrap();
        let b = build(&read(SPEC_PATH), &v1(), ids.clone(), &mut never_mint).unwrap();
        assert_eq!(to_json(&b.universe), read("data/reference/fx.json"));
        assert_eq!(b.ids, ids);
    }

    #[test]
    fn reuses_v1_and_shares_instruments_across_universes() {
        let b = build(&read(SPEC_PATH), &v1(), IdMap::default(), &mut fresh_mint()).unwrap();
        let v1 = v1();
        let eur_usd = &v1
            .instruments
            .iter()
            .find(|i| i.key == "eur-usd")
            .unwrap()
            .id;
        assert_eq!(b.ids.ids["fx:EUR/USD"], *eur_usd);
        assert!(b.universe.instruments.iter().all(|i| i.name != "EUR/USD"));
        assert!(b.universe.currencies.iter().all(|c| c.iso4217 != "USD"));
        let major = b
            .universe
            .universes
            .iter()
            .find(|u| u.key == "fx-major")
            .unwrap();
        let sea = b
            .universe
            .universes
            .iter()
            .find(|u| u.key == "fx-southeast-asia")
            .unwrap();
        let node = |u: &UniverseRecord, pair: &str| {
            u.members
                .iter()
                .find(|m| m.source_symbol.as_deref() == Some(pair))
                .map(|m| m.node.clone())
        };
        assert_eq!(node(major, "USD/SGD"), node(sea, "USD/SGD"));
        assert_eq!(node(major, "EUR/USD").as_deref(), Some(eur_usd.as_str()));
        assert_eq!(
            b.universe
                .instruments
                .iter()
                .filter(|i| i.name == "USD/SGD")
                .count(),
            1
        );
        // EUR/USD has two venue feeds: a mean of venue mids.
        assert!(
            b.universe
                .quote_aggregations
                .iter()
                .any(|a| a.subject == *eur_usd && a.method == "mean-venue-mid-v1")
        );
        // CAD/JPY is the Bank of Canada's JPY/CAD, inverted, in JPY.
        let cad_jpy = b
            .universe
            .quote_feeds
            .iter()
            .find(|f| f.symbol == "FXJPYCAD")
            .unwrap();
        assert!(cad_jpy.inverted);
        assert_eq!(cad_jpy.subject, "fx:CAD/JPY");
        assert_eq!(cad_jpy.unit, "iso4217:JPY");
        assert_eq!(cad_jpy.basis, "aggregated");
        assert_eq!(cad_jpy.price_type, "reference");
    }

    fn with(edit: impl Fn(&mut serde_json::Value)) -> Result<FxBuild, BuildError> {
        let mut spec: serde_json::Value = serde_json::from_slice(&read(SPEC_PATH)).unwrap();
        edit(&mut spec);
        build(
            &serde_json::to_vec(&spec).unwrap(),
            &v1(),
            IdMap::default(),
            &mut fresh_mint(),
        )
    }

    #[test]
    fn rejects_inverse_duplicates_and_mixed_semantics() {
        let pairs = |s: &mut serde_json::Value| s["pairs"].as_array_mut().unwrap().clone();
        // MYR/SGD beside SGD/MYR.
        let err = with(|s| {
            let mut p = pairs(s);
            p.push(serde_json::json!({"pair": "MYR/SGD", "universes": ["fx-southeast-asia"], "feeds": []}));
            s["pairs"] = p.into();
        });
        assert!(matches!(err, Err(BuildError::Invalid(m)) if m.contains("inverse")));
        // A reference rate beside a venue quote on EUR/USD.
        let err = with(|s| {
            s["pairs"][0]["feeds"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"source": "fed-h10", "symbol": "RXI$US_N.B.EU"}));
        });
        assert!(matches!(err, Err(BuildError::Invalid(m)) if m.contains("never combined")));
        // Lower-case or unknown codes, and a pair of one currency.
        for bad in ["eur/usd", "EUR/XXXX", "USD/USD", "EURUSD"] {
            let err = with(|s| s["pairs"][0]["pair"] = bad.into());
            assert!(err.is_err(), "{bad}");
        }
    }
}
