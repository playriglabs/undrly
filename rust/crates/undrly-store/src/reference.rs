//! Canonical nodes and reference objects.
//!
//! A node can only be created together with its category row (entity,
//! instrument, ...), in one transaction, so storage never holds a node without
//! its object. Inserts are idempotent: re-inserting identical content returns
//! [`Write::Unchanged`]; different content for an existing id is an error, never
//! an overwrite.
//!
//! Every object row names the raw source record that minted it
//! (`source_record_id`). A replay from another record keeps the original.

use chrono::{DateTime, Utc};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    CanonicalId, Category, Currency, CurrencyId, DisplayName, Entity, EntityId, FxPair, Instrument,
    InstrumentId, Listing, ListingId, Provenance, SourceId, UnitOfMeasure, Venue, VenueId,
};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{
    canonical_id_from_sql, decimal_from_sql, decimal_to_sql, entity_kind_from_sql,
    instrument_class_from_sql, timestamp_from_sql, unit_of_measure_from_sql,
};
use crate::sources::SourceRecordId;

/// Registers `id` in `nodes`. Must run inside the transaction that inserts the
/// category row.
async fn insert_node(conn: &mut PgConnection, id: CanonicalId) -> Result<(), StoreError> {
    let inserted =
        sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(id.uuid())
            .bind(id.category().as_str())
            .execute(&mut *conn)
            .await?
            .rows_affected()
            == 1;
    if !inserted && get_node(conn, id.uuid()).await? != Some(id) {
        return Err(StoreError::ExistingRecordDiffers {
            what: "node",
            key: id.to_string(),
        });
    }
    Ok(())
}

/// Looks up a canonical id by its UUID.
pub async fn get_node(
    conn: &mut PgConnection,
    uuid: sqlx::types::Uuid,
) -> Result<Option<CanonicalId>, StoreError> {
    let category: Option<String> = sqlx::query_scalar("SELECT category FROM nodes WHERE id = $1")
        .bind(uuid)
        .fetch_optional(conn)
        .await?;
    Ok(category
        .map(|c| canonical_id_from_sql(uuid, &c))
        .transpose()?)
}

/// Whether `id` has its object row (entity, instrument, ...). Nodes are only
/// ever created together with their object, so `false` indicates data written
/// outside the repositories.
pub async fn has_object(conn: &mut PgConnection, id: CanonicalId) -> Result<bool, StoreError> {
    let sql = match id.category() {
        Category::Entity => "SELECT EXISTS (SELECT 1 FROM entities WHERE id = $1)",
        Category::Instrument => "SELECT EXISTS (SELECT 1 FROM instruments WHERE id = $1)",
        Category::Listing => "SELECT EXISTS (SELECT 1 FROM listings WHERE id = $1)",
        Category::Venue => "SELECT EXISTS (SELECT 1 FROM venues WHERE id = $1)",
        Category::Currency => "SELECT EXISTS (SELECT 1 FROM currencies WHERE id = $1)",
    };
    Ok(sqlx::query_scalar(sql)
        .bind(id.uuid())
        .fetch_one(conn)
        .await?)
}

/// The raw record that minted `id`'s object row, if the object exists.
pub async fn object_source_record(
    conn: &mut PgConnection,
    id: CanonicalId,
) -> Result<Option<SourceRecordId>, StoreError> {
    let sql = match id.category() {
        Category::Entity => "SELECT source_record_id FROM entities WHERE id = $1",
        Category::Instrument => "SELECT source_record_id FROM instruments WHERE id = $1",
        Category::Listing => "SELECT source_record_id FROM listings WHERE id = $1",
        Category::Venue => "SELECT source_record_id FROM venues WHERE id = $1",
        Category::Currency => "SELECT source_record_id FROM currencies WHERE id = $1",
    };
    let id: Option<i64> = sqlx::query_scalar(sql)
        .bind(id.uuid())
        .fetch_optional(conn)
        .await?;
    Ok(id.map(SourceRecordId))
}

fn name(value: &str) -> Result<DisplayName, StoreError> {
    DisplayName::new(value).map_err(|e| corrupt("display name", e))
}

fn outcome<T: PartialEq>(
    inserted: bool,
    existing: Option<T>,
    new: &T,
    what: &'static str,
    key: impl ToString,
) -> Result<Write, StoreError> {
    if inserted {
        return Ok(Write::Inserted);
    }
    match existing {
        Some(existing) if existing == *new => Ok(Write::Unchanged),
        _ => Err(StoreError::ExistingRecordDiffers {
            what,
            key: key.to_string(),
        }),
    }
}

