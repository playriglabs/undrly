//! Normalizer for [`undrly_provider::curated`] reference data.
//!
//! Resolves dataset-local keys to the pinned canonical ids, validates every
//! identifier with its namespace's rules, maps vocabularies to canonical
//! enums, and checks relationship endpoints and feed shapes. Anything it does
//! not understand is an error.

use std::collections::HashMap;

use undrly_core::{
    AggregationMethod, AliasKind, CanonicalId, Category, Cik, Currency, CurrencyCode, CurrencyId,
    DEFAULT_STALE_AFTER_SECONDS, Decimal, DisplayName, Entity, EntityId, EntityKind, Figi,
    Instrument, InstrumentClass, InstrumentId, Isin, Lei, ListingId, Mic, ObservationBasis,
    PriceSubject, PriceType, PriceUnit, RelationshipType, SourceId, Timestamp, UnitOfMeasure,
    UniverseKey, UniverseMember, Venue, VenueId, VenueSymbol,
};
use undrly_provider::curated::Universe;

use crate::{NormalizeError, invalid};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedUniverse {
    pub currencies: Vec<(Currency, CurrencyCode)>,
    pub entities: Vec<(Entity, Option<Lei>, Option<Cik>)>,
    pub venues: Vec<(Venue, Option<Mic>)>,
    pub instruments: Vec<(Instrument, Option<Isin>, Option<Figi>)>,
    pub listings: Vec<NormalizedCuratedListing>,
    pub relationships: Vec<(CanonicalId, RelationshipType, CanonicalId)>,
    pub aliases: Vec<(CanonicalId, DisplayName, AliasKind)>,
    pub quote_feeds: Vec<NormalizedFeed>,
    pub quote_aggregations: Vec<(PriceSubject, PriceUnit, AggregationMethod)>,
    pub universes: Vec<NormalizedUniverseSnapshot>,
}

/// A universe snapshot: members plus the upstream raw file (source, record
/// key, SHA-256) that asserted them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedUniverseSnapshot {
    pub key: UniverseKey,
    pub source: SourceId,
    pub record_key: String,
    pub sha256: [u8; 32],
    pub as_of: Timestamp,
    pub members: Vec<UniverseMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCuratedListing {
    pub id: ListingId,
    pub instrument: InstrumentId,
    pub venue: VenueId,
    pub symbol: VenueSymbol,
    pub figi: Option<Figi>,
}

/// A quote feed without provenance (added by ingestion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedFeed {
    pub feed_source: SourceId,
    pub symbol: VenueSymbol,
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub basis: ObservationBasis,
    pub price_type: PriceType,
    pub stale_after_seconds: u32,
}

fn name(field: &'static str, value: &str) -> Result<DisplayName, NormalizeError> {
    DisplayName::new(value).map_err(|e| invalid(field, e))
}

fn id(field: &'static str, value: &str, category: Category) -> Result<CanonicalId, NormalizeError> {
    let id = CanonicalId::parse(value).map_err(|e| invalid(field, e))?;
    if id.category() == category {
        Ok(id)
    } else {
        Err(invalid(field, format!("`{value}` is not a {category} id")))
    }
}

fn unsupported(field: &'static str, value: &str) -> NormalizeError {
    NormalizeError::Unsupported {
        field,
        value: value.to_owned(),
    }
}

