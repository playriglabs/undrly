//! Reference objects: entities, instruments, venues, currencies, listings.
//!
//! Identity lives in the typed id; names are mutable display data and never
//! participate in equality of identity. Facts that connect objects (issuer,
//! venue, underlying, ...) are [`Relationship`](crate::Relationship)s, not
//! fields, so each fact carries its own provenance and is stored once.

use std::fmt;

use crate::id::{CurrencyId, EntityId, InstrumentId, ListingId, VenueId};
use crate::source::Provenance;

/// Human-readable, mutable name. Trimmed, non-empty, no control characters,
/// at most 256 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(Box<str>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("value is empty")]
    Empty,
    #[error("value has leading or trailing whitespace")]
    Untrimmed,
    #[error("value contains a control character")]
    ControlCharacter,
    #[error("value contains whitespace")]
    Whitespace,
    #[error("value exceeds {0} characters")]
    TooLong(usize),
}

impl DisplayName {
    pub fn new(s: &str) -> Result<Self, TextError> {
        if s.is_empty() {
            return Err(TextError::Empty);
        }
        if s.trim() != s {
            return Err(TextError::Untrimmed);
        }
        if s.chars().any(char::is_control) {
            return Err(TextError::ControlCharacter);
        }
        if s.chars().count() > 256 {
            return Err(TextError::TooLong(256));
        }
        Ok(Self(s.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A symbol as a specific venue spells it, e.g. `NVDA`, `BRK.B`, `7203`.
///
/// Venue-symbol rules differ from global identifier namespaces: there is no
/// normalization (no trimming, no case folding), because venues and symbol
/// conventions differ and case can be significant. Case and punctuation are
/// preserved exactly. A venue symbol is only
/// meaningful together with its venue; it is never a universal identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VenueSymbol(Box<str>);

impl VenueSymbol {
    pub fn new(s: &str) -> Result<Self, TextError> {
        if s.is_empty() {
            return Err(TextError::Empty);
        }
        if s.chars().any(char::is_control) {
            return Err(TextError::ControlCharacter);
        }
        if s.chars().any(char::is_whitespace) {
            return Err(TextError::Whitespace);
        }
        if s.chars().count() > 64 {
            return Err(TextError::TooLong(64));
        }
        Ok(Self(s.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VenueSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What an [`Alias`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AliasKind {
    /// A symbol or ticker-like code (`BTC`, `XAU`, `BTC-PERP`).
    Symbol,
    /// A name (`Bitcoin`, `Gold`).
    Name,
}

impl AliasKind {
    pub const ALL: [AliasKind; 2] = [AliasKind::Symbol, AliasKind::Name];

    pub const fn as_str(self) -> &'static str {
        match self {
            AliasKind::Symbol => "symbol",
            AliasKind::Name => "name",
        }
    }
}

/// A search term for a node, asserted by a source. For discovery only: an
/// alias never selects a node for identity resolution (unlike identifiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub node: crate::id::CanonicalId,
    pub text: DisplayName,
    pub kind: AliasKind,
    pub provenance: Provenance,
}

/// Kind of entity. Extended as new kinds are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Company,
}

impl EntityKind {
    pub const ALL: [EntityKind; 1] = [EntityKind::Company];

    pub const fn as_str(self) -> &'static str {
        match self {
            EntityKind::Company => "company",
        }
    }
}

/// Asset class of an instrument. An attribute, not identity: correcting a
/// classification never changes the instrument's canonical id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentClass {
    Equity,
    /// A crypto asset, including stablecoins. Never a fiat currency.
    CryptoAsset,
    /// A physical commodity in a defined unit (e.g. gold, one troy ounce).
    Commodity,
    /// A perpetual futures contract. Its underlying is a `DERIVES_FROM`
    /// relationship, its settlement asset a `SETTLES_IN` relationship.
    PerpetualFuture,
    /// A spot FX market between two fiat currencies ([`FxPair`]): its price
    /// is units of the quote currency per one unit of the base currency.
    /// The currencies stay currency nodes; only the market is an instrument.
    Fx,
}

impl InstrumentClass {
    pub const ALL: [InstrumentClass; 5] = [
        InstrumentClass::Equity,
        InstrumentClass::CryptoAsset,
        InstrumentClass::Commodity,
        InstrumentClass::PerpetualFuture,
        InstrumentClass::Fx,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            InstrumentClass::Equity => "equity",
            InstrumentClass::CryptoAsset => "crypto_asset",
            InstrumentClass::Commodity => "commodity",
            InstrumentClass::PerpetualFuture => "perpetual_future",
            InstrumentClass::Fx => "fx",
        }
    }
}

/// An economic or legal entity, such as a company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub name: DisplayName,
}

/// A financial instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    pub id: InstrumentId,
    pub class: InstrumentClass,
    pub name: DisplayName,
    /// Units of the underlying per contract, when not 1 (e.g. Hyperliquid's
    /// `kPEPE` is 1,000 PEPE). Prices are per contract.
    pub contract_multiplier: Option<crate::Decimal>,
    /// The quantity one unit of a commodity denotes (prices are per unit).
    pub unit_of_measure: Option<UnitOfMeasure>,
    /// Base and quote currency of an FX market; present exactly when the
    /// class is [`InstrumentClass::Fx`].
    pub fx_pair: Option<FxPair>,
}

/// The two currencies of a spot FX market, in canonical orientation:
/// `EUR/USD` is base EUR, quote USD, and prices 1 EUR in USD. The inverse
/// orientation (`USD/EUR`) is a different market, never a relabelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FxPair {
    pub base: CurrencyId,
    pub quote: CurrencyId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FxPairError {
    #[error("an FX pair needs two different currencies")]
    SameCurrency,
    #[error("an FX instrument states its base and quote currency; no other class does")]
    ClassMismatch,
}

impl FxPair {
    pub fn new(base: CurrencyId, quote: CurrencyId) -> Result<Self, FxPairError> {
        if base == quote {
            return Err(FxPairError::SameCurrency);
        }
        Ok(Self { base, quote })
    }
}

impl Instrument {
    /// Checks that `fx_pair` is present exactly for FX instruments.
    pub fn validate(&self) -> Result<(), FxPairError> {
        match (self.class, self.fx_pair) {
            (InstrumentClass::Fx, Some(p)) if p.base == p.quote => Err(FxPairError::SameCurrency),
            (InstrumentClass::Fx, Some(_)) => Ok(()),
            (InstrumentClass::Fx, None) | (_, Some(_)) => Err(FxPairError::ClassMismatch),
            (_, None) => Ok(()),
        }
    }
}

/// Physical unit a commodity instrument is quoted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitOfMeasure {
    TroyOunce,
    Barrel,
    Mmbtu,
    MetricTon,
    Kilogram,
}

