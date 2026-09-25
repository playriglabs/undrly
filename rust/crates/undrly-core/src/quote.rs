//! Canonical quotes: the aggregation boundary between source observations
//! and the product-facing price of a subject in a unit.
//!
//! ```text
//! observations (per source, per feed, with basis) → aggregate → canonical quote
//! ```
//!
//! The boundary exists even when only one observation is eligible. Version 1
//! ([`AggregationMethod::LatestObservationV1`]) selects the most recent
//! observation by effective time. It does not rank providers, weight prices,
//! detect outliers, or score confidence: with one feed per market it simply
//! selects that feed's latest observation.

use crate::observation::{MarketObservation, ObservationBasis, PriceSubject, PriceType, PriceUnit};
use crate::reference::VenueSymbol;
use crate::source::{Provenance, SourceId};

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
    pub provenance: Provenance,
}

/// How a canonical quote was produced from observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregationMethod {
    /// The most recent eligible observation by effective time (source time,
    /// else receipt time). Ties go to the later receipt, then the greater key.
    LatestObservationV1,
}

impl AggregationMethod {
    pub const ALL: [AggregationMethod; 1] = [AggregationMethod::LatestObservationV1];

    pub const fn as_str(self) -> &'static str {
        match self {
            AggregationMethod::LatestObservationV1 => "latest-observation-v1",
        }
    }
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
