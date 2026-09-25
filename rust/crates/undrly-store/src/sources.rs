//! Sources and raw source records.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use undrly_core::{CanonicalId, DisplayName, Provenance, Source, SourceId, Timestamp};

use crate::Write;
use crate::conflicts::ConflictId;
use crate::error::{StoreError, corrupt};
use crate::graph::RelationshipAssertionId;
use crate::identifiers::IdentifierAssignmentId;
use crate::listing_symbols::ListingSymbolId;
use crate::mapping::{
    canonical_id_from_sql, redistribution_from_sql, redistribution_to_sql, timestamp_from_sql,
};

/// Registers a source. Registration is administrative (it carries
/// redistribution terms); ingestion never creates sources implicitly.
pub async fn insert_source(conn: &mut PgConnection, source: &Source) -> Result<Write, StoreError> {
    let inserted = sqlx::query(
        "INSERT INTO sources (id, name, redistribution) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(source.id.as_str())
    .bind(source.name.as_str())
    .bind(redistribution_to_sql(source.redistribution))
    .execute(&mut *conn)
    .await?
    .rows_affected()
        == 1;
    if inserted {
        return Ok(Write::Inserted);
    }
    match get_source(conn, &source.id).await? {
        Some(existing) if existing == *source => Ok(Write::Unchanged),
        _ => Err(StoreError::ExistingRecordDiffers {
            what: "source",
            key: source.id.to_string(),
        }),
    }
}

pub async fn get_source(
    conn: &mut PgConnection,
    id: &SourceId,
) -> Result<Option<Source>, StoreError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT name, redistribution FROM sources WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(conn)
            .await?;
    row.map(|(name, redistribution)| {
        Ok(Source {
            id: id.clone(),
            name: DisplayName::new(&name).map_err(|e| corrupt("source name", e))?,
            redistribution: redistribution_from_sql(&redistribution)?,
        })
    })
    .transpose()
}

/// Database id of a stored raw source record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRecordId(pub i64);

/// A raw payload exactly as received from a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub source_id: SourceId,
    /// The source's own key for the record (e.g. a record id or file name).
    pub record_key: String,
    pub payload: Vec<u8>,
    pub received_at: Timestamp,
}

/// A stored raw record as the provenance of the facts derived from it.
///
/// Repositories that write source-derived facts take the record's id; a
/// fact's `source_id` and `received_at` must equal `provenance` (enforced by
/// composite foreign keys), so build facts from this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordProvenance {
    pub id: SourceRecordId,
    /// The record's source and its stored receipt time.
    pub provenance: Provenance,
}

/// Stores a raw record. Replaying the same payload for the same key returns
/// the existing row and keeps its original `received_at`.
pub async fn insert_source_record(
    conn: &mut PgConnection,
    record: &SourceRecord,
) -> Result<(RecordProvenance, Write), StoreError> {
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO source_records (source_id, record_key, payload, received_at)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT ON CONSTRAINT source_records_replay_key DO NOTHING
         RETURNING id",
    )
    .bind(record.source_id.as_str())
    .bind(&record.record_key)
    .bind(&record.payload)
    .bind(record.received_at.as_datetime())
    .fetch_optional(&mut *conn)
    .await?;
    let provenance = |id, received_at| RecordProvenance {
        id: SourceRecordId(id),
        provenance: Provenance {
            source_id: record.source_id.clone(),
            received_at,
        },
    };
    if let Some(id) = inserted {
        return Ok((provenance(id, record.received_at), Write::Inserted));
    }
    let (id, received_at): (i64, DateTime<Utc>) = sqlx::query_as(
        "SELECT id, received_at FROM source_records
         WHERE source_id = $1 AND record_key = $2 AND payload_sha256 = sha256($3)",
    )
    .bind(record.source_id.as_str())
    .bind(&record.record_key)
    .bind(&record.payload)
    .fetch_one(conn)
    .await?;
    Ok((
        provenance(id, timestamp_from_sql(received_at)?),
        Write::Unchanged,
    ))
}

pub async fn get_source_record(
    conn: &mut PgConnection,
    id: SourceRecordId,
) -> Result<Option<SourceRecord>, StoreError> {
    let row: Option<(String, String, Vec<u8>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT source_id, record_key, payload, received_at FROM source_records WHERE id = $1",
    )
    .bind(id.0)
    .fetch_optional(conn)
    .await?;
    row.map(|(source_id, record_key, payload, received_at)| {
        Ok(SourceRecord {
            source_id: SourceId::parse(&source_id).map_err(|e| corrupt("source id", e))?,
            record_key,
            payload,
            received_at: timestamp_from_sql(received_at)?,
        })
    })
    .transpose()
}

/// Every fact whose originating record is `id`, each list in id order.
///
/// Only the originating record is stored: a fact first written from another
/// record, and later asserted again by this one, is not listed here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivedFacts {
    /// Nodes minted from the record (their object rows), listings included.
    pub nodes: Vec<CanonicalId>,
    pub identifiers: Vec<IdentifierAssignmentId>,
    pub listing_symbols: Vec<ListingSymbolId>,
    pub relationships: Vec<RelationshipAssertionId>,
    pub conflicts: Vec<ConflictId>,
}

impl DerivedFacts {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The facts a raw record asserted: fact → source record, inverted.
pub async fn facts_from_source_record(
    conn: &mut PgConnection,
    id: SourceRecordId,
) -> Result<DerivedFacts, StoreError> {
    let nodes: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, category FROM (
           SELECT id, category FROM entities WHERE source_record_id = $1
           UNION ALL SELECT id, category FROM instruments WHERE source_record_id = $1
           UNION ALL SELECT id, category FROM venues WHERE source_record_id = $1
           UNION ALL SELECT id, category FROM currencies WHERE source_record_id = $1
           UNION ALL SELECT id, category FROM listings WHERE source_record_id = $1
         ) objects ORDER BY id",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let ids = |table: &'static str| {
        sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT id FROM {table} WHERE source_record_id = $1 ORDER BY id"
        )))
        .bind(id.0)
    };
    Ok(DerivedFacts {
        nodes: nodes
            .into_iter()
            .map(|(uuid, category)| canonical_id_from_sql(uuid, &category))
            .collect::<Result<_, _>>()?,
        identifiers: ids("identifiers")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(IdentifierAssignmentId)
            .collect(),
        listing_symbols: ids("listing_symbols")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(ListingSymbolId)
            .collect(),
        relationships: ids("graph_edges")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(RelationshipAssertionId)
            .collect(),
        conflicts: ids("identifier_conflicts")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(ConflictId)
            .collect(),
    })
}