// --- entities ------------------------------------------------------------------

pub async fn insert_entity(
    conn: &mut PgConnection,
    entity: &Entity,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let mut tx = conn.begin().await?;
    insert_node(&mut tx, entity.id.canonical()).await?;
    let inserted = sqlx::query(
        "INSERT INTO entities (id, entity_kind, name, source_record_id) VALUES ($1, $2, $3, $4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(entity.id.uuid())
    .bind(entity.kind.as_str())
    .bind(entity.name.as_str())
    .bind(source_record.0)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let existing = if inserted {
        None
    } else {
        get_entity(&mut tx, entity.id).await?
    };
    let result = outcome(inserted, existing, entity, "entity", entity.id)?;
    tx.commit().await?;
    Ok(result)
}

pub async fn get_entity(
    conn: &mut PgConnection,
    id: EntityId,
) -> Result<Option<Entity>, StoreError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT entity_kind, name FROM entities WHERE id = $1")
            .bind(id.uuid())
            .fetch_optional(conn)
            .await?;
    row.map(|(kind, n)| {
        Ok(Entity {
            id,
            kind: entity_kind_from_sql(&kind)?,
            name: name(&n)?,
        })
    })
    .transpose()
}

// --- instruments ---------------------------------------------------------------

pub async fn insert_instrument(
    conn: &mut PgConnection,
    instrument: &Instrument,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let mut tx = conn.begin().await?;
    insert_node(&mut tx, instrument.id.canonical()).await?;
    let inserted = sqlx::query(
        "INSERT INTO instruments
           (id, instrument_class, name, contract_multiplier, unit_of_measure, base_currency_id,
            quote_currency_id, source_record_id)
         VALUES ($1, $2, $3, $4::numeric, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
    )
    .bind(instrument.id.uuid())
    .bind(instrument.class.as_str())
    .bind(instrument.name.as_str())
    .bind(instrument.contract_multiplier.map(decimal_to_sql))
    .bind(instrument.unit_of_measure.map(UnitOfMeasure::as_str))
    .bind(instrument.fx_pair.map(|p| p.base.uuid()))
    .bind(instrument.fx_pair.map(|p| p.quote.uuid()))
    .bind(source_record.0)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let existing = if inserted {
        None
    } else {
        get_instrument(&mut tx, instrument.id).await?
    };
    let result = outcome(inserted, existing, instrument, "instrument", instrument.id)?;
    tx.commit().await?;
    Ok(result)
}

pub async fn get_instrument(
    conn: &mut PgConnection,
    id: InstrumentId,
) -> Result<Option<Instrument>, StoreError> {
    type Row = (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<Uuid>,
        Option<Uuid>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT instrument_class, name, contract_multiplier::text, unit_of_measure,
                base_currency_id, quote_currency_id
         FROM instruments WHERE id = $1",
    )
    .bind(id.uuid())
    .fetch_optional(conn)
    .await?;
    row.map(|(class, n, multiplier, unit, base, quote)| {
        let fx_pair = match (base, quote) {
            (Some(base), Some(quote)) => Some(FxPair {
                base: CurrencyId::from_uuid(base).map_err(|e| corrupt("base currency", e))?,
                quote: CurrencyId::from_uuid(quote).map_err(|e| corrupt("quote currency", e))?,
            }),
            _ => None,
        };
        Ok(Instrument {
            id,
            class: instrument_class_from_sql(&class)?,
            name: name(&n)?,
            contract_multiplier: multiplier.as_deref().map(decimal_from_sql).transpose()?,
            unit_of_measure: unit.as_deref().map(unit_of_measure_from_sql).transpose()?,
            fx_pair,
        })
    })
    .transpose()
}

// --- venues --------------------------------------------------------------------

pub async fn insert_venue(
    conn: &mut PgConnection,
    venue: &Venue,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let mut tx = conn.begin().await?;
    insert_node(&mut tx, venue.id.canonical()).await?;
    let inserted = sqlx::query(
        "INSERT INTO venues (id, name, source_record_id) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(venue.id.uuid())
    .bind(venue.name.as_str())
    .bind(source_record.0)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let existing = if inserted {
        None
    } else {
        get_venue(&mut tx, venue.id).await?
    };
    let result = outcome(inserted, existing, venue, "venue", venue.id)?;
    tx.commit().await?;
    Ok(result)
}

pub async fn get_venue(conn: &mut PgConnection, id: VenueId) -> Result<Option<Venue>, StoreError> {
    let row: Option<String> = sqlx::query_scalar("SELECT name FROM venues WHERE id = $1")
        .bind(id.uuid())
        .fetch_optional(conn)
        .await?;
    row.map(|n| {
        Ok(Venue {
            id,
            name: name(&n)?,
        })
    })
    .transpose()
}

// --- currencies ----------------------------------------------------------------

pub async fn insert_currency(
    conn: &mut PgConnection,
    currency: &Currency,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let mut tx = conn.begin().await?;
    insert_node(&mut tx, currency.id.canonical()).await?;
    let inserted = sqlx::query(
        "INSERT INTO currencies (id, name, source_record_id) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(currency.id.uuid())
    .bind(currency.name.as_str())
    .bind(source_record.0)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let existing = if inserted {
        None
    } else {
        get_currency(&mut tx, currency.id).await?
    };
    let result = outcome(inserted, existing, currency, "currency", currency.id)?;
    tx.commit().await?;
    Ok(result)
}

pub async fn get_currency(
    conn: &mut PgConnection,
    id: CurrencyId,
) -> Result<Option<Currency>, StoreError> {
    let row: Option<String> = sqlx::query_scalar("SELECT name FROM currencies WHERE id = $1")
        .bind(id.uuid())
        .fetch_optional(conn)
        .await?;
    row.map(|n| {
        Ok(Currency {
            id,
            name: name(&n)?,
        })
    })
    .transpose()
}

// --- listings ------------------------------------------------------------------

type ListingRow = (
    sqlx::types::Uuid,
    sqlx::types::Uuid,
    sqlx::types::Uuid,
    String,
    DateTime<Utc>,
);

fn listing_from_row(
    (id, instrument_id, venue_id, source_id, received_at): ListingRow,
) -> Result<Listing, StoreError> {
    Ok(Listing {
        id: ListingId::from_uuid(id).map_err(|e| corrupt("listing id", e))?,
        instrument_id: InstrumentId::from_uuid(instrument_id)
            .map_err(|e| corrupt("instrument id", e))?,
        venue_id: VenueId::from_uuid(venue_id).map_err(|e| corrupt("venue id", e))?,
        provenance: Provenance {
            source_id: SourceId::parse(&source_id).map_err(|e| corrupt("source id", e))?,
            received_at: timestamp_from_sql(received_at)?,
        },
    })
}

/// Inserts a listing asserted by `source_record`, whose source and receipt
/// time must equal `listing.provenance`. A replay with the same instrument and
/// venue is [`Write::Unchanged`] and keeps the original provenance and record.
pub async fn insert_listing(
    conn: &mut PgConnection,
    listing: &Listing,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let mut tx = conn.begin().await?;
    insert_node(&mut tx, listing.id.canonical()).await?;
    let inserted = sqlx::query(
        "INSERT INTO listings (id, instrument_id, venue_id, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (id) DO NOTHING",
    )
    .bind(listing.id.uuid())
    .bind(listing.instrument_id.uuid())
    .bind(listing.venue_id.uuid())
    .bind(listing.provenance.source_id.as_str())
    .bind(listing.provenance.received_at.as_datetime())
    .bind(source_record.0)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let result = if inserted {
        Write::Inserted
    } else {
        match get_listing(&mut tx, listing.id).await? {
            Some(existing)
                if existing.instrument_id == listing.instrument_id
                    && existing.venue_id == listing.venue_id =>
            {
                Write::Unchanged
            }
            _ => {
                return Err(StoreError::ExistingRecordDiffers {
                    what: "listing",
                    key: listing.id.to_string(),
                });
            }
        }
    };
    tx.commit().await?;
    Ok(result)
}

pub async fn get_listing(
    conn: &mut PgConnection,
    id: ListingId,
) -> Result<Option<Listing>, StoreError> {
    let row: Option<ListingRow> = sqlx::query_as(
        "SELECT id, instrument_id, venue_id, source_id, received_at FROM listings WHERE id = $1",
    )
    .bind(id.uuid())
    .fetch_optional(conn)
    .await?;
    row.map(listing_from_row).transpose()
}

/// Listings of an instrument, ordered by id (creation order).
pub async fn listings_for_instrument(
    conn: &mut PgConnection,
    instrument_id: InstrumentId,
) -> Result<Vec<Listing>, StoreError> {
    let rows: Vec<ListingRow> = sqlx::query_as(
        "SELECT id, instrument_id, venue_id, source_id, received_at FROM listings
         WHERE instrument_id = $1 ORDER BY id",
    )
    .bind(instrument_id.uuid())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(listing_from_row).collect()
}
