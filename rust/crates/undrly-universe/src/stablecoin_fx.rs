//! V1.9 stablecoin/fiat markets and derived FX crosses (docs/v1.9-live-fx.md).
//!
//! ```text
//! data/reference/stablecoin-fx-spec.json (Undrly-authored) + its id map
//!   + V1 curated universe + the FX id map (currencies, FX pairs)
//!   → build (pure) → data/reference/stablecoin-fx.json (curated format) + report
//! ```
//!
//! - A **market** is a stablecoin instrument priced in a fiat currency
//!   (`USDT` in `IDR`), like BTC in USD: no new instrument class. Its feeds
//!   are venue order books; two or more feeds → `mean-venue-mid-v1`.
//! - A **derived** pair is an existing FX instrument `USD/X` priced as
//!   `(S/X) / (S/USD)` through stablecoin `S` (`cross-via-stablecoin-v1`).
//!   Both legs must be markets of this spec.
//! - Stablecoins are referenced by V1 key (`usdt`, `usdc`); a stablecoin
//!   Undrly does not know yet is declared here (`EURC`).
//!
//! Seeded after the FX file and the V1.1 universe, whose currencies and FX
//! instruments it references by canonical id. Same inputs, same bytes; ids
//! come only from the id maps (`venue:<key>`, `asset:<CODE>`), minted once
//! for new keys.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Deserialize;
use undrly_core::{AggregationMethod, CanonicalId, Category, CurrencyCode};
use undrly_provider::curated::{
    AliasRecord, InstrumentRecord, QuoteAggregationRecord, QuoteDerivationRecord, QuoteFeedRecord,
    Universe, VenueRecord,
};

use crate::{BuildError, IdMap, sha256_hex};

/// Where the spec lives; also its record key when seeded.
pub const SPEC_PATH: &str = "data/reference/stablecoin-fx-spec.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Spec {
    pub dataset: String,
    pub version: u32,
    pub as_of: String,
    pub description: String,
    pub sources: BTreeMap<String, SourceDefaults>,
    pub venues: Vec<SpecVenue>,
    pub assets: Vec<SpecAsset>,
    pub markets: Vec<SpecMarket>,
    pub derived: Vec<SpecDerived>,
    pub excluded: Vec<SpecExclusion>,
}

/// How a venue source's feeds are declared (always a venue's own book).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDefaults {
    pub venue: String,
    pub price_type: String,
    pub stale_after_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecVenue {
    pub key: String,
    pub name: String,
}

