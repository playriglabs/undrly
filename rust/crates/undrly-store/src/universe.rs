//! Universe snapshots and their members.

use chrono::{DateTime, Utc};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};
use undrly_core::{Provenance, SourceId, UniverseKey, UniverseMember, UniverseSnapshot};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{canonical_id_from_sql, timestamp_from_sql};
use crate::sources::SourceRecordId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UniverseSnapshotId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredUniverseSnapshot {
    pub id: UniverseSnapshotId,
    pub snapshot: UniverseSnapshot,
    pub source_record: SourceRecordId,
}

/// Stores a universe snapshot and its members, asserted by `source_record`
/// (the upstream universe file). Replaying the same record is
/// [`Write::Unchanged`].
pub async fn insert_universe_snapshot(
    conn: &mut PgConnection,
    snapshot: &UniverseSnapshot,
    source_record: SourceRecordId,
) -> Result<(UniverseSnapshotId, Write), StoreError> {
    let mut tx = conn.begin().await?;
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO universe_snapshots (universe_key, as_of, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT ON CONSTRAINT universe_snapshots_replay_key DO NOTHING RETURNING id",
    )
    .bind(snapshot.key.as_str())
    .bind(snapshot.as_of.as_datetime())
    .bind(snapshot.provenance.source_id.as_str())
    .bind(snapshot.provenance.received_at.as_datetime())
    .bind(source_record.0)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(id) = inserted else {
        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM universe_snapshots WHERE universe_key = $1 AND source_record_id = $2",
        )
        .bind(snapshot.key.as_str())
        .bind(source_record.0)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok((UniverseSnapshotId(id), Write::Unchanged));
    };
    for m in &snapshot.members {
        sqlx::query(
            "INSERT INTO universe_members (snapshot_id, node_id, node_category, rank, source_symbol)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(m.node.uuid())
        .bind(m.node.category().as_str())
        .bind(m.rank.map(|r| i32::try_from(r).unwrap_or(i32::MAX)))
        .bind(m.source_symbol.as_deref())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok((UniverseSnapshotId(id), Write::Inserted))
}

/// id, as_of, source_id, received_at, source_record_id
type SnapshotRow = (i64, DateTime<Utc>, String, DateTime<Utc>, i64);

/// The latest snapshot of a universe (by as-of time, then insertion).
pub async fn latest_universe_snapshot(
    conn: &mut PgConnection,
    key: UniverseKey,
) -> Result<Option<StoredUniverseSnapshot>, StoreError> {
    let row: Option<SnapshotRow> = sqlx::query_as(
        "SELECT id, as_of, source_id, received_at, source_record_id FROM universe_snapshots
         WHERE universe_key = $1 ORDER BY as_of DESC, id DESC LIMIT 1",
    )
    .bind(key.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, as_of, source, received, record)) = row else {
        return Ok(None);
    };
    let members: Vec<(Uuid, String, Option<i32>, Option<String>)> = sqlx::query_as(
        "SELECT node_id, node_category, rank, source_symbol FROM universe_members
         WHERE snapshot_id = $1 ORDER BY rank NULLS LAST, source_symbol, node_id",
    )
    .bind(id)
    .fetch_all(conn)
    .await?;
    Ok(Some(StoredUniverseSnapshot {
        id: UniverseSnapshotId(id),
        snapshot: UniverseSnapshot {
            key,
            as_of: timestamp_from_sql(as_of)?,
            members: members
                .into_iter()
                .map(|(node, category, rank, symbol)| {
                    Ok(UniverseMember {
                        node: canonical_id_from_sql(node, &category)?,
                        rank: rank
                            .map(u32::try_from)
                            .transpose()
                            .map_err(|e| corrupt("rank", e))?,
                        source_symbol: symbol,
                    })
                })
                .collect::<Result<_, StoreError>>()?,
            provenance: Provenance {
                source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
                received_at: timestamp_from_sql(received)?,
            },
        },
        source_record: SourceRecordId(record),
    }))
}
