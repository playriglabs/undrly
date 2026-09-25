//! Quote feeds, market observations, and canonical quotes.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use undrly_core::{
    AggregationMethod, BidAsk, MarketObservation, ObservationBasis, PriceSubject, PriceUnit,
    QuoteFeed, SourceId, Timestamp, VenueSymbol,
};

use crate::Write;
use crate::error::{StoreError, corrupt};
use crate::mapping::{
    basis_from_sql, decimal_from_sql, decimal_to_sql, price_subject_from_sql, price_type_from_sql,
    price_unit_from_sql, timestamp_from_sql,
};
use crate::sources::SourceRecordId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QuoteFeedId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObservationId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredQuoteFeed {
    pub id: QuoteFeedId,
    pub feed: QuoteFeed,
    pub source_record: SourceRecordId,
}

fn subject_sql(subject: PriceSubject) -> (Uuid, &'static str) {
    let id = subject.canonical();
    (id.uuid(), id.category().as_str())
}

fn unit_sql(unit: PriceUnit) -> (Uuid, &'static str) {
    let id = unit.canonical();
    (id.uuid(), id.category().as_str())
}

// --- quote feeds -----------------------------------------------------------------

type FeedRow = (
    i64,
    String,
    String,
    Uuid,
    String,
    Uuid,
    String,
    String,
    Option<Uuid>,
    String,
    String,
    DateTime<Utc>,
    i64,
);

const FEED_SELECT: &str = "SELECT id, feed_source_id, symbol, subject_id, subject_category,
    unit_id, unit_category, basis, venue_id, price_type, source_id, received_at, source_record_id
    FROM quote_feeds";

fn feed_from_row(row: FeedRow) -> Result<StoredQuoteFeed, StoreError> {
    let (
        id,
        feed_source,
        symbol,
        subject,
        subject_category,
        unit,
        unit_category,
        basis,
        venue,
        price_type,
        source,
        received,
        record,
    ) = row;
    Ok(StoredQuoteFeed {
        id: QuoteFeedId(id),
        feed: QuoteFeed {
            feed_source: SourceId::parse(&feed_source).map_err(|e| corrupt("source id", e))?,
            symbol: VenueSymbol::new(&symbol).map_err(|e| corrupt("feed symbol", e))?,
            subject: price_subject_from_sql(subject, &subject_category)?,
            unit: price_unit_from_sql(unit, &unit_category)?,
            basis: basis_from_sql(&basis, venue)?,
            price_type: price_type_from_sql(&price_type)?,
            provenance: undrly_core::Provenance {
                source_id: SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
                received_at: timestamp_from_sql(received)?,
            },
        },
        source_record: SourceRecordId(record),
    })
}

/// Declares a quote feed, asserted by `source_record`. Re-declaring the same
/// feed is [`Write::Unchanged`] and keeps the original provenance; a
/// different meaning for the same (source, symbol, price type) is an error.
pub async fn insert_quote_feed(
    conn: &mut PgConnection,
    feed: &QuoteFeed,
    source_record: SourceRecordId,
) -> Result<(QuoteFeedId, Write), StoreError> {
    let (subject, subject_category) = subject_sql(feed.subject);
    let (unit, unit_category) = unit_sql(feed.unit);
    let venue = match feed.basis {
        ObservationBasis::Venue(v) => Some(v.uuid()),
        _ => None,
    };
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO quote_feeds
           (feed_source_id, symbol, subject_id, subject_category, unit_id, unit_category, basis,
            venue_id, price_type, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT ON CONSTRAINT quote_feeds_one_per_symbol DO NOTHING
         RETURNING id",
    )
    .bind(feed.feed_source.as_str())
    .bind(feed.symbol.as_str())
    .bind(subject)
    .bind(subject_category)
    .bind(unit)
    .bind(unit_category)
    .bind(feed.basis.as_str())
    .bind(venue)
    .bind(feed.price_type.as_str())
    .bind(feed.provenance.source_id.as_str())
    .bind(feed.provenance.received_at.as_datetime())
    .bind(source_record.0)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = inserted {
        return Ok((QuoteFeedId(id), Write::Inserted));
    }
    let row: FeedRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{FEED_SELECT} WHERE feed_source_id = $1 AND symbol = $2 AND price_type = $3"
    )))
    .bind(feed.feed_source.as_str())
    .bind(feed.symbol.as_str())
    .bind(feed.price_type.as_str())
    .fetch_one(conn)
    .await?;
    let existing = feed_from_row(row)?;
    let same = existing.feed.subject == feed.subject
        && existing.feed.unit == feed.unit
        && existing.feed.basis == feed.basis;
    if same {
        Ok((existing.id, Write::Unchanged))
    } else {
        Err(StoreError::ExistingRecordDiffers {
            what: "quote feed",
            key: format!("{}:{}", feed.feed_source, feed.symbol),
        })
    }
}

