//! Sources and raw source records.

use sqlx::PgConnection;
use undrly_core::{DisplayName, Source, SourceId, Timestamp};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{redistribution_from_sql, redistribution_to_sql, timestamp_from_sql};

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

/// Stores a raw record. Replaying the same payload for the same key returns
/// the existing row and keeps its original `received_at`.
pub async fn insert_source_record(
    conn: &mut PgConnection,
    record: &SourceRecord,
) -> Result<(SourceRecordId, Write), StoreError> {
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
    if let Some(id) = inserted {
        return Ok((SourceRecordId(id), Write::Inserted));
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM source_records
         WHERE source_id = $1 AND record_key = $2 AND payload_sha256 = sha256($3)",
    )
    .bind(record.source_id.as_str())
    .bind(&record.record_key)
    .bind(&record.payload)
    .fetch_one(conn)
    .await?;
    Ok((SourceRecordId(id), Write::Unchanged))
}

pub async fn get_source_record(
    conn: &mut PgConnection,
    id: SourceRecordId,
) -> Result<Option<SourceRecord>, StoreError> {
    let row: Option<(String, String, Vec<u8>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
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
