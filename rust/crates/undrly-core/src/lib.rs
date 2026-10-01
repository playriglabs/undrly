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
//! - [`onchain`]: blockchain networks (CAIP-2) and asset deployments on them (CAIP-19).
//! - [`observation`]: normalized market observations.
//! - [`quote`]: the aggregation boundary from observations to canonical quotes.

pub mod decimal;
pub mod id;
pub mod identifier;
pub mod market;
pub mod observation;
pub mod onchain;
pub mod quote;
pub mod reference;
pub mod relationship;
pub mod source;
pub mod time;
pub mod universe;

pub use id::{
    CanonicalId, Category, ChainId, CurrencyId, DeploymentId, EntityId, IdError, InstrumentId,
    ListingId, VenueId,
};
pub use identifier::{
    Cik, CurrencyCode, ExternalIdentifier, Figi, IdentifierAssignment, IdentifierError, Isin, Lei,
    ListingSymbol, Mic, Namespace,
};
pub use market::{
    Bar, BarError, BarInterval, CorporateAction, CorporateActionRole, CorporateActionType,
    EarningsReport, PerpContext, ReportTime, TradingSession,
};
pub use observation::{
    BidAsk, MarketObservation, ObservationBasis, ObservationError, PriceSubject, PriceType,
    PriceUnit,
};
pub use onchain::{
    AssetNamespace, Caip2, Chain, ChainAsset, ChainNamespace, Deployment, OnchainError,
};
pub use quote::{
    Aggregate, AggregationMethod, CrossLeg, DEFAULT_STALE_AFTER_SECONDS, FreshnessClock,
    MARK_BOOK_MAX_SKEW_SECONDS, MEAN_VENUE_MID_MAX_AGE_SECONDS, QuoteAggregation, QuoteDerivation,
    QuoteFeed, aggregate, cross_quote, cross_rate, invert_quote, invert_rate, select_latest,
};
pub use reference::{
    Alias, AliasKind, Currency, DisplayName, Entity, EntityKind, FxPair, FxPairError, Instrument,
    InstrumentClass, Listing, UnitOfMeasure, Venue, VenueSymbol,
};
pub use relationship::{Relationship, RelationshipError, RelationshipType};
pub use rust_decimal::Decimal;
pub use source::{Provenance, Redistribution, Source, SourceId};
pub use time::{Timestamp, Validity};
pub use universe::{UniverseKey, UniverseMember, UniverseSnapshot};
