//! PostgreSQL persistence for Undrly.
//!
//! Owns the schema (`database/migrations/`, embedded as [`MIGRATOR`]), the
//! mapping between `undrly-core` types and SQL, and repositories.
//! `undrly-core` never depends on this crate, and no `sqlx` type appears in
//! core. See `docs/persistence.md`.
//!
//! Repositories are persistence primitives: explicit, typed operations per
//! table that take `&mut PgConnection`. They contain no normalization,
//! reconciliation, source-priority policy, or provider logic. Operations that
//! must be atomic open their own transaction; called inside a caller's
//! transaction they use a savepoint, so callers can compose them into larger
//! atomic units.

pub mod aliases;
pub mod conflicts;
pub mod error;
pub mod graph;
pub mod identifiers;
pub mod listing_symbols;
pub mod mapping;
pub mod market;
pub mod market_data;
pub mod reference;
pub mod sources;
#[cfg(feature = "testing")]
pub mod testing;
pub mod universe;

pub use error::StoreError;

/// Outcome of an idempotent write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    Inserted,
    /// Identical content was already stored; nothing changed.
    Unchanged,
}

/// All schema migrations, applied in order to an empty database.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../../database/migrations");
