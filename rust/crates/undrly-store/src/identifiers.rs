//! Global external identifiers (ISIN, FIGI, LEI, MIC, ISO 4217) → nodes.

use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgRange;
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    CanonicalId, ExternalIdentifier, IdentifierAssignment, Provenance, SourceId, Timestamp,
};

use crate::conflicts::{ConflictClaim, ConflictId, record_identifier_conflict};
use crate::error::{StoreError, corrupt, is_exclusion_violation};
use crate::mapping::{
    canonical_id_from_sql, external_identifier_from_sql, timestamp_from_sql, validity_from_range,
    validity_to_range,
};
use crate::sources::SourceRecordId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IdentifierAssignmentId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIdentifier {
    pub id: IdentifierAssignmentId,
    pub assignment: IdentifierAssignment,
    /// The raw record whose ingestion wrote this mapping. Records that later
    /// asserted the same mapping are not recorded (corroboration is deferred).
    pub source_record: SourceRecordId,
}

/// Result of [`assign_identifier`]. Only `Assigned` writes a mapping; nothing
/// ever overwrites an existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignOutcome {
    /// The mapping was stored.
    Assigned(IdentifierAssignmentId),
    /// The same identifier → node mapping for the same period already exists
    /// (for example a replay). Its original provenance and source record are
    /// kept; the new record is not recorded as supporting evidence.
    Unchanged(IdentifierAssignmentId),
    /// The identifier is mapped to a different node for an overlapping period.
    /// The claim was quarantined in `identifier_conflicts`; canonical identity
    /// is unchanged.
    Conflict {
        quarantined: Vec<ConflictId>,
        existing: Vec<StoredIdentifier>,
    },
    /// The same node already holds this identifier for an overlapping but
    /// different period. Adjusting periods is a reconciliation decision, so
    /// nothing was written.
    OverlapsExistingPeriod { existing: Vec<StoredIdentifier> },
}

type IdentifierRow = (
    i64,
    String,
    String,
    Uuid,
    String,
    PgRange<DateTime<Utc>>,
    String,
    DateTime<Utc>,
    i64,
);

const SELECT: &str = "SELECT id, scheme, value, node_id, node_category, valid_during, source_id,
    received_at, source_record_id FROM identifiers";

fn from_row(
    (id, scheme, value, node, category, validity, source, received, record): IdentifierRow,
) -> Result<StoredIdentifier, StoreError> {
    let assignment = IdentifierAssignment::new(
        external_identifier_from_sql(&scheme, &value)?,
        canonical_id_from_sql(node, &category)?,
        validity_from_range(validity)?,
        Provenance {
            source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
            received_at: timestamp_from_sql(received)?,
        },
    )
    .map_err(|e| corrupt("identifier assignment", e))?;
    Ok(StoredIdentifier {
        id: IdentifierAssignmentId(id),
        assignment,
        source_record: SourceRecordId(record),
    })
}

async fn overlapping(
    conn: &mut PgConnection,
    claim: &IdentifierAssignment,
) -> Result<Vec<StoredIdentifier>, StoreError> {
    let rows: Vec<IdentifierRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE scheme = $1 AND value = $2 AND valid_during && $3 ORDER BY id FOR UPDATE"
    )))
    .bind(claim.identifier().namespace().as_str())
    .bind(claim.identifier().value())
    .bind(validity_to_range(claim.valid_during()))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

