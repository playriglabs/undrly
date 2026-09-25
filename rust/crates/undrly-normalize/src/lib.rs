//! Normalization: provider-native records → validated canonical values.
//!
//! Normalization is semantic conversion, not field renaming (`AGENT.md` §13).
//! It validates every identifier with its namespace's rules and maps provider
//! vocabularies (e.g. `"common-stock"`) to canonical enums. It does no I/O and
//! does not resolve or mint canonical ids: the output describes *what the
//! source claims*, independent of what storage already contains. Anything a
//! normalizer does not understand is an error, never a guess.

use undrly_core::{
    CurrencyCode, DisplayName, EntityKind, Figi, InstrumentClass, Isin, Lei, Mic, Validity,
    VenueSymbol,
};

pub mod fixture;

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