/// A stablecoin: a V1 instrument key, or a new instrument with its name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecAsset {
    pub code: String,
    #[serde(default)]
    pub instrument: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecMarket {
    pub asset: String,
    pub unit: String,
    pub feeds: Vec<SpecFeed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecFeed {
    pub source: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecDerived {
    /// `BASE/QUOTE`, an FX pair of the FX spec.
    pub pair: String,
    /// The stablecoin both legs price.
    pub via: String,
}

/// A venue book researched and left out, with why.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecExclusion {
    pub source: String,
    pub symbol: String,
    pub reason: String,
}

pub struct StablecoinFxBuild {
    pub universe: Universe,
    pub ids: IdMap,
    pub report: String,
    pub minted: usize,
}

fn invalid(message: impl Into<String>) -> BuildError {
    BuildError::Invalid(format!("stablecoin-fx spec: {}", message.into()))
}

fn currency_code(field: &str, code: &str) -> Result<(), BuildError> {
    let parsed =
        CurrencyCode::parse(code).map_err(|e| invalid(format!("{field} `{code}`: {e}")))?;
    if parsed.as_str() != code {
        return Err(invalid(format!("{field} `{code}`: codes are upper case")));
    }
    Ok(())
}

/// Builds the stablecoin markets and derived crosses from `spec_bytes` (the
/// file at [`SPEC_PATH`]). `fx_ids` is the FX build's id map (read only);
/// `mint` supplies ids for keys `ids` does not have.
pub fn build(
    spec_bytes: &[u8],
    v1: &Universe,
    fx_ids: &IdMap,
    ids: IdMap,
    mint: &mut dyn FnMut(Category) -> CanonicalId,
) -> Result<StablecoinFxBuild, BuildError> {
    let spec: Spec =
        serde_json::from_slice(spec_bytes).map_err(|e| invalid(format!("decode: {e}")))?;
    if spec.dataset != "undrly-stablecoin-fx-spec" || spec.version != 1 {
        return Err(invalid("unexpected dataset or version"));
    }
    let v1_id = |key: &str| -> Result<String, BuildError> {
        v1.venues
            .iter()
            .map(|x| (x.key.as_str(), x.id.as_str()))
            .chain(
                v1.instruments
                    .iter()
                    .map(|x| (x.key.as_str(), x.id.as_str())),
            )
            .find(|(k, _)| *k == key)
            .map(|(_, id)| id.to_owned())
            .ok_or_else(|| BuildError::V1(format!("no object with key `{key}`")))
    };
    let fx_id =
        |key: &str| -> Result<String, BuildError> {
            fx_ids.ids.get(key).cloned().ok_or_else(|| {
                BuildError::IdMap(format!("FX id map has no `{key}` (run `fx build`)"))
            })
        };

    let mut ids = ids;
    ids.version = 1;
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

    let mut out = Universe {
        dataset: "undrly-stablecoin-fx".into(),
        version: 1,
        description: "Generated by `undrly-collect fx build` from data/reference/stablecoin-fx-spec.json (docs/v1.9-live-fx.md). Undrly-authored reference data: venues, stablecoin/fiat feed declarations and derived FX crosses; no upstream prices.".into(),
        currencies: Vec::new(),
        entities: Vec::new(),
        venues: Vec::new(),
        instruments: Vec::new(),
        listings: Vec::new(),
        relationships: Vec::new(),
        aliases: Vec::new(),
        quote_feeds: Vec::new(),
        quote_aggregations: Vec::new(),
        quote_derivations: Vec::new(),
        universes: Vec::new(),
    };
    let mut aliases: BTreeSet<(String, String, String)> = BTreeSet::new();

    // Venues: V1's Kraken and Coinbase by id, new venues by key.
    let mut venue_ref: BTreeMap<String, String> = BTreeMap::new();
    for key in ["kraken", "coinbase"] {
        venue_ref.insert(key.into(), v1_id(key)?);
    }
    for v in &spec.venues {
        if venue_ref.contains_key(&v.key) {
            return Err(invalid(format!("venue {} declared twice", v.key)));
        }
        let key = format!("venue:{}", v.key);
        let id = id_of(&mut ids, &key, Category::Venue)?;
        out.venues.push(VenueRecord {
            key: key.clone(),
            id,
            name: v.name.clone(),
            mic: None,
        });
        aliases.insert((key.clone(), v.name.clone(), "name".into()));
        venue_ref.insert(v.key.clone(), key);
    }

    // Stablecoins.
    let mut asset_ref: BTreeMap<String, String> = BTreeMap::new();
    for a in &spec.assets {
        if asset_ref.contains_key(&a.code) {
            return Err(invalid(format!("asset {} declared twice", a.code)));
        }
        let r = match (&a.instrument, &a.name) {
            (Some(v1_key), None) => v1_id(v1_key)?,
            (None, Some(name)) => {
                let key = format!("asset:{}", a.code);
                let id = id_of(&mut ids, &key, Category::Instrument)?;
                out.instruments.push(InstrumentRecord {
                    key: key.clone(),
                    id,
                    class: "crypto_asset".into(),
                    name: name.clone(),
                    isin: None,
                    figi: None,
                    contract_multiplier: None,
                    unit_of_measure: None,
                    base: None,
                    quote: None,
                });
                aliases.insert((key.clone(), name.clone(), "name".into()));
                aliases.insert((key.clone(), a.code.clone(), "symbol".into()));
                key
            }
            _ => {
                return Err(invalid(format!(
                    "asset {}: either a V1 instrument key or a new instrument's name",
                    a.code
                )));
            }
        };
        asset_ref.insert(a.code.clone(), r);
    }

    // Markets.
    let excluded: BTreeSet<(&str, &str)> = spec
        .excluded
        .iter()
        .map(|e| (e.source.as_str(), e.symbol.as_str()))
        .collect();
    let mut seen_markets: BTreeSet<(String, String)> = BTreeSet::new();
    let mut seen_feeds: BTreeSet<(String, String)> = BTreeSet::new();
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for m in &spec.markets {
        let name = format!("{}/{}", m.asset, m.unit);
        currency_code("market unit", &m.unit)?;
        let asset = asset_ref
            .get(&m.asset)
            .ok_or_else(|| invalid(format!("{name}: undeclared asset")))?;
        let unit = fx_id(&format!("iso4217:{}", m.unit))?;
        if !seen_markets.insert((m.asset.clone(), m.unit.clone())) {
            return Err(invalid(format!("duplicate market {name}")));
        }
        if m.feeds.is_empty() {
            return Err(invalid(format!("{name}: no feeds")));
        }
        let mut described = Vec::new();
        for f in &m.feeds {
            let d = spec
                .sources
                .get(&f.source)
                .ok_or_else(|| invalid(format!("{name}: unknown source `{}`", f.source)))?;
            if !seen_feeds.insert((f.source.clone(), f.symbol.clone())) {
                return Err(invalid(format!(
                    "feed {}:{} declared twice",
                    f.source, f.symbol
                )));
            }
            if excluded.contains(&(f.source.as_str(), f.symbol.as_str())) {
                return Err(invalid(format!(
                    "feed {}:{} is excluded",
                    f.source, f.symbol
                )));
            }
            let venue = venue_ref
                .get(&d.venue)
                .cloned()
                .ok_or_else(|| invalid(format!("unknown venue `{}`", d.venue)))?;
            out.quote_feeds.push(QuoteFeedRecord {
                source: f.source.clone(),
                symbol: f.symbol.clone(),
                subject: asset.clone(),
                unit: unit.clone(),
                basis: "venue".into(),
                venue: Some(venue),
                price_type: d.price_type.clone(),
                stale_after_seconds: Some(d.stale_after_seconds),
                freshness_clock: None,
                inverted: false,
            });
            described.push(format!("{} `{}`", f.source, f.symbol));
        }
        let method = if m.feeds.len() >= 2 {
            out.quote_aggregations.push(QuoteAggregationRecord {
                subject: asset.clone(),
                unit: unit.clone(),
                method: AggregationMethod::MeanVenueMidV1.as_str().into(),
            });
            AggregationMethod::MeanVenueMidV1
        } else {
            AggregationMethod::LatestObservationV1
        };
        rows.push((name, described.join(", "), method.as_str().into()));
    }

    // Derived crosses.
    let mut seen_derived: BTreeSet<String> = BTreeSet::new();
    let mut derived_rows: Vec<(String, String)> = Vec::new();
    for d in &spec.derived {
        let (base, quote) = d
            .pair
            .split_once('/')
            .ok_or_else(|| invalid(format!("`{}` is not BASE/QUOTE", d.pair)))?;
        currency_code("derived pair", base)?;
        currency_code("derived pair", quote)?;
        if !seen_derived.insert(d.pair.clone()) {
            return Err(invalid(format!("{} derived twice", d.pair)));
        }
        let via = asset_ref
            .get(&d.via)
            .ok_or_else(|| invalid(format!("{}: undeclared asset {}", d.pair, d.via)))?;
        for leg in [quote, base] {
            if !seen_markets.contains(&(d.via.clone(), leg.to_owned())) {
                return Err(invalid(format!(
                    "{}: leg {}/{leg} is not a market of this spec",
                    d.pair, d.via
                )));
            }
        }
        out.quote_derivations.push(QuoteDerivationRecord {
            subject: fx_id(&format!("fx:{}", d.pair))?,
            unit: fx_id(&format!("iso4217:{quote}"))?,
            method: AggregationMethod::CrossViaStablecoinV1.as_str().into(),
            via: via.clone(),
            base: fx_id(&format!("iso4217:{base}"))?,
        });
        derived_rows.push((
            d.pair.clone(),
            format!("{}/{quote} ÷ {}/{base}", d.via, d.via),
        ));
    }

    out.aliases = aliases
        .into_iter()
        .map(|(node, alias, kind)| AliasRecord { node, alias, kind })
        .collect();
    let sha256 = sha256_hex(spec_bytes);
    let report = report(&spec, &rows, &derived_rows, &sha256, minted);
    Ok(StablecoinFxBuild {
        universe: out,
        ids,
        report,
        minted,
    })
}

fn report(
    spec: &Spec,
    rows: &[(String, String, String)],
    derived: &[(String, String)],
    sha256: &str,
    minted: usize,
) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "# Stablecoin FX build report\n");
    let _ = writeln!(
        r,
        "Generated by `undrly-collect fx build` from `{SPEC_PATH}` (SHA-256 `{}…`, as of {}). docs/v1.9-live-fx.md.\n",
        &sha256[..16],
        spec.as_of
    );
    let _ = writeln!(r, "## Markets ({})\n", rows.len());
    let _ = writeln!(r, "| Market | Feeds | Method |\n| --- | --- | --- |");
    for (m, feeds, method) in rows {
        let _ = writeln!(r, "| {m} | {feeds} | {method} |");
    }
    let _ = writeln!(r, "\n## Derived crosses ({})\n", derived.len());
    let _ = writeln!(r, "| Pair | Legs | Method |\n| --- | --- | --- |");
    for (pair, legs) in derived {
        let _ = writeln!(r, "| {pair} | {legs} | cross-via-stablecoin-v1 |");
    }
    let _ = writeln!(r, "\n## Excluded venue books ({})\n", spec.excluded.len());
    let _ = writeln!(r, "| Source | Symbol | Reason |\n| --- | --- | --- |");
    for e in &spec.excluded {
        let _ = writeln!(r, "| {} | `{}` | {} |", e.source, e.symbol, e.reason);
    }
    let _ = writeln!(r, "\n{minted} ids minted by this build.");
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1() -> Universe {
        serde_json::from_slice(
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../data/demo/universe.json"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn fx_ids() -> IdMap {
        serde_json::from_slice(
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../data/reference/fx-ids.json"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn spec() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(SPEC_PATH),
        )
        .unwrap()
    }

    fn empty() -> IdMap {
        IdMap {
            version: 1,
            ids: BTreeMap::new(),
        }
    }

    #[test]
    fn builds_markets_aggregations_and_derived_crosses_deterministically() {
        let mut mint = crate::mint;
        let a = build(&spec(), &v1(), &fx_ids(), empty(), &mut mint).unwrap();
        // EURC and six new venues are minted once ...
        assert_eq!(a.minted, 7);
        // ... and a second build with that map mints nothing and is identical.
        let mut mint = crate::mint;
        let b = build(&spec(), &v1(), &fx_ids(), a.ids.clone(), &mut mint).unwrap();
        assert_eq!(b.minted, 0);
        assert_eq!(
            serde_json::to_string(&a.universe).unwrap(),
            serde_json::to_string(&b.universe).unwrap()
        );
        let u = &a.universe;
        assert!(
            u.quote_feeds
                .iter()
                .all(|f| f.basis == "venue" && f.venue.is_some())
        );
        // Every market with two or more feeds is a mean of venue mids.
        assert!(
            u.quote_aggregations
                .iter()
                .all(|x| x.method == "mean-venue-mid-v1")
        );
        assert_eq!(u.quote_derivations.len(), 8);
        assert!(u.quote_derivations.iter().all(
            |d| d.method == "cross-via-stablecoin-v1" && d.base == fx_ids().ids["iso4217:USD"]
        ));
    }

    #[test]
    fn rejects_unknown_legs_excluded_feeds_and_duplicates() {
        let base: serde_json::Value = serde_json::from_slice(&spec()).unwrap();
        let reject = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut v = base.clone();
            edit(&mut v);
            let bytes = serde_json::to_vec(&v).unwrap();
            let mut mint = crate::mint;
            assert!(build(&bytes, &v1(), &fx_ids(), empty(), &mut mint).is_err());
        };
        // EURC has no IDR market: the leg is missing.
        reject(&|v| v["derived"][0]["via"] = "EURC".into());
        reject(&|v| v["derived"][0]["pair"] = "USD/JPY".into());
        reject(&|v| {
            v["markets"][0]["feeds"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"source": "kraken", "symbol": "USDTJPY"}))
        });
        reject(&|v| {
            let first = v["markets"][0].clone();
            v["markets"].as_array_mut().unwrap().push(first);
        });
        reject(&|v| v["markets"][0]["unit"] = "usd".into());
    }
}
