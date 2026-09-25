//! Normalized market observations: one source's price for one subject.

use rust_decimal::Decimal;

use crate::id::{CanonicalId, CurrencyId, InstrumentId, VenueId};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PriceUnit {
    Currency(CurrencyId),
    /// An instrument used as a unit of account; expected to be a crypto asset.
    Asset(InstrumentId),
}

impl PriceUnit {
    pub fn canonical(self) -> CanonicalId {
        match self {
            PriceUnit::Currency(id) => id.into(),
            PriceUnit::Asset(id) => id.into(),
        }
    }
}

/// What is being priced: one unit of an instrument (a share, one BTC, one
/// troy ounce of gold, one perpetual contract) or of a fiat currency (FX:
/// the price of 1 EUR in USD). A "pair" is a subject priced in a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PriceSubject {
    Instrument(InstrumentId),
    Currency(CurrencyId),
}

impl PriceSubject {
    pub fn canonical(self) -> CanonicalId {
        match self {
            PriceSubject::Instrument(id) => id.into(),
            PriceSubject::Currency(id) => id.into(),
        }
    }
}

/// Which price a source reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PriceType {
    /// Last trade.
    Last,
    /// Midpoint of the best bid and ask.
    Mid,
    /// A derivatives venue's mark price (used for margining).
    Mark,
    /// A published reference or benchmark price, not a trade.
    Reference,
    /// A published average over a period (e.g. a monthly average), neither
    /// spot nor futures.
    Average,
}

impl PriceType {
    pub const ALL: [PriceType; 5] = [
        PriceType::Last,
        PriceType::Mid,
        PriceType::Mark,
        PriceType::Reference,
        PriceType::Average,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            PriceType::Last => "last",
            PriceType::Mid => "mid",
            PriceType::Mark => "mark",
            PriceType::Reference => "reference",
            PriceType::Average => "average",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObservationError {
    #[error("a subject cannot be priced in units of itself")]
    SelfDenominated,
    #[error("bid {bid} is above ask {ask}")]
    CrossedQuote { bid: Decimal, ask: Decimal },
}

/// Best bid and ask, when the source reports both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidAsk {
    pub bid: Decimal,
    pub ask: Decimal,
}

/// A single price observation, as reported by one source.
///
/// `observed_at` is when the source says the price was current. Some sources
/// state no time at all (e.g. a ticker snapshot); then it is `None`, never
/// filled in with Undrly's clock. `received_at` is when Undrly received the
/// response. Freshness depends on when the question is asked and is computed
/// at read time, not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketObservation {
    subject: PriceSubject,
    basis: ObservationBasis,
    price_type: PriceType,
    price: Decimal,
    bid_ask: Option<BidAsk>,
    unit: PriceUnit,
    source_id: SourceId,
    observed_at: Option<Timestamp>,
    received_at: Timestamp,
}

impl MarketObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject: PriceSubject,
        basis: ObservationBasis,
        price_type: PriceType,
        price: Decimal,
        bid_ask: Option<BidAsk>,
        unit: PriceUnit,
        source_id: SourceId,
        observed_at: Option<Timestamp>,
        received_at: Timestamp,
    ) -> Result<Self, ObservationError> {
        if subject.canonical() == unit.canonical() {
            return Err(ObservationError::SelfDenominated);
        }
        if let Some(BidAsk { bid, ask }) = bid_ask
            && bid > ask
        {
            return Err(ObservationError::CrossedQuote { bid, ask });
        }
        Ok(Self {
            subject,
            basis,
            price_type,
            price,
            bid_ask,
            unit,
            source_id,
            observed_at,
            received_at,
        })
    }

    pub fn subject(&self) -> PriceSubject {
        self.subject
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

    pub fn price_type(&self) -> PriceType {
        self.price_type
    }

    pub fn price(&self) -> Decimal {
        self.price
    }

    pub fn bid_ask(&self) -> Option<BidAsk> {
        self.bid_ask
    }

    pub fn unit(&self) -> PriceUnit {
        self.unit
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn observed_at(&self) -> Option<Timestamp> {
        self.observed_at
    }

    pub fn received_at(&self) -> Timestamp {
        self.received_at
    }

    /// The time the price is known to be current as of: the source's time
    /// when it states one, otherwise when Undrly received it.
    pub fn effective_at(&self) -> Timestamp {
        self.observed_at.unwrap_or(self.received_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::parse_canonical;

    fn observation(
        subject: PriceSubject,
        basis: ObservationBasis,
        unit: PriceUnit,
        bid_ask: Option<BidAsk>,
    ) -> Result<MarketObservation, ObservationError> {
        MarketObservation::new(
            subject,
            basis,
            PriceType::Last,
            parse_canonical("183.4200").unwrap(),
            bid_ask,
            unit,
            SourceId::parse("example-source").unwrap(),
            Some(Timestamp::parse("2026-09-24T12:00:00Z").unwrap()),
            Timestamp::parse("2026-09-24T12:00:00.012Z").unwrap(),
        )
    }

    fn usd() -> PriceUnit {
        PriceUnit::Currency(CurrencyId::generate())
    }

    #[test]
    fn venue_basis_carries_venue() {
        let venue = VenueId::generate();
        let subject = PriceSubject::Instrument(InstrumentId::generate());
        let o = observation(subject, ObservationBasis::Venue(venue), usd(), None).unwrap();
        assert_eq!(o.venue_id(), Some(venue));
        assert_eq!(o.price().to_string(), "183.4200");
        let aggregated = observation(subject, ObservationBasis::Aggregated, usd(), None).unwrap();
        assert_eq!(aggregated.venue_id(), None);
    }

    #[test]
    fn rejects_self_denominated_prices() {
        let btc = InstrumentId::generate();
        assert_eq!(
            observation(
                PriceSubject::Instrument(btc),
                ObservationBasis::Derived,
                PriceUnit::Asset(btc),
                None
            ),
            Err(ObservationError::SelfDenominated)
        );
        let eur = CurrencyId::generate();
        assert_eq!(
            observation(
                PriceSubject::Currency(eur),
                ObservationBasis::Derived,
                PriceUnit::Currency(eur),
                None
            ),
            Err(ObservationError::SelfDenominated)
        );
        // FX: a currency priced in another currency.
        assert!(
            observation(
                PriceSubject::Currency(eur),
                ObservationBasis::Aggregated,
                usd(),
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_crossed_bid_ask_and_uses_source_time_when_stated() {
        let subject = PriceSubject::Instrument(InstrumentId::generate());
        let d = |s| parse_canonical(s).unwrap();
        assert!(matches!(
            observation(
                subject,
                ObservationBasis::Aggregated,
                usd(),
                Some(BidAsk {
                    bid: d("2.0"),
                    ask: d("1.0")
                })
            ),
            Err(ObservationError::CrossedQuote { .. })
        ));
        let o = observation(
            subject,
            ObservationBasis::Aggregated,
            usd(),
            Some(BidAsk {
                bid: d("1.0"),
                ask: d("1.0"),
            }),
        )
        .unwrap();
        assert_eq!(o.effective_at(), o.observed_at().unwrap());
    }
}
