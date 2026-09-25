//! Normalized market observations.

use rust_decimal::Decimal;

use crate::id::{CurrencyId, InstrumentId, VenueId};
use crate::source::SourceId;
use crate::time::Timestamp;

/// What market a price describes.
///
/// Encoded as an enum so a venue-specific observation always names its venue
/// and an aggregated or derived value can never be labeled as a venue quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationBasis {
    /// Reported for trading on one venue.
    Venue(VenueId),
    /// Combined by the source across several venues or contributors.
    Aggregated,
    /// Computed from other values (e.g. a cross rate).
    Derived,
}

impl ObservationBasis {
    pub const NAMES: [&'static str; 3] = ["venue", "aggregated", "derived"];

    pub const fn as_str(&self) -> &'static str {
        match self {
            ObservationBasis::Venue(_) => "venue",
            ObservationBasis::Aggregated => "aggregated",
            ObservationBasis::Derived => "derived",
        }
    }
}

/// The unit a price is expressed in, identified canonically.
///
/// Fiat currencies and crypto assets stay distinct: a stablecoin such as USDC
/// is an [`Asset`](PriceUnit::Asset), never the currency it tracks. Codes and
/// symbols (`USD`, `USDC`) are display data resolved through the identifier
/// layer, never unit identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceUnit {
    Currency(CurrencyId),
    /// An instrument used as a unit of account; expected to be a crypto asset.
    Asset(InstrumentId),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("an instrument cannot be priced in units of itself")]
pub struct SelfDenominated;

/// A single price observation of an instrument, as reported by a source.
///
/// `observed_at` is when the source says the observation happened;
/// `received_at` is when Undrly received it. No ordering between them is
/// enforced: source and Undrly clocks differ. Freshness depends on when the
/// question is asked and is computed at read time, not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketObservation {
    instrument_id: InstrumentId,
    basis: ObservationBasis,
    price: Decimal,
    unit: PriceUnit,
    source_id: SourceId,
    observed_at: Timestamp,
    received_at: Timestamp,
}

impl MarketObservation {
    pub fn new(
        instrument_id: InstrumentId,
        basis: ObservationBasis,
        price: Decimal,
        unit: PriceUnit,
        source_id: SourceId,
        observed_at: Timestamp,
        received_at: Timestamp,
    ) -> Result<Self, SelfDenominated> {
        if unit == PriceUnit::Asset(instrument_id) {
            return Err(SelfDenominated);
        }
        Ok(Self {
            instrument_id,
            basis,
            price,
            unit,
            source_id,
            observed_at,
            received_at,
        })
    }

    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    pub fn basis(&self) -> ObservationBasis {
        self.basis
    }

    pub fn venue_id(&self) -> Option<VenueId> {
        match self.basis {
            ObservationBasis::Venue(venue_id) => Some(venue_id),
            ObservationBasis::Aggregated | ObservationBasis::Derived => None,
        }
    }

    pub fn price(&self) -> Decimal {
        self.price
    }

    pub fn unit(&self) -> PriceUnit {
        self.unit
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    pub fn received_at(&self) -> Timestamp {
        self.received_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(
        basis: ObservationBasis,
        unit: PriceUnit,
        instrument_id: InstrumentId,
    ) -> Result<MarketObservation, SelfDenominated> {
        MarketObservation::new(
            instrument_id,
            basis,
            crate::decimal::parse_canonical("183.4200").unwrap(),
            unit,
            SourceId::parse("example-source").unwrap(),
            Timestamp::parse("2026-09-24T12:00:00Z").unwrap(),
            Timestamp::parse("2026-09-24T12:00:00.012Z").unwrap(),
        )
    }

    #[test]
    fn venue_basis_carries_venue() {
        let venue = VenueId::generate();
        let o = observation(
            ObservationBasis::Venue(venue),
            PriceUnit::Currency(CurrencyId::generate()),
            InstrumentId::generate(),
        )
        .unwrap();
        assert_eq!(o.venue_id(), Some(venue));
        assert_eq!(o.price().to_string(), "183.4200");
        let aggregated = observation(
            ObservationBasis::Aggregated,
            PriceUnit::Currency(CurrencyId::generate()),
            InstrumentId::generate(),
        )
        .unwrap();
        assert_eq!(aggregated.venue_id(), None);
    }

    #[test]
    fn rejects_self_denominated_prices() {
        let btc = InstrumentId::generate();
        assert_eq!(
            observation(ObservationBasis::Derived, PriceUnit::Asset(btc), btc),
            Err(SelfDenominated)
        );
        assert!(
            observation(
                ObservationBasis::Derived,
                PriceUnit::Asset(InstrumentId::generate()),
                btc
            )
            .is_ok()
        );
    }
}
