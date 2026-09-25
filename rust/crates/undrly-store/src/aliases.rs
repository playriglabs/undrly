//! Search aliases. Discovery only: an alias never resolves identity.

use sqlx::PgConnection;
use undrly_core::{Alias, CanonicalId};

use crate::Write;
use crate::error::StoreError;
use crate::sources::SourceRecordId;

/// Stores an alias asserted by `source_record` (whose source and receipt time
/// must equal the alias's provenance). The same alias (case-insensitive) for
/// the same node, kind and source is [`Write::Unchanged`].
pub async fn insert_alias(
    conn: &mut PgConnection,
    alias: &Alias,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let inserted = sqlx::query(
        "INSERT INTO aliases
           (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT ON CONSTRAINT aliases_one_per_source DO NOTHING",
    )
    .bind(alias.node.uuid())
    .bind(alias.node.category().as_str())
    .bind(alias.text.as_str())
    .bind(alias.kind.as_str())
    .bind(alias.provenance.source_id.as_str())
    .bind(alias.provenance.received_at.as_datetime())
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

/// Alias texts of a node, in insertion order.
pub async fn aliases_of(
    conn: &mut PgConnection,
    node: CanonicalId,
) -> Result<Vec<String>, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT alias FROM aliases WHERE node_id = $1 ORDER BY id")
            .bind(node.uuid())
            .fetch_all(conn)
            .await?,
    )
}
