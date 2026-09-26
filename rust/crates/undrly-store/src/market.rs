//! Quote feeds, market observations, and canonical quotes.

use chrono::{DateTime, Utc};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    AggregationMethod, BidAsk, Decimal, FreshnessClock, MarketObservation, ObservationBasis,
    PriceSubject, PriceType, PriceUnit, QuoteAggregation, QuoteFeed, SourceId, Timestamp,
    VenueSymbol,
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
    i32,
    String,
    bool,
);

const FEED_SELECT: &str = "SELECT id, feed_source_id, symbol, subject_id, subject_category,
    unit_id, unit_category, basis, venue_id, price_type, source_id, received_at, source_record_id,
    stale_after_seconds, freshness_clock, inverted FROM quote_feeds";

fn freshness_clock_from_sql(value: &str) -> Result<FreshnessClock, StoreError> {
    FreshnessClock::ALL
        .into_iter()
        .find(|c| c.as_str() == value)
        .ok_or_else(|| corrupt("freshness clock", value))
}

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
        stale_after,
        clock,
        inverted,
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
            stale_after_seconds: u32::try_from(stale_after)
                .map_err(|e| corrupt("stale_after_seconds", e))?,
            freshness_clock: freshness_clock_from_sql(&clock)?,
            inverted,
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
            venue_id, price_type, source_id, received_at, source_record_id, stale_after_seconds,
            freshness_clock, inverted)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
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
    .bind(i32::try_from(feed.stale_after_seconds).unwrap_or(i32::MAX))
    .bind(feed.freshness_clock.as_str())
    .bind(feed.inverted)
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
        && existing.feed.basis == feed.basis
        && existing.feed.stale_after_seconds == feed.stale_after_seconds
        && existing.feed.freshness_clock == feed.freshness_clock
        && existing.feed.inverted == feed.inverted;
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
    insert_observation_with(conn, observation, source_record, false).await
}

