//! Canonical quotes: the aggregation boundary between source observations
//! and the product-facing price of a subject in a unit.
//!
//! ```text
//! observations (per source, per feed, with basis) → aggregate → canonical quote
//! ```
//!
//! The boundary exists even when only one observation is eligible. Methods
//! are named and versioned; a pair uses the method declared for it (default
//! [`AggregationMethod::LatestObservationV1`]):
//!
//! - `latest-observation-v1` selects the most recent observation by effective
//!   time. The canonical quote *is* that observation (its basis and venue).
//! - `mean-venue-mid-v1` averages the mids of fresh venue quotes. The result
//!   is `basis = aggregated`, attributed to no single venue.
//!
//! No method ranks providers, weights prices, detects outliers, or scores
//! confidence.

use chrono::TimeDelta;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::observation::{
    BidAsk, MarketObservation, ObservationBasis, PriceSubject, PriceType, PriceUnit,
};
use crate::reference::VenueSymbol;
use crate::source::{Provenance, SourceId};
use crate::time::Timestamp;

/// A quote feed: `feed_source` publishes, under its own `symbol`, the
/// `price_type` of `subject` in `unit`, at a venue or aggregated. The
/// declaration is itself a fact with provenance (e.g. curated reference
/// data); the observations the feed produces carry their own.
///
/// The symbol is the source's own spelling (e.g. Kraken's `XXBTZUSD`), with
/// venue-symbol rules: exact, no normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteFeed {
    pub feed_source: SourceId,
    pub symbol: VenueSymbol,
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub basis: ObservationBasis,
    pub price_type: PriceType,
    /// After how long the feed's latest observation counts as stale: its
    /// expected cadence (seconds for market data, days for daily reference
    /// series, weeks for monthly averages).
    pub stale_after_seconds: u32,
    pub provenance: Provenance,
}

/// Default freshness window of a feed (V1 behaviour).
pub const DEFAULT_STALE_AFTER_SECONDS: u32 = 300;

/// The aggregation method declared for a (subject, unit), with provenance.
/// Pairs without a declaration use [`AggregationMethod::LatestObservationV1`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteAggregation {
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub method: AggregationMethod,
    pub provenance: Provenance,
}

/// How a canonical quote was produced from observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregationMethod {
    /// The most recent eligible observation by effective time (source time,
    /// else receipt time). Ties go to the later receipt, then the greater key.
    LatestObservationV1,
    /// Arithmetic mean of venue mid prices:
    ///
    /// 1. eligible: each feed's latest observation that is a **venue** quote,
    ///    has **both bid and ask**, and is at most [`MEAN_VENUE_MID_MAX_AGE_SECONDS`]
    ///    old at compute time (effective time: source time, else receipt time);
    /// 2. `mid_i = (bid_i + ask_i) / 2`, exact, at scale
    ///    `max(scale(bid_i), scale(ask_i)) + 1`;
    /// 3. `price = Σ mid_i / n`, `bid = Σ bid_i / n`, `ask = Σ ask_i / n`, all
    ///    at one scale `max(scale(mid_i)) + 1`, rounded half to even (exact
    ///    for n ≤ 2). One scale keeps `bid ≤ price ≤ ask`.
    ///
    /// The bid and ask are the *means* of the same inputs' bids and asks, not
    /// a best bid/offer. One eligible observation yields its own mid, bid and
    /// ask (`eligible = 1`); zero yields no canonical quote. The result is
    /// `basis = aggregated`, price type `mid`, as of the oldest input's
    /// effective time.
    MeanVenueMidV1,
}

/// Freshness window of [`AggregationMethod::MeanVenueMidV1`], in seconds.
pub const MEAN_VENUE_MID_MAX_AGE_SECONDS: i64 = 30;

