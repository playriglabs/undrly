//! Repository errors.

use crate::mapping::MappingError;

/// Why a repository operation failed. Expected domain outcomes (for example an
/// identifier conflict) are not errors; they are returned as explicit results.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Mapping(#[from] MappingError),
    /// A row with this key exists with different content. Repositories never
    /// overwrite; resolving the difference is a policy decision for the caller.
    #[error("{what} `{key}` already exists with different content")]
    ExistingRecordDiffers { what: &'static str, key: String },
    #[error("{what} `{key}` not found")]
    NotFound { what: &'static str, key: String },
    #[error("listing `{listing}` is on venue `{actual}`, not `{claimed}`")]
    ListingVenueMismatch {
        listing: String,
        actual: String,
        claimed: String,
    },
    /// Stored data failed domain validation; indicates a bug or manual edit.
    #[error("stored {what} is invalid: {detail}")]
    Corrupt { what: &'static str, detail: String },
}

pub(crate) fn corrupt(what: &'static str, detail: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        what,
        detail: detail.to_string(),
    }
}

/// SQLSTATE 23P01: a row violated an exclusion constraint.
pub(crate) fn is_exclusion_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| code == "23P01")
}