/// [`insert_observation`], recording whether the price was inverted from
/// the source's inverse pair (`inverted`; the raw record keeps the source's
/// own value).
pub async fn insert_observation_with(
    conn: &mut PgConnection,
    observation: &MarketObservation,
    source_record: SourceRecordId,
    inverted: bool,
) -> Result<(ObservationId, Write), StoreError> {
    let (subject, subject_category) = subject_sql(observation.subject());
    let (unit, unit_category) = unit_sql(observation.unit());
    let venue = observation.venue_id().map(|v| v.uuid());
    let bid_ask = observation.bid_ask();
    // ON CONFLICT without a target covers both replay keys.
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO market_observations
           (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask,
            unit_id, unit_category, source_id, observed_at, received_at, source_record_id,
            inverted)
         VALUES ($1, $2, $3, $4, $5, $6::numeric, $7::numeric, $8::numeric,
                 $9, $10, $11, $12, $13, $14, $15)
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
    .bind(inverted)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = inserted {
        return Ok((ObservationId(id), Write::Inserted));
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM market_observations
         WHERE subject_id = $1 AND unit_id = $2 AND venue_id IS NOT DISTINCT FROM $3
           AND price_type = $4
           AND observed_at IS NOT DISTINCT FROM $8
           AND (source_record_id = $5 OR (source_id = $6 AND basis = $7 AND $8::timestamptz IS NOT NULL))
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

// --- aggregation declarations ---------------------------------------------------

/// Declares a pair's aggregation method, asserted by `source_record`. The same
/// declaration again is [`Write::Unchanged`]; a different method for the
/// pair is an error (never overwritten).
pub async fn insert_quote_aggregation(
    conn: &mut PgConnection,
    a: &QuoteAggregation,
    source_record: SourceRecordId,
) -> Result<Write, StoreError> {
    let (subject, subject_category) = subject_sql(a.subject);
    let (unit, unit_category) = unit_sql(a.unit);
    let inserted = sqlx::query(
        "INSERT INTO quote_aggregations
           (subject_id, subject_category, unit_id, unit_category, method, source_id,
            received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (subject_id, unit_id) DO NOTHING",
    )
    .bind(subject)
    .bind(subject_category)
    .bind(unit)
    .bind(unit_category)
    .bind(a.method.as_str())
    .bind(a.provenance.source_id.as_str())
    .bind(a.provenance.received_at.as_datetime())
    .bind(source_record.0)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        == 1;
    if inserted {
        return Ok(Write::Inserted);
    }
    if aggregation_method_of(conn, a.subject, a.unit).await? == a.method {
        Ok(Write::Unchanged)
    } else {
        Err(StoreError::ExistingRecordDiffers {
            what: "quote aggregation",
            key: format!("{} in {}", a.subject.canonical(), a.unit.canonical()),
        })
    }
}

fn method_from_sql(value: &str) -> Result<AggregationMethod, StoreError> {
    AggregationMethod::ALL
        .into_iter()
        .find(|m| m.as_str() == value)
        .ok_or_else(|| corrupt("aggregation method", value))
}

/// The method declared for a pair, or `latest-observation-v1`.
pub async fn aggregation_method_of(
    conn: &mut PgConnection,
    subject: PriceSubject,
    unit: PriceUnit,
) -> Result<AggregationMethod, StoreError> {
    let method: Option<String> = sqlx::query_scalar(
        "SELECT method FROM quote_aggregations WHERE subject_id = $1 AND unit_id = $2",
    )
    .bind(subject.canonical().uuid())
    .bind(unit.canonical().uuid())
    .fetch_optional(conn)
    .await?;
    method.map_or(Ok(AggregationMethod::LatestObservationV1), |m| {
        method_from_sql(&m)
    })
}

// --- canonical quotes ------------------------------------------------------------

/// A canonical quote: an aggregation method's output for (subject, unit), and
/// exactly the observations it used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCanonicalQuote {
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub method: AggregationMethod,
    pub price: Decimal,
    pub price_type: PriceType,
    pub basis: ObservationBasis,
    /// The aggregation's bid and ask (mean bid/ask for `mean-venue-mid-v1`).
    pub bid_ask: Option<BidAsk>,
    pub as_of: Timestamp,
    pub computed_at: Timestamp,
    /// The observations used, each with the price it contributed, by id.
    pub inputs: Vec<(ObservationId, Decimal)>,
}

impl StoredCanonicalQuote {
    pub fn eligible_count(&self) -> usize {
        self.inputs.len()
    }

    /// Equal except for when it was computed.
    fn same_result(&self, other: &Self) -> bool {
        self.method == other.method
            && self.price == other.price
            && self.price.scale() == other.price.scale()
            && self.price_type == other.price_type
            && self.basis == other.basis
            && same_bid_ask(self.bid_ask, other.bid_ask)
            && self.as_of == other.as_of
            && self.inputs == other.inputs
    }
}

/// Equal values at equal scales.
fn same_bid_ask(a: Option<BidAsk>, b: Option<BidAsk>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a == b && a.bid.scale() == b.bid.scale() && a.ask.scale() == b.ask.scale()
        }
        _ => false,
    }
}