impl AggregationMethod {
    pub const ALL: [AggregationMethod; 2] = [
        AggregationMethod::LatestObservationV1,
        AggregationMethod::MeanVenueMidV1,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            AggregationMethod::LatestObservationV1 => "latest-observation-v1",
            AggregationMethod::MeanVenueMidV1 => "mean-venue-mid-v1",
        }
    }

    /// The freshness window observations must satisfy, if the method has one.
    pub const fn max_age_seconds(self) -> Option<i64> {
        match self {
            AggregationMethod::LatestObservationV1 => None,
            AggregationMethod::MeanVenueMidV1 => Some(MEAN_VENUE_MID_MAX_AGE_SECONDS),
        }
    }
}

/// The output of aggregation for one (subject, unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate<K> {
    pub method: AggregationMethod,
    pub price: Decimal,
    pub price_type: PriceType,
    pub basis: ObservationBasis,
    /// The single input's bid and ask (`latest-observation-v1`), or the mean
    /// bid and mean ask of the inputs (`mean-venue-mid-v1`).
    pub bid_ask: Option<BidAsk>,
    /// The oldest effective time among the inputs.
    pub as_of: Timestamp,
    /// Exactly the observations used, each with the price it contributed
    /// (its mid for `mean-venue-mid-v1`), ordered by key.
    pub inputs: Vec<(K, Decimal)>,
}

/// Aggregates `candidates` (one subject, one unit; the latest observation of
/// each feed, tagged with a caller key such as a storage id) with `method` at
/// `computed_at`. `None` when nothing is eligible. Pure and deterministic:
/// the result does not depend on input order.
pub fn aggregate<K: Ord + Clone>(
    method: AggregationMethod,
    candidates: &[(K, MarketObservation)],
    computed_at: Timestamp,
) -> Option<Aggregate<K>> {
    match method {
        AggregationMethod::LatestObservationV1 => {
            let (key, o) = select_latest(candidates)?;
            Some(Aggregate {
                method,
                price: o.price(),
                price_type: o.price_type(),
                basis: o.basis(),
                bid_ask: o.bid_ask(),
                as_of: o.effective_at(),
                inputs: vec![(key.clone(), o.price())],
            })
        }
        AggregationMethod::MeanVenueMidV1 => {
            let max_age = TimeDelta::seconds(MEAN_VENUE_MID_MAX_AGE_SECONDS);
            let mut inputs: Vec<MeanInput<K>> = candidates
                .iter()
                .filter(|(_, o)| matches!(o.basis(), ObservationBasis::Venue(_)))
                .filter(|(_, o)| {
                    computed_at.as_datetime() - o.effective_at().as_datetime() <= max_age
                })
                .filter_map(|(k, o)| {
                    let ba = o.bid_ask()?;
                    Some(MeanInput {
                        key: k.clone(),
                        mid: mid_price(ba.bid, ba.ask)?,
                        bid_ask: ba,
                        at: o.effective_at(),
                    })
                })
                .collect();
            if inputs.is_empty() {
                return None;
            }
            inputs.sort_by(|a, b| a.key.cmp(&b.key));
            let scale = inputs.iter().map(|i| i.mid.scale()).max()? + 1;
            let n = Decimal::from(inputs.len());
            let mean = |value: fn(&MeanInput<K>) -> Decimal| -> Option<Decimal> {
                let sum = inputs
                    .iter()
                    .try_fold(Decimal::ZERO, |acc, i| acc.checked_add(value(i)))?;
                Some(rescaled(sum.checked_div(n)?, scale))
            };
            let price = mean(|i| i.mid)?;
            let bid_ask = BidAsk {
                bid: mean(|i| i.bid_ask.bid)?,
                ask: mean(|i| i.bid_ask.ask)?,
            };
            Some(Aggregate {
                method,
                price,
                price_type: PriceType::Mid,
                basis: ObservationBasis::Aggregated,
                bid_ask: Some(bid_ask),
                as_of: inputs.iter().map(|i| i.at).min()?,
                inputs: inputs.into_iter().map(|i| (i.key, i.mid)).collect(),
            })
        }
    }
}