/// Feeds published by `feed_source`, in id order.
pub async fn quote_feeds_of_source(
    conn: &mut PgConnection,
    feed_source: &SourceId,
) -> Result<Vec<StoredQuoteFeed>, StoreError> {
    let rows: Vec<FeedRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{FEED_SELECT} WHERE feed_source_id = $1 ORDER BY id"
    )))
    .bind(feed_source.as_str())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(feed_from_row).collect()
}

// --- observations ----------------------------------------------------------------

type ObservationRow = (
    i64,
    Uuid,
    String,
    String,
    Option<Uuid>,
    String,
    String,
    Option<String>,
    Option<String>,
    Uuid,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

const OBSERVATION_COLUMNS: &str = "id, subject_id, subject_category, basis, venue_id, price_type,
    price::text, bid::text, ask::text, unit_id, unit_category, source_id, observed_at, received_at";

fn observation_from_row(
    row: ObservationRow,
) -> Result<(ObservationId, MarketObservation), StoreError> {
    let (
        id,
        subject,
        subject_category,
        basis,
        venue,
        price_type,
        price,
        bid,
        ask,
        unit,
        unit_category,
        source,
        observed,
        received,
    ) = row;
    let bid_ask = match (bid, ask) {
        (Some(bid), Some(ask)) => Some(BidAsk {
            bid: decimal_from_sql(&bid)?,
            ask: decimal_from_sql(&ask)?,
        }),
        (None, None) => None,
        _ => {
            return Err(corrupt(
                "observation",
                format!("row {id} has half a spread"),
            ));
        }
    };
    let observation = MarketObservation::new(
        price_subject_from_sql(subject, &subject_category)?,
        basis_from_sql(&basis, venue)?,
        price_type_from_sql(&price_type)?,
        decimal_from_sql(&price)?,
        bid_ask,
        price_unit_from_sql(unit, &unit_category)?,
        SourceId::parse(&source).map_err(|e| corrupt("source id", e))?,
        observed.map(timestamp_from_sql).transpose()?,
        timestamp_from_sql(received)?,
    )
    .map_err(|e| corrupt("observation", e))?;
    Ok((ObservationId(id), observation))
}

/// Stores an observation normalized from `source_record` (whose source and
/// receipt time must equal the observation's). A replay of the record, or a
/// later response restating the same source-timestamped price, is
/// [`Write::Unchanged`] and returns the existing row.
pub async fn insert_observation(
    conn: &mut PgConnection,
    observation: &MarketObservation,
    source_record: SourceRecordId,
) -> Result<(ObservationId, Write), StoreError> {
    let (subject, subject_category) = subject_sql(observation.subject());
    let (unit, unit_category) = unit_sql(observation.unit());
    let venue = observation.venue_id().map(|v| v.uuid());
    let bid_ask = observation.bid_ask();
    // ON CONFLICT without a target covers both replay keys.
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO market_observations
           (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask,
            unit_id, unit_category, source_id, observed_at, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6::numeric, $7::numeric, $8::numeric,
                 $9, $10, $11, $12, $13, $14)
         ON CONFLICT DO NOTHING
         RETURNING id",
    )
    .bind(subject)
    .bind(subject_category)
    .bind(observation.basis().as_str())
    .bind(venue)
    .bind(observation.price_type().as_str())
    .bind(decimal_to_sql(observation.price()))
    .bind(bid_ask.map(|b| decimal_to_sql(b.bid)))
    .bind(bid_ask.map(|b| decimal_to_sql(b.ask)))
    .bind(unit)
    .bind(unit_category)
    .bind(observation.source_id().as_str())
    .bind(observation.observed_at().map(|t| t.as_datetime()))
    .bind(observation.received_at().as_datetime())
    .bind(source_record.0)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = inserted {
        return Ok((ObservationId(id), Write::Inserted));
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM market_observations
         WHERE subject_id = $1 AND unit_id = $2 AND venue_id IS NOT DISTINCT FROM $3
           AND price_type = $4
           AND (source_record_id = $5
                OR (source_id = $6 AND basis = $7 AND observed_at = $8))
         ORDER BY id LIMIT 1",
    )
    .bind(subject)
    .bind(unit)
    .bind(venue)
    .bind(observation.price_type().as_str())
    .bind(source_record.0)
    .bind(observation.source_id().as_str())
    .bind(observation.basis().as_str())
    .bind(observation.observed_at().map(|t| t.as_datetime()))
    .fetch_one(conn)
    .await?;
    Ok((ObservationId(id), Write::Unchanged))
}

