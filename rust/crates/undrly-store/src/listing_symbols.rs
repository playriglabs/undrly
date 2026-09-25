//! Venue-scoped listing symbols. A symbol identifies a listing only together
//! with its venue and only for a period; it is never global identity.

use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgRange;
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    ListingId, ListingSymbol, Provenance, SourceId, Timestamp, VenueId, VenueSymbol,
};

use crate::conflicts::{ConflictClaim, ConflictId, record_identifier_conflict};
use crate::error::{StoreError, corrupt, is_exclusion_violation};
use crate::mapping::{timestamp_from_sql, validity_from_range, validity_to_range};
use crate::reference::get_listing;
use crate::sources::SourceRecordId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListingSymbolId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredListingSymbol {
    pub id: ListingSymbolId,
    pub symbol: ListingSymbol,
    /// The raw record whose ingestion wrote this mapping (see
    /// [`crate::identifiers::StoredIdentifier::source_record`]).
    pub source_record: SourceRecordId,
}

/// Result of [`assign_listing_symbol`]. Only `Assigned` writes a mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolAssignOutcome {
    Assigned(ListingSymbolId),
    /// Same listing, venue, symbol and period already stored.
    Unchanged(ListingSymbolId),
    /// The symbol belongs to a different listing on this venue for an
    /// overlapping period. The claim was quarantined.
    Conflict {
        quarantined: Vec<ConflictId>,
        existing: Vec<StoredListingSymbol>,
    },
    /// The listing already has a different symbol for an overlapping period.
    /// Not quarantined: `identifier_conflicts` references a collision on the
    /// same symbol value, which this is not. Nothing was written.
    ListingHasOtherSymbol {
        existing: Vec<StoredListingSymbol>,
    },
    /// The same listing already holds this symbol for an overlapping but
    /// different period. Nothing was written.
    OverlapsExistingPeriod {
        existing: Vec<StoredListingSymbol>,
    },
}

type SymbolRow = (
    i64,
    Uuid,
    Uuid,
    String,
    PgRange<DateTime<Utc>>,
    String,
    DateTime<Utc>,
    i64,
);

const SELECT: &str =
    "SELECT id, listing_id, venue_id, symbol, valid_during, source_id, received_at,
    source_record_id FROM listing_symbols";

fn from_row(
    (id, listing, venue, symbol, validity, source, received, record): SymbolRow,
) -> Result<StoredListingSymbol, StoreError> {
    Ok(StoredListingSymbol {
        id: ListingSymbolId(id),
        symbol: ListingSymbol {
            listing_id: ListingId::from_uuid(listing).map_err(|e| corrupt("listing id", e))?,
            venue_id: VenueId::from_uuid(venue).map_err(|e| corrupt("venue id", e))?,
            symbol: VenueSymbol::new(&symbol).map_err(|e| corrupt("venue symbol", e))?,
            valid_during: validity_from_range(validity)?,
            provenance: Provenance {
                source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
                received_at: timestamp_from_sql(received)?,
            },
        },
        source_record: SourceRecordId(record),
    })
}

/// Rows for the claimed symbol on the claimed venue, overlapping the period.
async fn overlapping_symbol(
    conn: &mut PgConnection,
    claim: &ListingSymbol,
) -> Result<Vec<StoredListingSymbol>, StoreError> {
    let rows: Vec<SymbolRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE venue_id = $1 AND symbol = $2 AND valid_during && $3 ORDER BY id FOR UPDATE"
    )))
    .bind(claim.venue_id.uuid())
    .bind(claim.symbol.as_str())
    .bind(validity_to_range(claim.valid_during))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// Rows for the claimed listing, overlapping the period.
async fn overlapping_listing(
    conn: &mut PgConnection,
    claim: &ListingSymbol,
) -> Result<Vec<StoredListingSymbol>, StoreError> {
    let rows: Vec<SymbolRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE listing_id = $1 AND valid_during && $2 ORDER BY id FOR UPDATE"
    )))
    .bind(claim.listing_id.uuid())
    .bind(validity_to_range(claim.valid_during))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

