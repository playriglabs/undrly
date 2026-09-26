//! Normalization: provider-native records → validated canonical values.
//!
//! Normalization is semantic conversion, not field renaming (`AGENT.md` §13).
//! It validates every identifier with its namespace's rules and maps provider
//! vocabularies (e.g. `"common-stock"`) to canonical enums. It does no I/O and
//! does not resolve or mint canonical ids: the output describes *what the
//! source claims*, independent of what storage already contains. Anything a
//! normalizer does not understand is an error, never a guess.

use undrly_core::{
    BidAsk, CurrencyCode, Decimal, DisplayName, EntityKind, ExternalIdentifier, Figi,
    InstrumentClass, Isin, Lei, Mic, PriceType, Timestamp, Validity, VenueSymbol,
};

pub mod alpaca;
pub mod coinbase;
pub mod curated;
pub mod eia;
pub mod fixture;
pub mod fx;
pub mod gold_api;
pub mod hyperliquid;
pub mod kraken;
pub mod market_data;
pub mod sec;
pub mod worldbank;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NormalizeError {
    #[error("{field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("{field}: unsupported value `{value}`")]
    Unsupported { field: &'static str, value: String },
}

/// Provider-neutral reference claims about one listed security.
///
/// Each object carries its **primary identifier**, the key ingestion uses to
/// find an existing canonical node (LEI, ISIN, MIC, ISO 4217; a listing is
/// found by instrument + venue). Other identifiers are secondary claims that
/// are assigned, and quarantined on conflict, but never used to pick a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedReference {
    pub issuer: NormalizedEntity,
    pub instrument: NormalizedInstrument,
    pub denomination: NormalizedCurrency,
    pub listing: NormalizedListing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEntity {
    pub kind: EntityKind,
    pub name: DisplayName,
    pub lei: Lei,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedInstrument {
    pub class: InstrumentClass,
    pub name: DisplayName,
    pub isin: Isin,
    pub share_class_figi: Option<Figi>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCurrency {
    pub name: DisplayName,
    pub code: CurrencyCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedListing {
    pub venue_name: DisplayName,
    pub mic: Mic,
    pub symbol: VenueSymbol,
    pub exchange_figi: Option<Figi>,
    /// Period the source claims for the symbol. Unbounded means the source
    /// gave no bounds, not that the symbol is eternal.
    pub symbol_valid_during: Validity,
}

/// Provider-neutral claims about one entity on its own: no securities,
/// listings, or venues. For sources that are authoritative for an entity's
/// identity but not for what it issues or where that trades (e.g. SEC EDGAR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEntityRecord {
    pub kind: EntityKind,
    pub name: DisplayName,
    /// Entity identifiers the source asserts (LEI, CIK), in the source's
    /// order. Never empty; ingestion resolves the entity by these.
    pub identifiers: Vec<ExternalIdentifier>,
}

/// Normalizes one provider's reference records.
pub trait ReferenceNormalizer {
    type Record;

    fn normalize(&self, record: &Self::Record) -> Result<NormalizedReference, NormalizeError>;
}

pub(crate) fn invalid(field: &'static str, reason: impl ToString) -> NormalizeError {
    NormalizeError::Invalid {
        field,
        reason: reason.to_string(),
    }
}

/// Normalizes one provider's entity records.
pub trait EntityNormalizer {
    type Record;

    fn normalize_entity(
        &self,
        record: &Self::Record,
    ) -> Result<NormalizedEntityRecord, NormalizeError>;
}

/// One price a source reported under its own symbol. Ingestion matches it to
/// a quote feed (source, symbol, price type) to learn what it prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedQuote {
    pub symbol: VenueSymbol,
    pub price_type: PriceType,
    pub price: Decimal,
    pub bid_ask: Option<BidAsk>,
    /// The source's time for the price; `None` when the source states none.
    pub observed_at: Option<Timestamp>,
}

/// Normalizes one provider's quote payloads, for the requested `symbols`
/// only. A requested symbol absent from the payload is simply not returned;
/// ingestion reports it.
pub trait QuoteNormalizer {
    type Quote;

    fn normalize_quotes(
        &self,
        quote: &Self::Quote,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError>;
}

/// Normalizes every dated value a reference-series payload holds (V1.3
/// history), for the requested symbols; [`QuoteNormalizer`] keeps only the
/// newest of these per symbol ([`newest_per_symbol`]).
pub trait HistoryNormalizer: QuoteNormalizer {
    fn normalize_history(
        &self,
        quote: &Self::Quote,
        symbols: &[VenueSymbol],
    ) -> Result<Vec<NormalizedQuote>, NormalizeError>;
}

/// The newest point (by source time) of each symbol, in first-seen symbol
/// order. Points without a source time are not considered.
pub fn newest_per_symbol(points: Vec<NormalizedQuote>) -> Vec<NormalizedQuote> {
    let mut out: Vec<NormalizedQuote> = Vec::new();
    for p in points {
        match out.iter_mut().find(|q| q.symbol == p.symbol) {
            Some(q) if p.observed_at > q.observed_at => *q = p,
            Some(_) => {}
            None => out.push(p),
        }
    }
    out
}

/// A financial decimal from a source's text, exactly (scale preserved).
pub(crate) fn decimal(field: &'static str, text: &str) -> Result<Decimal, NormalizeError> {
    undrly_core::decimal::parse_canonical(text).map_err(|e| invalid(field, e))
}

/// A source's RFC 3339 timestamp, in UTC, truncated to microseconds (the
/// precision Undrly stores). Sources may send nanoseconds (Alpaca) or offsets.
pub(crate) fn timestamp(field: &'static str, text: &str) -> Result<Timestamp, NormalizeError> {
    let parsed = chrono::DateTime::parse_from_rfc3339(text).map_err(|e| invalid(field, e))?;
    Timestamp::from_datetime_truncating(parsed.with_timezone(&chrono::Utc))
        .map_err(|e| invalid(field, e))
}