pub async fn get_observation(
    conn: &mut PgConnection,
    id: ObservationId,
) -> Result<Option<MarketObservation>, StoreError> {
    let row: Option<ObservationRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {OBSERVATION_COLUMNS} FROM market_observations WHERE id = $1"
    )))
    .bind(id.0)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(observation_from_row).transpose()?.map(|(_, o)| o))
}

/// The latest observation of each (source, venue, price type) pricing
/// `subject` in `unit`: the aggregation input.
pub async fn latest_observations(
    conn: &mut PgConnection,
    subject: PriceSubject,
    unit: PriceUnit,
) -> Result<Vec<(ObservationId, MarketObservation)>, StoreError> {
    let rows: Vec<ObservationRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {OBSERVATION_COLUMNS} FROM (
           SELECT DISTINCT ON (source_id, venue_id, price_type) *
           FROM market_observations
           WHERE subject_id = $1 AND unit_id = $2
           ORDER BY source_id, venue_id, price_type,
                    COALESCE(observed_at, received_at) DESC, received_at DESC, id DESC
         ) latest ORDER BY id"
    )))
    .bind(subject.canonical().uuid())
    .bind(unit.canonical().uuid())
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(observation_from_row).collect()
}

// --- canonical quotes ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCanonicalQuote {
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub observation: ObservationId,
    pub method: AggregationMethod,
    pub eligible_count: i32,
    pub computed_at: Timestamp,
}

/// Writes the canonical quote of (subject, unit). This table is a derived
/// cache, so it is overwritten: [`Write::Unchanged`] when the same
/// observation was already selected.
pub async fn upsert_canonical_quote(
    conn: &mut PgConnection,
    quote: &StoredCanonicalQuote,
) -> Result<Write, StoreError> {
    let (subject, subject_category) = subject_sql(quote.subject);
    let (unit, unit_category) = unit_sql(quote.unit);
    let changed = sqlx::query(
        "INSERT INTO canonical_quotes
           (subject_id, subject_category, unit_id, unit_category, observation_id, method,
            eligible_count, computed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (subject_id, unit_id) DO UPDATE
           SET observation_id = EXCLUDED.observation_id, method = EXCLUDED.method,
               eligible_count = EXCLUDED.eligible_count, computed_at = EXCLUDED.computed_at
           WHERE canonical_quotes.observation_id <> EXCLUDED.observation_id
              OR canonical_quotes.eligible_count <> EXCLUDED.eligible_count",
    )
    .bind(subject)
    .bind(subject_category)
    .bind(unit)
    .bind(unit_category)
    .bind(quote.observation.0)
    .bind(quote.method.as_str())
    .bind(quote.eligible_count)
    .bind(quote.computed_at.as_datetime())
    .execute(conn)
    .await?
    .rows_affected()
        == 1;
    Ok(if changed {
        Write::Inserted
    } else {
        Write::Unchanged
    })
}

pub async fn get_canonical_quote(
    conn: &mut PgConnection,
    subject: PriceSubject,
    unit: PriceUnit,
) -> Result<Option<StoredCanonicalQuote>, StoreError> {
    let row: Option<(i64, String, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT observation_id, method, eligible_count, computed_at FROM canonical_quotes
         WHERE subject_id = $1 AND unit_id = $2",
    )
    .bind(subject.canonical().uuid())
    .bind(unit.canonical().uuid())
    .fetch_optional(conn)
    .await?;
    row.map(|(observation, method, eligible_count, computed)| {
        Ok(StoredCanonicalQuote {
            subject,
            unit,
            observation: ObservationId(observation),
            method: AggregationMethod::ALL
                .into_iter()
                .find(|m| m.as_str() == method)
                .ok_or_else(|| corrupt("aggregation method", &method))?,
            eligible_count,
            computed_at: timestamp_from_sql(computed)?,
        })
    })
    .transpose()
}
