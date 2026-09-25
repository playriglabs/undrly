//! Graph edges in canonical direction. Inverse traversal is a query over the
//! same rows ([`relationships_to`]), never a stored duplicate.
//!
//! A row is one source's assertion of an edge and names the raw record that
//! first asserted it. Contradictory assertions (e.g. two different `ISSUED_BY`
//! objects for one instrument) are stored side by side, each with its record;
//! none is chosen as canonical truth until reconciliation exists.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use undrly_core::{CanonicalId, Provenance, Relationship, RelationshipType, SourceId};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{canonical_id_from_sql, timestamp_from_sql};
use crate::sources::SourceRecordId;

/// Database id of one stored edge assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelationshipAssertionId(pub i64);

/// An edge as asserted by one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRelationship {
    pub id: RelationshipAssertionId,
    pub relationship: Relationship,
    /// The raw record that first asserted this edge for this source. Later
    /// records from the same source asserting it again are not recorded.
    pub source_record: SourceRecordId,
}

/// Stores a source's current assertion of an edge, made by `source_record`
/// (whose source and receipt time must equal the relationship's provenance).
/// Replaying the same assertion from the same source is [`Write::Unchanged`]
/// and keeps the original `received_at` and record; another source's
/// assertion is a separate row.
pub async fn insert_relationship(
    conn: &mut PgConnection,
    relationship: &Relationship,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let subject = relationship.subject();
    let object = relationship.object();
    let provenance = relationship.provenance();
    // ON CONFLICT without a target also applies to the exclusion constraint.
    let inserted = sqlx::query(
        "INSERT INTO graph_edges
           (subject_id, subject_category, relationship_type, object_id, object_category,
            source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT DO NOTHING",
    )
    .bind(subject.uuid())
    .bind(subject.category().as_str())
    .bind(relationship.relationship_type().as_str())
    .bind(object.uuid())
    .bind(object.category().as_str())
    .bind(provenance.source_id.as_str())
    .bind(provenance.received_at.as_datetime())
    .bind(source_record.0)
    .execute(conn)
    .await?
    .rows_affected()
        == 1;
    Ok(if inserted {
        Write::Inserted
    } else {
        Write::Unchanged
    })
}

type EdgeRow = (
    i64,
    Uuid,
    String,
    String,
    Uuid,
    String,
    String,
    DateTime<Utc>,
    i64,
);

const SELECT: &str = "SELECT id, subject_id, subject_category, relationship_type, object_id,
    object_category, source_id, received_at, source_record_id FROM graph_edges";

fn from_row(
    (id, subject, subject_category, kind, object, object_category, source, received, record): EdgeRow,
) -> Result<StoredRelationship, StoreError> {
    let relationship = Relationship::new(
        canonical_id_from_sql(subject, &subject_category)?,
        kind.parse::<RelationshipType>()
            .map_err(|e| corrupt("relationship type", e))?,
        canonical_id_from_sql(object, &object_category)?,
        Provenance {
            source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
            received_at: timestamp_from_sql(received)?,
        },
    )
    .map_err(|e| corrupt("relationship", e))?;
    Ok(StoredRelationship {
        id: RelationshipAssertionId(id),
        relationship,
        source_record: SourceRecordId(record),
    })
}

/// Edges where `subject` is the subject (forward traversal), in id order.
pub async fn relationships_from(
    conn: &mut PgConnection,
    subject: CanonicalId,
    relationship_type: Option<RelationshipType>,
) -> Result<Vec<StoredRelationship>, StoreError> {
    let rows: Vec<EdgeRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE subject_id = $1 AND ($2::text IS NULL OR relationship_type = $2) ORDER BY id"
    )))
    .bind(subject.uuid())
    .bind(relationship_type.map(RelationshipType::as_str))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}

/// Edges where `object` is the object (inverse traversal), in id order.
pub async fn relationships_to(
    conn: &mut PgConnection,
    object: CanonicalId,
    relationship_type: Option<RelationshipType>,
) -> Result<Vec<StoredRelationship>, StoreError> {
    let rows: Vec<EdgeRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE object_id = $1 AND ($2::text IS NULL OR relationship_type = $2) ORDER BY id"
    )))
    .bind(object.uuid())
    .bind(relationship_type.map(RelationshipType::as_str))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}