impl UnitOfMeasure {
    pub const ALL: [UnitOfMeasure; 5] = [
        UnitOfMeasure::TroyOunce,
        UnitOfMeasure::Barrel,
        UnitOfMeasure::Mmbtu,
        UnitOfMeasure::MetricTon,
        UnitOfMeasure::Kilogram,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            UnitOfMeasure::TroyOunce => "troy_ounce",
            UnitOfMeasure::Barrel => "barrel",
            UnitOfMeasure::Mmbtu => "mmbtu",
            UnitOfMeasure::MetricTon => "metric_ton",
            UnitOfMeasure::Kilogram => "kilogram",
        }
    }
}

/// A trading venue. Its MIC, if any, is an external identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Venue {
    pub id: VenueId,
    pub name: DisplayName,
}

/// A fiat currency node. Its ISO 4217 code is an external identifier, not
/// identity (codes are reassigned on redenomination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Currency {
    pub id: CurrencyId,
    pub name: DisplayName,
}

/// An instrument's listing on a venue, e.g. NVIDIA common stock on Nasdaq.
///
/// The listing has its own identity so it survives symbol changes; its
/// venue symbols are [`ListingSymbol`](crate::ListingSymbol)s with validity
/// periods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub id: ListingId,
    pub instrument_id: InstrumentId,
    pub venue_id: VenueId,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_validation() {
        assert_eq!(
            DisplayName::new("NVIDIA Corporation").unwrap().as_str(),
            "NVIDIA Corporation"
        );
        assert_eq!(DisplayName::new(""), Err(TextError::Empty));
        assert_eq!(DisplayName::new(" NVIDIA"), Err(TextError::Untrimmed));
        assert_eq!(DisplayName::new("NVIDIA\n"), Err(TextError::Untrimmed));
        assert_eq!(
            DisplayName::new("NV\u{0}DIA"),
            Err(TextError::ControlCharacter)
        );
        assert_eq!(
            DisplayName::new(&"a".repeat(257)),
            Err(TextError::TooLong(256))
        );
    }

    #[test]
    fn venue_symbol_preserves_spelling() {
        for s in ["NVDA", "BRK.B", "BRK/B", "7203", "nvda"] {
            assert_eq!(VenueSymbol::new(s).unwrap().as_str(), s);
        }
        assert_ne!(
            VenueSymbol::new("NVDA").unwrap(),
            VenueSymbol::new("nvda").unwrap()
        );
        assert_eq!(VenueSymbol::new("NV DA"), Err(TextError::Whitespace));
        assert_eq!(VenueSymbol::new(""), Err(TextError::Empty));
    }
}