/// Writes the canonical quote of a pair and its inputs, in one transaction.
/// This table is a derived cache, so it is overwritten; recomputing the same
/// result is [`Write::Unchanged`] and keeps the earlier `computed_at`.
pub async fn upsert_canonical_quote(
    conn: &mut PgConnection,
    quote: &StoredCanonicalQuote,
) -> Result<Write, StoreError> {
    if quote.inputs.is_empty() {
        return Err(corrupt("canonical quote", "a canonical quote needs inputs"));
    }
    let mut tx = conn.begin().await?;
    if let Some(existing) = get_canonical_quote(&mut tx, quote.subject, quote.unit).await?
        && existing.same_result(quote)
    {
        tx.commit().await?;
        return Ok(Write::Unchanged);
    }
    let (subject, subject_category) = subject_sql(quote.subject);
    let (unit, unit_category) = unit_sql(quote.unit);
    let venue = match quote.basis {
        ObservationBasis::Venue(v) => Some(v.uuid()),
        _ => None,
    };
    sqlx::query("DELETE FROM canonical_quotes WHERE subject_id = $1 AND unit_id = $2")
        .bind(subject)
        .bind(unit)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO canonical_quotes
           (subject_id, subject_category, unit_id, unit_category, method, price, price_type,
            basis, venue_id, as_of, eligible_count, computed_at, bid, ask)
         VALUES ($1, $2, $3, $4, $5, $6::numeric, $7, $8, $9, $10, $11, $12,
                 $13::numeric, $14::numeric)",
    )
    .bind(subject)
    .bind(subject_category)
    .bind(unit)
    .bind(unit_category)
    .bind(quote.method.as_str())
    .bind(decimal_to_sql(quote.price))
    .bind(quote.price_type.as_str())
    .bind(quote.basis.as_str())
    .bind(venue)
    .bind(quote.as_of.as_datetime())
    .bind(i32::try_from(quote.inputs.len()).unwrap_or(i32::MAX))
    .bind(quote.computed_at.as_datetime())
    .bind(quote.bid_ask.map(|ba| decimal_to_sql(ba.bid)))
    .bind(quote.bid_ask.map(|ba| decimal_to_sql(ba.ask)))
    .execute(&mut *tx)
    .await?;
    for (observation, input_price) in &quote.inputs {
        sqlx::query(
            "INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
             VALUES ($1, $2, $3, $4::numeric)",
        )
        .bind(subject)
        .bind(unit)
        .bind(observation.0)
        .bind(decimal_to_sql(*input_price))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Write::Inserted)
}

/// Removes a pair's canonical quote (nothing is eligible). Returns whether one
/// existed. Observations are untouched.
pub async fn delete_canonical_quote(
    conn: &mut PgConnection,
    subject: PriceSubject,
    unit: PriceUnit,
) -> Result<bool, StoreError> {
    Ok(
        sqlx::query("DELETE FROM canonical_quotes WHERE subject_id = $1 AND unit_id = $2")
            .bind(subject.canonical().uuid())
            .bind(unit.canonical().uuid())
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}

pub async fn get_canonical_quote(
    conn: &mut PgConnection,
    subject: PriceSubject,
    unit: PriceUnit,
) -> Result<Option<StoredCanonicalQuote>, StoreError> {
    type Row = (
        String,
        String,
        String,
        String,
        Option<Uuid>,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<String>,
        Option<String>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT method, price::text, price_type, basis, venue_id, as_of, computed_at,
                bid::text, ask::text
         FROM canonical_quotes WHERE subject_id = $1 AND unit_id = $2",
    )
    .bind(subject.canonical().uuid())
    .bind(unit.canonical().uuid())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((method, price, price_type, basis, venue, as_of, computed, bid, ask)) = row else {
        return Ok(None);
    };
    let inputs: Vec<(i64, String)> = sqlx::query_as(
        "SELECT observation_id, input_price::text FROM canonical_quote_inputs
         WHERE subject_id = $1 AND unit_id = $2 ORDER BY observation_id",
    )
    .bind(subject.canonical().uuid())
    .bind(unit.canonical().uuid())
    .fetch_all(conn)
    .await?;
    Ok(Some(StoredCanonicalQuote {
        subject,
        unit,
        method: method_from_sql(&method)?,
        price: decimal_from_sql(&price)?,
        price_type: price_type_from_sql(&price_type)?,
        basis: basis_from_sql(&basis, venue)?,
        bid_ask: match (bid, ask) {
            (None, None) => None,
            (Some(bid), Some(ask)) => Some(BidAsk {
                bid: decimal_from_sql(&bid)?,
                ask: decimal_from_sql(&ask)?,
            }),
            _ => return Err(corrupt("canonical quote", "bid and ask come together")),
        },
        as_of: timestamp_from_sql(as_of)?,
        computed_at: timestamp_from_sql(computed)?,
        inputs: inputs
            .into_iter()
            .map(|(id, p)| Ok((ObservationId(id), decimal_from_sql(&p)?)))
            .collect::<Result<_, StoreError>>()?,
    }))
}
