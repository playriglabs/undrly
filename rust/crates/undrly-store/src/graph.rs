//! Graph edges in canonical direction. Inverse traversal is a query over the
//! same rows ([`relationships_to`]), never a stored duplicate.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use undrly_core::{CanonicalId, Provenance, Relationship, RelationshipType, SourceId};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{canonical_id_from_sql, timestamp_from_sql};

/// Stores a source's current assertion of an edge. Replaying the same
/// assertion from the same source is [`Write::Unchanged`] and keeps the
/// original `received_at`; another source's assertion is a separate row.
pub async fn insert_relationship(
    conn: &mut PgConnection,
    relationship: &Relationship,
) -> Result<Write, StoreError> {
    let subject = relationship.subject();
    let object = relationship.object();
    let provenance = relationship.provenance();
    // ON CONFLICT without a target also applies to the exclusion constraint.
    let inserted = sqlx::query(
        "INSERT INTO graph_edges
           (subject_id, subject_category, relationship_type, object_id, object_category,
            source_id, received_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT DO NOTHING",
    )
    .bind(subject.uuid())
    .bind(subject.category().as_str())
    .bind(relationship.relationship_type().as_str())
    .bind(object.uuid())
    .bind(object.category().as_str())
    .bind(provenance.source_id.as_str())
    .bind(provenance.received_at.as_datetime())
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

type EdgeRow = (Uuid, String, String, Uuid, String, String, DateTime<Utc>);

const SELECT: &str = "SELECT subject_id, subject_category, relationship_type, object_id,
    object_category, source_id, received_at FROM graph_edges";

fn from_row(
    (subject, subject_category, kind, object, object_category, source, received): EdgeRow,
) -> Result<Relationship, StoreError> {
    Relationship::new(
        canonical_id_from_sql(subject, &subject_category)?,
        kind.parse::<RelationshipType>()
            .map_err(|e| corrupt("relationship type", e))?,
        canonical_id_from_sql(object, &object_category)?,
        Provenance {
            source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
            received_at: timestamp_from_sql(received)?,
        },
    )
    .map_err(|e| corrupt("relationship", e))
}

/// Edges where `subject` is the subject (forward traversal), in id order.
pub async fn relationships_from(
    conn: &mut PgConnection,
    subject: CanonicalId,
    relationship_type: Option<RelationshipType>,
) -> Result<Vec<Relationship>, StoreError> {
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
) -> Result<Vec<Relationship>, StoreError> {
    let rows: Vec<EdgeRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE object_id = $1 AND ($2::text IS NULL OR relationship_type = $2) ORDER BY id"
    )))
    .bind(object.uuid())
    .bind(relationship_type.map(RelationshipType::as_str))
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(from_row).collect()
}