/// One eligible input of [`AggregationMethod::MeanVenueMidV1`].
struct MeanInput<K> {
    key: K,
    mid: Decimal,
    bid_ask: BidAsk,
    at: Timestamp,
}

/// `(bid + ask) / 2` at scale `max(scale(bid), scale(ask)) + 1`: exact, the
/// one mid-price rule used by normalizers and aggregation alike.
pub fn mid_price(bid: Decimal, ask: Decimal) -> Option<Decimal> {
    let scale = bid.scale().max(ask.scale()) + 1;
    Some(rescaled(
        bid.checked_add(ask)?.checked_div(Decimal::TWO)?,
        scale,
    ))
}

fn rescaled(value: Decimal, scale: u32) -> Decimal {
    let mut v = value.round_dp_with_strategy(scale, RoundingStrategy::MidpointNearestEven);
    v.rescale(scale);
    v
}

/// Selects the canonical observation among `candidates` (one subject, one
/// unit), each tagged with a caller key such as a storage id. `None` when
/// there are no candidates. Pure and deterministic: the result does not
/// depend on input order.
pub fn select_latest<K: Ord>(
    candidates: &[(K, MarketObservation)],
) -> Option<&(K, MarketObservation)> {
    candidates.iter().max_by(|(ka, a), (kb, b)| {
        a.effective_at()
            .cmp(&b.effective_at())
            .then(a.received_at().cmp(&b.received_at()))
            .then(ka.cmp(kb))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::parse_canonical;
    use crate::id::{CurrencyId, InstrumentId};
    use crate::observation::{ObservationBasis, PriceSubject, PriceType, PriceUnit};
    use crate::source::SourceId;
    use crate::time::Timestamp;

    fn obs(observed_at: Option<&str>, received_at: &str) -> MarketObservation {
        MarketObservation::new(
            PriceSubject::Instrument(InstrumentId::generate()),
            ObservationBasis::Aggregated,
            PriceType::Last,
            parse_canonical("1.0").unwrap(),
            None,
            PriceUnit::Currency(CurrencyId::generate()),
            SourceId::parse("example-source").unwrap(),
            observed_at.map(|t| Timestamp::parse(t).unwrap()),
            Timestamp::parse(received_at).unwrap(),
        )
        .unwrap()
    }

    fn venue_obs(
        bid: &str,
        ask: &str,
        observed_at: Option<&str>,
        received_at: &str,
    ) -> MarketObservation {
        MarketObservation::new(
            PriceSubject::Instrument(InstrumentId::generate()),
            ObservationBasis::Venue(crate::id::VenueId::generate()),
            PriceType::Last,
            parse_canonical(ask).unwrap(),
            Some(crate::observation::BidAsk {
                bid: parse_canonical(bid).unwrap(),
                ask: parse_canonical(ask).unwrap(),
            }),
            PriceUnit::Currency(CurrencyId::generate()),
            SourceId::parse("example-source").unwrap(),
            observed_at.map(|t| Timestamp::parse(t).unwrap()),
            Timestamp::parse(received_at).unwrap(),
        )
        .unwrap()
    }

    fn at(t: &str) -> Timestamp {
        Timestamp::parse(t).unwrap()
    }

    #[test]
    fn mean_venue_mid_averages_fresh_venue_mids_exactly() {
        let kraken = (
            1,
            venue_obs("84145.90000", "84146.00000", None, "2026-09-25T00:00:10Z"),
        );
        let coinbase = (
            2,
            venue_obs(
                "83984.36",
                "83984.37",
                Some("2026-09-25T00:00:05Z"),
                "2026-09-25T00:00:06Z",
            ),
        );
        let now = at("2026-09-25T00:00:20Z");
        for order in [
            vec![kraken.clone(), coinbase.clone()],
            vec![coinbase.clone(), kraken.clone()],
        ] {
            let a = aggregate(AggregationMethod::MeanVenueMidV1, &order, now).unwrap();
            // mids 84145.950000 and 83984.365; mean at scale 7.
            assert_eq!(a.price.to_string(), "84065.1575000");
            // Mean bid and mean ask of the same two inputs, same scale.
            assert_eq!(
                bid_ask_of(&a),
                ("84065.1300000".to_owned(), "84065.1850000".to_owned())
            );
            assert_eq!(a.basis, ObservationBasis::Aggregated);
            assert_eq!(a.price_type, PriceType::Mid);
            assert_eq!(a.as_of, at("2026-09-25T00:00:05Z"), "oldest input");
            let inputs: Vec<(i32, String)> =
                a.inputs.iter().map(|(k, m)| (*k, m.to_string())).collect();
            assert_eq!(
                inputs,
                vec![(1, "84145.950000".to_owned()), (2, "83984.365".to_owned())]
            );
        }
    }

    fn bid_ask_of<K>(a: &Aggregate<K>) -> (String, String) {
        let ba = a.bid_ask.expect("mean-venue-mid-v1 states bid and ask");
        (ba.bid.to_string(), ba.ask.to_string())
    }

    #[test]
    fn mean_venue_mid_bid_and_ask_are_means_of_input_bids_and_asks() {
        // SUI/USD: Coinbase 1.1819/1.1821 (mid 1.1820), Kraken 1.1803/1.1805
        // (mid 1.1804).
        let coinbase = (
            1,
            venue_obs(
                "1.1819",
                "1.1821",
                Some("2026-09-25T00:00:05Z"),
                "2026-09-25T00:00:06Z",
            ),
        );
        let kraken = (
            2,
            venue_obs("1.1803", "1.1805", None, "2026-09-25T00:00:07Z"),
        );
        let now = at("2026-09-25T00:00:20Z");
        let mut seen = Vec::new();
        for order in [
            vec![coinbase.clone(), kraken.clone()],
            vec![kraken.clone(), coinbase.clone()],
        ] {
            let a = aggregate(AggregationMethod::MeanVenueMidV1, &order, now).unwrap();
            // price = (1.1820 + 1.1804) / 2; bid = (1.1819 + 1.1803) / 2;
            // ask = (1.1821 + 1.1805) / 2; one scale (mid scale 5, plus 1).
            assert_eq!(a.price.to_string(), "1.181200");
            assert_eq!(
                bid_ask_of(&a),
                ("1.181100".to_owned(), "1.181300".to_owned())
            );
            let ba = a.bid_ask.unwrap();
            assert!(ba.bid <= a.price && a.price <= ba.ask);
            // Not a best bid/offer: max(bid) would be 1.1819, min(ask) 1.1805.
            assert_ne!(ba.bid, parse_canonical("1.1819").unwrap());
            assert_eq!(a.basis, ObservationBasis::Aggregated);
            assert_eq!(a.price_type, PriceType::Mid);
            assert_eq!(
                a.inputs
                    .iter()
                    .map(|(k, m)| (*k, m.to_string()))
                    .collect::<Vec<_>>(),
                vec![(1, "1.18200".to_owned()), (2, "1.18040".to_owned())]
            );
            seen.push(a);
        }
        assert_eq!(seen[0], seen[1], "input order changes nothing");
    }

    #[test]
    fn mean_venue_mid_bid_price_ask_stay_ordered_when_rounded() {
        // Three inputs: the means are not exact, so all three are rounded at
        // one scale; bid <= price <= ask must still hold, in any order.
        let a = (1, venue_obs("0.01", "0.02", None, "2026-09-25T00:00:01Z"));
        let b = (2, venue_obs("0.01", "0.01", None, "2026-09-25T00:00:02Z"));
        let c = (3, venue_obs("0.02", "0.02", None, "2026-09-25T00:00:03Z"));
        let now = at("2026-09-25T00:00:10Z");
        let mut seen = Vec::new();
        for order in [
            vec![a.clone(), b.clone(), c.clone()],
            vec![c.clone(), a.clone(), b.clone()],
            vec![b.clone(), c.clone(), a.clone()],
        ] {
            let agg = aggregate(AggregationMethod::MeanVenueMidV1, &order, now).unwrap();
            let ba = agg.bid_ask.unwrap();
            assert!(ba.bid <= agg.price && agg.price <= ba.ask, "{agg:?}");
            seen.push(agg);
        }
        assert!(seen.windows(2).all(|w| w[0] == w[1]));
        // mids 0.015, 0.010, 0.020: mean 0.015 at scale 4; bids 0.0133…,
        // asks 0.0166… rounded half to even at the same scale.
        assert_eq!(seen[0].price.to_string(), "0.0150");
        assert_eq!(
            bid_ask_of(&seen[0]),
            ("0.0133".to_owned(), "0.0167".to_owned())
        );
    }

    #[test]
    fn mean_venue_mid_freshness_and_fallback() {
        let kraken = (1, venue_obs("100.0", "100.2", None, "2026-09-25T00:00:40Z"));
        let coinbase = (
            2,
            venue_obs(
                "99.9",
                "100.1",
                Some("2026-09-25T00:00:00Z"),
                "2026-09-25T00:00:01Z",
            ),
        );
        let both = [kraken.clone(), coinbase.clone()];
        let method = AggregationMethod::MeanVenueMidV1;
        // At 00:00:30 both are within 30 s.
        assert_eq!(
            aggregate(method, &both, at("2026-09-25T00:00:30Z"))
                .unwrap()
                .inputs
                .len(),
            2
        );
        // At 00:00:45 Coinbase (00:00:00) is stale: one input, its own mid.
        let one = aggregate(method, &both, at("2026-09-25T00:00:45Z")).unwrap();
        assert_eq!(
            one.inputs.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(one.price.to_string(), "100.100");
        assert_eq!(one.basis, ObservationBasis::Aggregated);
        // The stale Coinbase quote contributes nothing: bid and ask are
        // Kraken's own (100.0 / 100.2), at the aggregate's scale.
        let ba = one.bid_ask.unwrap();
        assert_eq!(ba.bid, parse_canonical("100.0").unwrap());
        assert_eq!(ba.ask, parse_canonical("100.2").unwrap());
        assert_eq!(
            bid_ask_of(&one),
            ("100.000".to_owned(), "100.200".to_owned())
        );
        // Both fresh: bid and ask are the means of both.
        let two = aggregate(method, &both, at("2026-09-25T00:00:30Z")).unwrap();
        assert_eq!(
            bid_ask_of(&two),
            ("99.950".to_owned(), "100.150".to_owned())
        );
        assert_eq!(two.price.to_string(), "100.050");
        // At 00:01:11 both are stale: no canonical quote.
        assert!(aggregate(method, &both, at("2026-09-25T00:01:11Z")).is_none());
    }

    #[test]
    fn mean_venue_mid_requires_venue_quotes_with_bid_and_ask() {
        let now = at("2026-09-25T00:00:10Z");
        let no_spread = (1, obs(None, "2026-09-25T00:00:05Z")); // aggregated, no bid/ask
        assert!(aggregate(AggregationMethod::MeanVenueMidV1, &[no_spread], now).is_none());
    }

    #[test]
    fn a_single_observation_is_selected() {
        let only = [(1, obs(None, "2026-09-25T00:00:00Z"))];
        assert_eq!(select_latest(&only).unwrap().0, 1);
        assert!(select_latest::<i32>(&[]).is_none());
    }

    #[test]
    fn selects_by_effective_time_regardless_of_order() {
        let a = (1, obs(Some("2026-09-25T00:00:05Z"), "2026-09-25T00:00:06Z"));
        // No source time: effective time is receipt time.
        let b = (2, obs(None, "2026-09-25T00:00:07Z"));
        let c = (3, obs(Some("2026-09-25T00:00:01Z"), "2026-09-25T00:00:09Z"));
        for order in [
            vec![a.clone(), b.clone(), c.clone()],
            vec![c.clone(), b.clone(), a.clone()],
        ] {
            assert_eq!(select_latest(&order).unwrap().0, 2);
        }
    }
}
