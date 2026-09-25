//! Quarantine of conflicting identifier claims.
//!
//! A conflict row records a claim that collided with an existing mapping. It
//! is evidence for investigation, never authoritative: nothing here changes
//! canonical identity, merges nodes, or picks a winning source.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::postgres::types::PgRange;
use sqlx::types::Uuid;
use undrly_core::{
    ExternalIdentifier, IdentifierAssignment, ListingId, ListingSymbol, Provenance, SourceId,
    Timestamp, VenueId, VenueSymbol,
};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::identifiers::IdentifierAssignmentId;
use crate::listing_symbols::ListingSymbolId;
use crate::mapping::{
    canonical_id_from_sql, external_identifier_from_sql, timestamp_from_sql, validity_from_range,
    validity_to_range,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConflictId(pub i64);

/// A rejected claim and the existing mapping it collided with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictClaim {
    Identifier {
        claim: IdentifierAssignment,
        existing: IdentifierAssignmentId,
    },
    ListingSymbol {
        claim: ListingSymbol,
        existing: ListingSymbolId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConflict {
    pub id: ConflictId,
    pub claim: ConflictClaim,
    pub detected_at: Timestamp,
}

/// Records a conflict. Replaying the same claim returns the existing row.
pub async fn record_identifier_conflict(
    conn: &mut PgConnection,
    conflict: &ConflictClaim,
) -> Result<(ConflictId, Write), StoreError> {
    let (namespace, value, scope, node, category, validity, provenance, identifier, symbol) =
        match conflict {
            ConflictClaim::Identifier { claim, existing } => (
                claim.identifier().namespace().as_str(),
                claim.identifier().value().to_owned(),
                None,
                claim.node().uuid(),
                claim.node().category().as_str(),
                claim.valid_during(),
                claim.provenance(),
                Some(existing.0),
                None,
            ),
            ConflictClaim::ListingSymbol { claim, existing } => (
                "venue_symbol",
                claim.symbol.as_str().to_owned(),
                Some(claim.venue_id.uuid()),
                claim.listing_id.uuid(),
                "listing",
                claim.valid_during,
                &claim.provenance,
                None,
                Some(existing.0),
            ),
        };
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO identifier_conflicts
           (namespace, value, scope_venue_id, claimed_node_id, claimed_node_category,
            claimed_valid_during, source_id, received_at,
            conflicting_identifier_id, conflicting_listing_symbol_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT ON CONSTRAINT identifier_conflicts_replay_key DO NOTHING
         RETURNING id",
    )
    .bind(namespace)
    .bind(&value)
    .bind(scope)
    .bind(node)
    .bind(category)
    .bind(validity_to_range(validity))
    .bind(provenance.source_id.as_str())
    .bind(provenance.received_at.as_datetime())
    .bind(identifier)
    .bind(symbol)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = inserted {
        return Ok((ConflictId(id), Write::Inserted));
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM identifier_conflicts
         WHERE namespace = $1 AND value = $2 AND scope_venue_id IS NOT DISTINCT FROM $3
           AND claimed_node_id = $4 AND claimed_valid_during = $5 AND source_id = $6
           AND conflicting_identifier_id IS NOT DISTINCT FROM $7
           AND conflicting_listing_symbol_id IS NOT DISTINCT FROM $8",
    )
    .bind(namespace)
    .bind(&value)
    .bind(scope)
    .bind(node)
    .bind(validity_to_range(validity))
    .bind(provenance.source_id.as_str())
    .bind(identifier)
    .bind(symbol)
    .fetch_one(conn)
    .await?;
    Ok((ConflictId(id), Write::Unchanged))
}

type ConflictRow = (
    i64,
    String,
    String,
    Option<Uuid>,
    Uuid,
    String,
    PgRange<DateTime<Utc>>,
    String,
    DateTime<Utc>,
    Option<i64>,
    Option<i64>,
    DateTime<Utc>,
);

const SELECT: &str = "SELECT id, namespace, value, scope_venue_id, claimed_node_id,
    claimed_node_category, claimed_valid_during, source_id, received_at,
    conflicting_identifier_id, conflicting_listing_symbol_id, detected_at
    FROM identifier_conflicts";

fn conflict_from_row(row: ConflictRow) -> Result<StoredConflict, StoreError> {
    let (
        id,
        namespace,
        value,
        scope,
        node,
        category,
        validity,
        source,
        received,
        ident,
        symbol,
        detected,
    ) = row;
    let provenance = Provenance {
        source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
        received_at: timestamp_from_sql(received)?,
    };
    let valid_during = validity_from_range(validity)?;
    let claim = match (namespace.as_str(), scope, ident, symbol) {
        ("venue_symbol", Some(venue), None, Some(existing)) => ConflictClaim::ListingSymbol {
            claim: ListingSymbol {
                listing_id: ListingId::from_uuid(node).map_err(|e| corrupt("listing id", e))?,
                venue_id: VenueId::from_uuid(venue).map_err(|e| corrupt("venue id", e))?,
                symbol: VenueSymbol::new(&value).map_err(|e| corrupt("venue symbol", e))?,
                valid_during,
                provenance,
            },
            existing: ListingSymbolId(existing),
        },
        (scheme, None, Some(existing), None) => ConflictClaim::Identifier {
            claim: IdentifierAssignment::new(
                external_identifier_from_sql(scheme, &value)?,
                canonical_id_from_sql(node, &category)?,
                valid_during,
                provenance,
            )
            .map_err(|e| corrupt("identifier conflict", e))?,
            existing: IdentifierAssignmentId(existing),
        },
        _ => {
            return Err(corrupt(
                "identifier conflict",
                format!("row {id} has an invalid target"),
            ));
        }
    };
    Ok(StoredConflict {
        id: ConflictId(id),
        claim,
        detected_at: timestamp_from_sql(detected)?,
    })
}

/// Quarantined claims for a global identifier, oldest first.
pub async fn conflicts_for_identifier(
    conn: &mut PgConnection,
    identifier: &ExternalIdentifier,
) -> Result<Vec<StoredConflict>, StoreError> {
    let rows: Vec<ConflictRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE namespace = $1 AND value = $2 ORDER BY id"
    )))
    .bind(identifier.namespace().as_str())
    .bind(identifier.value())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(conflict_from_row).collect()
}

/// Quarantined claims for a venue symbol, oldest first.
pub async fn conflicts_for_listing_symbol(
    conn: &mut PgConnection,
    venue_id: VenueId,
    symbol: &VenueSymbol,
) -> Result<Vec<StoredConflict>, StoreError> {
    let rows: Vec<ConflictRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE namespace = 'venue_symbol' AND scope_venue_id = $1 AND value = $2 ORDER BY id"
    )))
    .bind(venue_id.uuid())
    .bind(symbol.as_str())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(conflict_from_row).collect()
}
