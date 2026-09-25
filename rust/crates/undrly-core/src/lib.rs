//! Canonical financial domain for Undrly.
//!
//! Canonical semantics originate here. This crate has no I/O and no database
//! dependency; persistence lives in `undrly-store`, which maps these types to
//! PostgreSQL.
//!
//! Module map:
//! - [`id`]: generated canonical identifiers (`undrly:<category>:<uuidv7>`).
//! - [`identifier`]: external identifiers (ISIN, FIGI, LEI, MIC, ISO 4217),
//!   each with namespace-specific rules, and their assignments to nodes.
//! - [`decimal`]: exact decimal rules.
//! - [`time`]: microsecond-precision UTC [`Timestamp`] and [`Validity`] periods.
//! - [`source`]: data sources, redistribution terms, provenance.
//! - [`reference`]: entities, instruments, venues, currencies, listings.
//! - [`relationship`]: directed relationships in canonical direction.
//! - [`observation`]: normalized market observations.

pub mod decimal;
pub mod id;
pub mod identifier;
pub mod observation;
pub mod reference;
pub mod relationship;
pub mod source;
pub mod time;

pub use id::{
    CanonicalId, Category, CurrencyId, EntityId, IdError, InstrumentId, ListingId, VenueId,
};
pub use identifier::{
    CurrencyCode, ExternalIdentifier, Figi, IdentifierAssignment, IdentifierError, Isin, Lei,
    ListingSymbol, Mic, Namespace,
};
pub use observation::{MarketObservation, ObservationBasis, PriceUnit};
pub use reference::{
    Currency, DisplayName, Entity, EntityKind, Instrument, InstrumentClass, Listing, Venue,
    VenueSymbol,
};
pub use relationship::{Relationship, RelationshipError, RelationshipType};
pub use rust_decimal::Decimal;
pub use source::{Provenance, Redistribution, Source, SourceId};
pub use time::{Timestamp, Validity};