async fn classify(
    conn: &mut PgConnection,
    claim: &IdentifierAssignment,
    source_record: SourceRecordId,
    existing: Vec<StoredIdentifier>,
) -> Result<AssignOutcome, StoreError> {
    let other_nodes: Vec<&StoredIdentifier> = existing
        .iter()
        .filter(|e| e.assignment.node() != claim.node())
        .collect();
    if !other_nodes.is_empty() {
        let mut quarantined = Vec::with_capacity(other_nodes.len());
        for collided in other_nodes {
            let conflict = ConflictClaim::Identifier {
                claim: claim.clone(),
                existing: collided.id,
            };
            quarantined.push(
                record_identifier_conflict(conn, &conflict, source_record)
                    .await?
                    .0,
            );
        }
        return Ok(AssignOutcome::Conflict {
            quarantined,
            existing,
        });
    }
    match existing.as_slice() {
        [same] if same.assignment.valid_during() == claim.valid_during() => {
            Ok(AssignOutcome::Unchanged(same.id))
        }
        _ => Ok(AssignOutcome::OverlapsExistingPeriod { existing }),
    }
}

/// Assigns an external identifier to a node, atomically, as asserted by the
/// raw record `source_record`. The claim's source and receipt time must be the
/// record's (see [`crate::sources::RecordProvenance`]); a quarantined claim
/// also names the record.
///
/// Runs in one transaction (a savepoint when the caller already has one): the
/// mapping is inserted, or found unchanged, or the claim is quarantined and
/// [`AssignOutcome::Conflict`] returned. A concurrent writer that wins the race
/// is detected by the exclusion constraint and handled the same way.
pub async fn assign_identifier(
    conn: &mut PgConnection,
    claim: &IdentifierAssignment,
    source_record: SourceRecordId,
) -> Result<AssignOutcome, StoreError> {
    let mut tx = conn.begin().await?;
    let existing = overlapping(&mut tx, claim).await?;
    let outcome = if existing.is_empty() {
        let mut attempt = tx.begin().await?;
        let inserted: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "INSERT INTO identifiers
               (scheme, value, node_id, node_category, valid_during, source_id, received_at,
                source_record_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(claim.identifier().namespace().as_str())
        .bind(claim.identifier().value())
        .bind(claim.node().uuid())
        .bind(claim.node().category().as_str())
        .bind(validity_to_range(claim.valid_during()))
        .bind(claim.provenance().source_id.as_str())
        .bind(claim.provenance().received_at.as_datetime())
        .bind(source_record.0)
        .fetch_one(&mut *attempt)
        .await;
        match inserted {
            Ok(id) => {
                attempt.commit().await?;
                AssignOutcome::Assigned(IdentifierAssignmentId(id))
            }
            Err(err) if is_exclusion_violation(&err) => {
                attempt.rollback().await?;
                let existing = overlapping(&mut tx, claim).await?;
                classify(&mut tx, claim, source_record, existing).await?
            }
            Err(err) => return Err(err.into()),
        }
    } else {
        classify(&mut tx, claim, source_record, existing).await?
    };
    tx.commit().await?;
    Ok(outcome)
}

/// The node an identifier refers to at instant `at`, if any.
pub async fn resolve_identifier(
    conn: &mut PgConnection,
    identifier: &ExternalIdentifier,
    at: Timestamp,
) -> Result<Option<StoredIdentifier>, StoreError> {
    let row: Option<IdentifierRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE scheme = $1 AND value = $2 AND valid_during @> $3::timestamptz"
    )))
    .bind(identifier.namespace().as_str())
    .bind(identifier.value())
    .bind(at.as_datetime())
    .fetch_optional(conn)
    .await?;
    row.map(from_row).transpose()
}

/// Every stored period of an identifier, in id order.
pub async fn identifier_history(
    conn: &mut PgConnection,
    identifier: &ExternalIdentifier,
) -> Result<Vec<StoredIdentifier>, StoreError> {
    let rows: Vec<IdentifierRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE scheme = $1 AND value = $2 ORDER BY id"
    )))
    .bind(identifier.namespace().as_str())
    .bind(identifier.value())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// Every identifier assigned to a node, in id order.
pub async fn identifiers_for_node(
    conn: &mut PgConnection,
    node: CanonicalId,
) -> Result<Vec<StoredIdentifier>, StoreError> {
    let rows: Vec<IdentifierRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE node_id = $1 ORDER BY id"
    )))
    .bind(node.uuid())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}