/// Normalizes the curated universe.
pub fn normalize_universe(u: &Universe) -> Result<NormalizedUniverse, NormalizeError> {
    let mut keys: HashMap<String, CanonicalId> = HashMap::new();
    let mut key = |k: &str, id: CanonicalId| -> Result<(), NormalizeError> {
        if keys.insert(k.to_owned(), id).is_some() {
            return Err(invalid("key", format!("duplicate key `{k}`")));
        }
        Ok(())
    };
    // Keys are inserted into a map borrowed by `key`; collect first, then use.
    let mut currencies = Vec::new();
    for c in &u.currencies {
        let id = id("currencies.id", &c.id, Category::Currency)?;
        key(&c.key, id)?;
        currencies.push((
            Currency {
                id: CurrencyId::try_from(id).expect("checked category"),
                name: name("currencies.name", &c.name)?,
            },
            CurrencyCode::normalize(&c.iso4217).map_err(|e| invalid("currencies.iso4217", e))?,
        ));
    }
    let mut entities = Vec::new();
    for e in &u.entities {
        let id = id("entities.id", &e.id, Category::Entity)?;
        key(&e.key, id)?;
        let kind = match e.kind.as_str() {
            "company" => EntityKind::Company,
            other => return Err(unsupported("entities.kind", other)),
        };
        entities.push((
            Entity {
                id: EntityId::try_from(id).expect("checked category"),
                kind,
                name: name("entities.name", &e.name)?,
            },
            e.lei
                .as_deref()
                .map(|v| Lei::normalize(v).map_err(|err| invalid("entities.lei", err)))
                .transpose()?,
            e.cik
                .as_deref()
                .map(|v| Cik::normalize(v).map_err(|err| invalid("entities.cik", err)))
                .transpose()?,
        ));
    }
    let mut venues = Vec::new();
    for v in &u.venues {
        let id = id("venues.id", &v.id, Category::Venue)?;
        key(&v.key, id)?;
        venues.push((
            Venue {
                id: VenueId::try_from(id).expect("checked category"),
                name: name("venues.name", &v.name)?,
            },
            v.mic
                .as_deref()
                .map(|m| Mic::normalize(m).map_err(|err| invalid("venues.mic", err)))
                .transpose()?,
        ));
    }
    let mut instruments = Vec::new();
    for i in &u.instruments {
        let id = id("instruments.id", &i.id, Category::Instrument)?;
        key(&i.key, id)?;
        let class = InstrumentClass::ALL
            .into_iter()
            .find(|c| c.as_str() == i.class)
            .ok_or_else(|| unsupported("instruments.class", &i.class))?;
        instruments.push((
            Instrument {
                id: InstrumentId::try_from(id).expect("checked category"),
                class,
                name: name("instruments.name", &i.name)?,
                contract_multiplier: i
                    .contract_multiplier
                    .as_deref()
                    .map(|m| {
                        let d = crate::decimal("instruments.contractMultiplier", m)?;
                        if d > Decimal::ZERO {
                            Ok(d)
                        } else {
                            Err(invalid(
                                "instruments.contractMultiplier",
                                "must be positive",
                            ))
                        }
                    })
                    .transpose()?,
                unit_of_measure: i
                    .unit_of_measure
                    .as_deref()
                    .map(|v| {
                        UnitOfMeasure::ALL
                            .into_iter()
                            .find(|x| x.as_str() == v)
                            .ok_or_else(|| unsupported("instruments.unitOfMeasure", v))
                    })
                    .transpose()?,
            },
            i.isin
                .as_deref()
                .map(|v| Isin::normalize(v).map_err(|err| invalid("instruments.isin", err)))
                .transpose()?,
            i.figi
                .as_deref()
                .map(|v| Figi::normalize(v).map_err(|err| invalid("instruments.figi", err)))
                .transpose()?,
        ));
    }
    let mut listings = Vec::new();
    for l in &u.listings {
        let lid = id("listings.id", &l.id, Category::Listing)?;
        key(&l.key, lid)?;
        listings.push((l, lid));
    }

    // A reference is a key defined in this file, or the canonical id of an
    // object that already exists (e.g. a V1 instrument a snapshot reuses).
    let lookup = |field: &'static str, k: &str| -> Result<CanonicalId, NormalizeError> {
        if k.starts_with("undrly:") {
            return CanonicalId::parse(k).map_err(|e| invalid(field, e));
        }
        keys.get(k)
            .copied()
            .ok_or_else(|| invalid(field, format!("unknown key `{k}`")))
    };
    let typed = |field: &'static str, id: CanonicalId, want: Category| {
        if id.category() == want {
            Ok(id)
        } else {
            Err(invalid(field, format!("`{id}` is not a {want}")))
        }
    };

    let listings = listings
        .into_iter()
        .map(|(l, lid)| {
            Ok(NormalizedCuratedListing {
                id: ListingId::try_from(lid).expect("checked category"),
                instrument: InstrumentId::try_from(typed(
                    "listings.instrument",
                    lookup("listings.instrument", &l.instrument)?,
                    Category::Instrument,
                )?)
                .expect("checked category"),
                venue: VenueId::try_from(typed(
                    "listings.venue",
                    lookup("listings.venue", &l.venue)?,
                    Category::Venue,
                )?)
                .expect("checked category"),
                symbol: VenueSymbol::new(&l.symbol).map_err(|e| invalid("listings.symbol", e))?,
                figi: l
                    .figi
                    .as_deref()
                    .map(|v| Figi::normalize(v).map_err(|err| invalid("listings.figi", err)))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    let relationships = u
        .relationships
        .iter()
        .map(|(s, t, o)| {
            let kind: RelationshipType = t
                .parse()
                .map_err(|_| unsupported("relationships.type", t))?;
            let subject = lookup("relationships.subject", s)?;
            let object = lookup("relationships.object", o)?;
            if !kind
                .allowed_endpoints()
                .contains(&(subject.category(), object.category()))
            {
                return Err(invalid(
                    "relationships",
                    format!("{kind} cannot connect `{s}` to `{o}`"),
                ));
            }
            Ok((subject, kind, object))
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    let aliases = u
        .aliases
        .iter()
        .map(|a| {
            let kind = AliasKind::ALL
                .into_iter()
                .find(|k| k.as_str() == a.kind)
                .ok_or_else(|| unsupported("aliases.kind", &a.kind))?;
            Ok((
                lookup("aliases.node", &a.node)?,
                name("aliases.alias", &a.alias)?,
                kind,
            ))
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    let quote_feeds = u
        .quote_feeds
        .iter()
        .map(|f| {
            let subject = match lookup("quoteFeeds.subject", &f.subject)? {
                id if id.category() == Category::Instrument => {
                    PriceSubject::Instrument(id.try_into().expect("checked category"))
                }
                id if id.category() == Category::Currency => {
                    PriceSubject::Currency(id.try_into().expect("checked category"))
                }
                _ => {
                    return Err(invalid(
                        "quoteFeeds.subject",
                        "not an instrument or currency",
                    ));
                }
            };
            let unit = match lookup("quoteFeeds.unit", &f.unit)? {
                id if id.category() == Category::Instrument => {
                    PriceUnit::Asset(id.try_into().expect("checked category"))
                }
                id if id.category() == Category::Currency => {
                    PriceUnit::Currency(id.try_into().expect("checked category"))
                }
                _ => return Err(invalid("quoteFeeds.unit", "not a currency or asset")),
            };
            if subject.canonical() == unit.canonical() {
                return Err(invalid(
                    "quoteFeeds",
                    "a subject cannot be priced in itself",
                ));
            }
            let basis = match (f.basis.as_str(), f.venue.as_deref()) {
                ("venue", Some(v)) => ObservationBasis::Venue(
                    VenueId::try_from(typed(
                        "quoteFeeds.venue",
                        lookup("quoteFeeds.venue", v)?,
                        Category::Venue,
                    )?)
                    .expect("checked category"),
                ),
                ("aggregated", None) => ObservationBasis::Aggregated,
                ("derived", None) => ObservationBasis::Derived,
                (other, _) => return Err(unsupported("quoteFeeds.basis/venue", other)),
            };
            Ok(NormalizedFeed {
                feed_source: SourceId::parse(&f.source)
                    .map_err(|e| invalid("quoteFeeds.source", e))?,
                symbol: VenueSymbol::new(&f.symbol).map_err(|e| invalid("quoteFeeds.symbol", e))?,
                subject,
                unit,
                basis,
                price_type: PriceType::ALL
                    .into_iter()
                    .find(|t| t.as_str() == f.price_type)
                    .ok_or_else(|| unsupported("quoteFeeds.priceType", &f.price_type))?,
                stale_after_seconds: match f.stale_after_seconds {
                    None => DEFAULT_STALE_AFTER_SECONDS,
                    Some(0) => {
                        return Err(invalid("quoteFeeds.staleAfterSeconds", "must be positive"));
                    }
                    Some(s) => s,
                },
            })
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    let quote_aggregations = u
        .quote_aggregations
        .iter()
        .map(|a| {
            let subject = match lookup("quoteAggregations.subject", &a.subject)? {
                id if id.category() == Category::Instrument => {
                    PriceSubject::Instrument(id.try_into().expect("checked category"))
                }
                id if id.category() == Category::Currency => {
                    PriceSubject::Currency(id.try_into().expect("checked category"))
                }
                _ => return Err(invalid("quoteAggregations.subject", "not priceable")),
            };
            let unit = match lookup("quoteAggregations.unit", &a.unit)? {
                id if id.category() == Category::Instrument => {
                    PriceUnit::Asset(id.try_into().expect("checked category"))
                }
                id if id.category() == Category::Currency => {
                    PriceUnit::Currency(id.try_into().expect("checked category"))
                }
                _ => return Err(invalid("quoteAggregations.unit", "not a unit")),
            };
            let method = AggregationMethod::ALL
                .into_iter()
                .find(|m| m.as_str() == a.method)
                .ok_or_else(|| unsupported("quoteAggregations.method", &a.method))?;
            Ok((subject, unit, method))
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    let universes = u
        .universes
        .iter()
        .map(|x| {
            let members = x
                .members
                .iter()
                .map(|m| {
                    Ok(UniverseMember {
                        node: lookup("universes.members.node", &m.node)?,
                        rank: m.rank,
                        source_symbol: m.source_symbol.clone(),
                    })
                })
                .collect::<Result<Vec<_>, NormalizeError>>()?;
            Ok(NormalizedUniverseSnapshot {
                key: x.key.parse().map_err(|e| invalid("universes.key", e))?,
                source: SourceId::parse(&x.source).map_err(|e| invalid("universes.source", e))?,
                record_key: x.record_key.clone(),
                sha256: hex32(&x.sha256)
                    .ok_or_else(|| invalid("universes.sha256", "expected 64 hex digits"))?,
                as_of: crate::timestamp("universes.asOf", &x.as_of)?,
                members,
            })
        })
        .collect::<Result<Vec<_>, NormalizeError>>()?;

    Ok(NormalizedUniverse {
        currencies,
        entities,
        venues,
        instruments,
        listings,
        relationships,
        aliases,
        quote_feeds,
        quote_aggregations,
        universes,
    })
}

fn hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use undrly_provider::ReferenceDataProvider;
    use undrly_provider::curated::CuratedProvider;

    use super::*;

    fn universe() -> Universe {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/demo/universe.json");
        CuratedProvider::new()
            .decode_reference(&std::fs::read(path).unwrap())
            .unwrap()
    }

    #[test]
    fn normalizes_the_demo_universe() {
        let n = normalize_universe(&universe()).unwrap();
        assert_eq!(n.currencies.len(), 2);
        assert_eq!(n.instruments.len(), 5);
        let classes: Vec<InstrumentClass> = n.instruments.iter().map(|(i, _, _)| i.class).collect();
        assert_eq!(
            classes,
            vec![
                InstrumentClass::Equity,
                InstrumentClass::CryptoAsset,
                InstrumentClass::CryptoAsset,
                InstrumentClass::Commodity,
                InstrumentClass::PerpetualFuture,
            ]
        );
        // The perpetual derives from Bitcoin and settles in USDC.
        let perp = n.instruments[4].0.id.canonical();
        let btc = n.instruments[1].0.id.canonical();
        let usdc = n.instruments[2].0.id.canonical();
        assert!(
            n.relationships
                .contains(&(perp, RelationshipType::DerivesFrom, btc))
        );
        assert!(
            n.relationships
                .contains(&(perp, RelationshipType::SettlesIn, usdc))
        );
        // FX is a currency priced in a currency; the perp is priced in USDC.
        assert!(
            n.quote_feeds
                .iter()
                .any(|f| matches!(f.subject, PriceSubject::Currency(_)))
        );
        let perp_feed = n
            .quote_feeds
            .iter()
            .find(|f| f.symbol.as_str() == "BTC")
            .unwrap();
        assert_eq!(perp_feed.unit.canonical(), usdc);
        assert_eq!(perp_feed.price_type, PriceType::Mark);
        let nvda_feed = n
            .quote_feeds
            .iter()
            .find(|f| f.symbol.as_str() == "NVDA")
            .unwrap();
        assert!(matches!(nvda_feed.basis, ObservationBasis::Venue(_)));
    }

    #[test]
    fn normalizes_the_commodity_list() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/reference/commodities.json");
        let u = CuratedProvider::new()
            .decode_reference(&std::fs::read(path).unwrap())
            .unwrap();
        let n = normalize_universe(&u).unwrap();
        assert_eq!(n.instruments.len(), 13);
        assert!(n.instruments.iter().all(|(i, _, _)| {
            i.class == InstrumentClass::Commodity && i.unit_of_measure.is_some()
        }));
        let stale: Vec<(&str, PriceType, u32)> = n
            .quote_feeds
            .iter()
            .map(|f| (f.feed_source.as_str(), f.price_type, f.stale_after_seconds))
            .collect();
        assert!(stale.contains(&("eia", PriceType::Reference, 1_209_600)));
        assert!(stale.contains(&("worldbank", PriceType::Average, 5_356_800)));
        assert!(stale.contains(&("gold-api", PriceType::Reference, 300)));
        // The unit is V1's USD, referenced by canonical id.
        let usd = normalize_universe(&universe()).unwrap().currencies[0].0.id;
        assert!(
            n.quote_feeds
                .iter()
                .all(|f| f.unit == PriceUnit::Currency(usd))
        );
    }

    #[test]
    fn rejects_bad_references_and_vocabulary() {
        let mut u = universe();
        u.relationships
            .push(("btc-perp".into(), "DERIVES_FROM".into(), "usd".into()));
        assert!(
            normalize_universe(&u).is_err(),
            "perp cannot derive from a currency"
        );
        let mut u = universe();
        u.quote_feeds[0].subject = "missing".into();
        assert!(normalize_universe(&u).is_err());
        let mut u = universe();
        u.instruments[0].class = "bond".into();
        assert!(matches!(
            normalize_universe(&u),
            Err(NormalizeError::Unsupported { .. })
        ));
        let mut u = universe();
        u.quote_feeds[3].venue = Some("kraken".into());
        assert!(
            normalize_universe(&u).is_err(),
            "aggregated feed names no venue"
        );
    }
}