async fn classify(
    conn: &mut PgConnection,
    claim: &ListingSymbol,
    source_record: SourceRecordId,
) -> Result<Option<SymbolAssignOutcome>, StoreError> {
    let same_symbol = overlapping_symbol(conn, claim).await?;
    let other_listings: Vec<&StoredListingSymbol> = same_symbol
        .iter()
        .filter(|s| s.symbol.listing_id != claim.listing_id)
        .collect();
    if !other_listings.is_empty() {
        let mut quarantined = Vec::with_capacity(other_listings.len());
        for collided in other_listings {
            let conflict = ConflictClaim::ListingSymbol {
                claim: claim.clone(),
                existing: collided.id,
            };
            quarantined.push(
                record_identifier_conflict(conn, &conflict, source_record)
                    .await?
                    .0,
            );
        }
        return Ok(Some(SymbolAssignOutcome::Conflict {
            quarantined,
            existing: same_symbol,
        }));
    }
    let same_listing = overlapping_listing(conn, claim).await?;
    let other_symbols: Vec<StoredListingSymbol> = same_listing
        .iter()
        .filter(|s| s.symbol.symbol != claim.symbol)
        .cloned()
        .collect();
    if !other_symbols.is_empty() {
        return Ok(Some(SymbolAssignOutcome::ListingHasOtherSymbol {
            existing: other_symbols,
        }));
    }
    Ok(match same_symbol.as_slice() {
        [] => None,
        [same] if same.symbol.valid_during == claim.valid_during => {
            Some(SymbolAssignOutcome::Unchanged(same.id))
        }
        _ => Some(SymbolAssignOutcome::OverlapsExistingPeriod {
            existing: same_symbol,
        }),
    })
}

/// Assigns a venue symbol to a listing, atomically, as asserted by
/// `source_record` (see [`crate::identifiers::assign_identifier`]). The
/// symbol's venue must be the listing's venue.
pub async fn assign_listing_symbol(
    conn: &mut PgConnection,
    claim: &ListingSymbol,
    source_record: SourceRecordId,
) -> Result<SymbolAssignOutcome, StoreError> {
    let mut tx = conn.begin().await?;
    let listing = get_listing(&mut tx, claim.listing_id)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            what: "listing",
            key: claim.listing_id.to_string(),
        })?;
    if listing.venue_id != claim.venue_id {
        return Err(StoreError::ListingVenueMismatch {
            listing: claim.listing_id.to_string(),
            actual: listing.venue_id.to_string(),
            claimed: claim.venue_id.to_string(),
        });
    }
    let outcome = match classify(&mut tx, claim, source_record).await? {
        Some(outcome) => outcome,
        None => {
            let mut attempt = tx.begin().await?;
            let inserted: Result<i64, sqlx::Error> = sqlx::query_scalar(
                "INSERT INTO listing_symbols
                   (listing_id, venue_id, symbol, valid_during, source_id, received_at,
                    source_record_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
            )
            .bind(claim.listing_id.uuid())
            .bind(claim.venue_id.uuid())
            .bind(claim.symbol.as_str())
            .bind(validity_to_range(claim.valid_during))
            .bind(claim.provenance.source_id.as_str())
            .bind(claim.provenance.received_at.as_datetime())
            .bind(source_record.0)
            .fetch_one(&mut *attempt)
            .await;
            match inserted {
                Ok(id) => {
                    attempt.commit().await?;
                    SymbolAssignOutcome::Assigned(ListingSymbolId(id))
                }
                Err(err) if is_exclusion_violation(&err) => {
                    attempt.rollback().await?;
                    classify(&mut tx, claim, source_record)
                        .await?
                        .ok_or_else(|| StoreError::Database(err))?
                }
                Err(err) => return Err(err.into()),
            }
        }
    };
    tx.commit().await?;
    Ok(outcome)
}

/// The listing a venue symbol refers to on `venue_id` at instant `at`.
pub async fn resolve_listing_symbol(
    conn: &mut PgConnection,
    venue_id: VenueId,
    symbol: &VenueSymbol,
    at: Timestamp,
) -> Result<Option<StoredListingSymbol>, StoreError> {
    let row: Option<SymbolRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE venue_id = $1 AND symbol = $2 AND valid_during @> $3::timestamptz"
    )))
    .bind(venue_id.uuid())
    .bind(symbol.as_str())
    .bind(at.as_datetime())
    .fetch_optional(conn)
    .await?;
    row.map(from_row).transpose()
}

/// Every listing using `symbol` at `at`, across all venues. A bare symbol is
/// ambiguous by design: callers must choose a venue, never assume one.
pub async fn listings_with_symbol(
    conn: &mut PgConnection,
    symbol: &VenueSymbol,
    at: Timestamp,
) -> Result<Vec<StoredListingSymbol>, StoreError> {
    let rows: Vec<SymbolRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE symbol = $1 AND valid_during @> $2::timestamptz ORDER BY id"
    )))
    .bind(symbol.as_str())
    .bind(at.as_datetime())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// Every symbol period of a listing, in id order.
pub async fn symbols_for_listing(
    conn: &mut PgConnection,
    listing_id: ListingId,
) -> Result<Vec<StoredListingSymbol>, StoreError> {
    let rows: Vec<SymbolRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE listing_id = $1 ORDER BY id"
    )))
    .bind(listing_id.uuid())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}
