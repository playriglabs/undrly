//! V1.3 market data: bars, perpetual contexts, trading sessions (migration
//! 0015) and corporate actions (0016). Each row names its raw source record.

use chrono::NaiveDate;
use sqlx::PgConnection;
use undrly_core::{
    Bar, CorporateAction, EarningsReport, PerpContext, PriceSubject, PriceUnit, TradingSession,
    VenueId,
};

use crate::Write;
use crate::error::StoreError;
use crate::mapping::decimal_to_sql;
use crate::sources::RecordProvenance;

/// Outcome of storing a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarWrite {
    Inserted,
    /// A newer record changed a stored bar (it was still in progress).
    Replaced,
    /// The same values, or an older record: nothing changed.
    Unchanged,
}

fn unit_sql(unit: PriceUnit) -> (sqlx::types::Uuid, &'static str) {
    let id = unit.canonical();
    (id.uuid(), id.category().as_str())
}

/// Stores a bar normalized from `record`. A bar for the same market,
/// source, venue, interval and open time is replaced only by a newer record
/// with different values (an in-progress bar fetched again); the stored row
/// always names the record its values came from.
pub async fn upsert_bar(
    conn: &mut PgConnection,
    bar: &Bar,
    record: &RecordProvenance,
) -> Result<BarWrite, StoreError> {
    let PriceSubject::Instrument(subject) = bar.subject else {
        return Err(StoreError::Corrupt {
            what: "bar",
            detail: "bars are stored for instruments only".into(),
        });
    };
    let (unit, unit_category) = unit_sql(bar.unit);
    let row: Option<(bool,)> = sqlx::query_as(
        "INSERT INTO market_bars
           (subject_id, subject_category, unit_id, unit_category, source_id, venue_id,
            bar_interval, open_time, close_time, open, high, low, close, volume, trade_count,
            received_at, source_record_id)
         VALUES ($1, 'instrument', $2, $3, $4, $5, $6, $7, $8, $9::numeric, $10::numeric,
                 $11::numeric, $12::numeric, $13::numeric, $14, $15, $16)
         ON CONFLICT ON CONSTRAINT market_bars_one_per_period DO UPDATE SET
           close_time = EXCLUDED.close_time, open = EXCLUDED.open, high = EXCLUDED.high,
           low = EXCLUDED.low, close = EXCLUDED.close, volume = EXCLUDED.volume,
           trade_count = EXCLUDED.trade_count, received_at = EXCLUDED.received_at,
           source_record_id = EXCLUDED.source_record_id
         WHERE market_bars.received_at < EXCLUDED.received_at
           AND (market_bars.close_time, market_bars.open, market_bars.high, market_bars.low,
                market_bars.close, market_bars.volume, market_bars.trade_count)
               IS DISTINCT FROM
               (EXCLUDED.close_time, EXCLUDED.open, EXCLUDED.high, EXCLUDED.low,
                EXCLUDED.close, EXCLUDED.volume, EXCLUDED.trade_count)
         RETURNING (xmax = 0)",
    )
    .bind(subject.uuid())
    .bind(unit)
    .bind(unit_category)
    .bind(record.provenance.source_id.as_str())
    .bind(bar.venue.uuid())
    .bind(bar.interval.as_str())
    .bind(bar.open_time.as_datetime())
    .bind(bar.close_time.as_datetime())
    .bind(decimal_to_sql(bar.open))
    .bind(decimal_to_sql(bar.high))
    .bind(decimal_to_sql(bar.low))
    .bind(decimal_to_sql(bar.close))
    .bind(bar.volume.map(decimal_to_sql))
    .bind(
        bar.trade_count
            .map(|n| i64::try_from(n).unwrap_or(i64::MAX)),
    )
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
    .fetch_optional(conn)
    .await?;
    Ok(match row {
        Some((true,)) => BarWrite::Inserted,
        Some((false,)) => BarWrite::Replaced,
        None => BarWrite::Unchanged,
    })
}

/// Stores a perpetual's context from `record` (one per record and market).
pub async fn insert_perp_context(
    conn: &mut PgConnection,
    c: &PerpContext,
    record: &RecordProvenance,
) -> Result<Write, StoreError> {
    let (unit, unit_category) = unit_sql(c.unit);
    let inserted = sqlx::query(
        "INSERT INTO perp_contexts
           (subject_id, unit_id, unit_category, source_id, venue_id, mark_price, oracle_price,
            mid_price, funding_rate, funding_interval_hours, open_interest, volume_24h_base,
            volume_24h_notional, price_24h_ago, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6::numeric, $7::numeric, $8::numeric, $9::numeric, $10,
                 $11::numeric, $12::numeric, $13::numeric, $14::numeric, $15, $16)
         ON CONFLICT ON CONSTRAINT perp_contexts_record_key DO NOTHING",
    )
    .bind(c.perpetual.uuid())
    .bind(unit)
    .bind(unit_category)
    .bind(record.provenance.source_id.as_str())
    .bind(c.venue.uuid())
    .bind(decimal_to_sql(c.mark_price))
    .bind(c.oracle_price.map(decimal_to_sql))
    .bind(c.mid_price.map(decimal_to_sql))
    .bind(c.funding_rate.map(|(r, _)| decimal_to_sql(r)))
    .bind(
        c.funding_rate
            .map(|(_, h)| i32::try_from(h).unwrap_or(i32::MAX)),
    )
    .bind(c.open_interest.map(decimal_to_sql))
    .bind(c.volume_24h_base.map(decimal_to_sql))
    .bind(c.volume_24h_notional.map(decimal_to_sql))
    .bind(c.price_24h_ago.map(decimal_to_sql))
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
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

/// A reference series' open/high/low/close over the source's own window
/// (V1.9, `reference_windows`), for one (subject, unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceWindow {
    pub subject: undrly_core::PriceSubject,
    pub unit: undrly_core::PriceUnit,
    pub start: undrly_core::Timestamp,
    pub end: undrly_core::Timestamp,
    pub open: undrly_core::Decimal,
    pub high: undrly_core::Decimal,
    pub low: undrly_core::Decimal,
    pub close: undrly_core::Decimal,
}

/// Stores a window from `record`; the same window again is [`Write::Unchanged`].
pub async fn insert_reference_window(
    conn: &mut PgConnection,
    w: &ReferenceWindow,
    record: &RecordProvenance,
) -> Result<Write, StoreError> {
    let subject = w.subject.canonical();
    let unit = w.unit.canonical();
    let inserted = sqlx::query(
        "INSERT INTO reference_windows
           (subject_id, subject_category, unit_id, unit_category, source_id, window_start,
            window_end, open, high, low, close, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::numeric, $9::numeric, $10::numeric,
                 $11::numeric, $12, $13)
         ON CONFLICT (subject_id, unit_id, source_id, window_start, window_end) DO NOTHING",
    )
    .bind(subject.uuid())
    .bind(subject.category().as_str())
    .bind(unit.uuid())
    .bind(unit.category().as_str())
    .bind(record.provenance.source_id.as_str())
    .bind(w.start.as_datetime())
    .bind(w.end.as_datetime())
    .bind(decimal_to_sql(w.open))
    .bind(decimal_to_sql(w.high))
    .bind(decimal_to_sql(w.low))
    .bind(decimal_to_sql(w.close))
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
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

/// Stores a venue's calendar from `record`: its sessions, and the range of
/// dates the calendar covers (dates in it without a session are closed).
/// A session already stored for a date is replaced by a newer record's.
pub async fn store_trading_calendar(
    conn: &mut PgConnection,
    venue: VenueId,
    first_date: NaiveDate,
    last_date: NaiveDate,
    sessions: &[TradingSession],
    record: &RecordProvenance,
) -> Result<usize, StoreError> {
    let mut written = 0;
    for s in sessions {
        if s.venue != venue || s.date < first_date || s.date > last_date || !s.is_ordered() {
            return Err(StoreError::Corrupt {
                what: "trading session",
                detail: format!("{} outside the calendar or out of order", s.date),
            });
        }
        written += sqlx::query(
            "INSERT INTO trading_sessions
               (venue_id, session_date, pre_open_at, open_at, close_at, post_close_at,
                source_id, received_at, source_record_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (venue_id, session_date) DO UPDATE SET
               pre_open_at = EXCLUDED.pre_open_at, open_at = EXCLUDED.open_at,
               close_at = EXCLUDED.close_at, post_close_at = EXCLUDED.post_close_at,
               source_id = EXCLUDED.source_id, received_at = EXCLUDED.received_at,
               source_record_id = EXCLUDED.source_record_id
             WHERE trading_sessions.received_at < EXCLUDED.received_at
               AND (trading_sessions.pre_open_at, trading_sessions.open_at,
                    trading_sessions.close_at, trading_sessions.post_close_at)
                   IS DISTINCT FROM
                   (EXCLUDED.pre_open_at, EXCLUDED.open_at, EXCLUDED.close_at,
                    EXCLUDED.post_close_at)",
        )
        .bind(venue.uuid())
        .bind(s.date)
        .bind(s.pre_open_at.as_datetime())
        .bind(s.open_at.as_datetime())
        .bind(s.close_at.as_datetime())
        .bind(s.post_close_at.as_datetime())
        .bind(record.provenance.source_id.as_str())
        .bind(record.provenance.received_at.as_datetime())
        .bind(record.id.0)
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize;
    }
    sqlx::query(
        "INSERT INTO trading_calendar_ranges
           (venue_id, first_date, last_date, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT ON CONSTRAINT trading_calendar_ranges_record_key DO NOTHING",
    )
    .bind(venue.uuid())
    .bind(first_date)
    .bind(last_date)
    .bind(record.provenance.source_id.as_str())
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
    .execute(&mut *conn)
    .await?;
    Ok(written)
}

/// The number of stored bars of a market and interval (tests, reports).
pub async fn bar_count(
    conn: &mut PgConnection,
    subject: PriceSubject,
    interval: &str,
) -> Result<i64, StoreError> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM market_bars WHERE subject_id = $1 AND bar_interval = $2",
    )
    .bind(subject.canonical().uuid())
    .bind(interval)
    .fetch_one(conn)
    .await?)
}

/// Stores a corporate action from `record`. The same source action for the
/// same instrument and role is replaced only by a newer record with
/// different terms or dates (the source revised it).
pub async fn upsert_corporate_action(
    conn: &mut PgConnection,
    a: &CorporateAction,
    record: &RecordProvenance,
) -> Result<BarWrite, StoreError> {
    let row: Option<(bool,)> = sqlx::query_as(
        "INSERT INTO corporate_actions
           (instrument_id, source_id, source_action_id, action_type, role, ex_date, record_date,
            payable_date, effective_date, process_date, cash_amount, stock_rate, old_rate,
            new_rate, special, other_symbol, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::numeric, $12::numeric,
                 $13::numeric, $14::numeric, $15, $16, $17, $18)
         ON CONFLICT ON CONSTRAINT corporate_actions_one_per_instrument DO UPDATE SET
           action_type = EXCLUDED.action_type, ex_date = EXCLUDED.ex_date,
           record_date = EXCLUDED.record_date, payable_date = EXCLUDED.payable_date,
           effective_date = EXCLUDED.effective_date, process_date = EXCLUDED.process_date,
           cash_amount = EXCLUDED.cash_amount, stock_rate = EXCLUDED.stock_rate,
           old_rate = EXCLUDED.old_rate, new_rate = EXCLUDED.new_rate,
           special = EXCLUDED.special, other_symbol = EXCLUDED.other_symbol,
           received_at = EXCLUDED.received_at, source_record_id = EXCLUDED.source_record_id
         WHERE corporate_actions.received_at < EXCLUDED.received_at
           AND (corporate_actions.action_type, corporate_actions.ex_date,
                corporate_actions.record_date, corporate_actions.payable_date,
                corporate_actions.effective_date, corporate_actions.process_date,
                corporate_actions.cash_amount, corporate_actions.stock_rate,
                corporate_actions.old_rate, corporate_actions.new_rate,
                corporate_actions.special, corporate_actions.other_symbol)
               IS DISTINCT FROM
               (EXCLUDED.action_type, EXCLUDED.ex_date, EXCLUDED.record_date,
                EXCLUDED.payable_date, EXCLUDED.effective_date, EXCLUDED.process_date,
                EXCLUDED.cash_amount, EXCLUDED.stock_rate, EXCLUDED.old_rate,
                EXCLUDED.new_rate, EXCLUDED.special, EXCLUDED.other_symbol)
         RETURNING (xmax = 0)",
    )
    .bind(a.instrument.uuid())
    .bind(record.provenance.source_id.as_str())
    .bind(&a.source_action_id)
    .bind(a.action_type.as_str())
    .bind(a.role.as_str())
    .bind(a.ex_date)
    .bind(a.record_date)
    .bind(a.payable_date)
    .bind(a.effective_date)
    .bind(a.process_date)
    .bind(a.cash_amount.map(decimal_to_sql))
    .bind(a.stock_rate.map(decimal_to_sql))
    .bind(a.ratio.map(|(o, _)| decimal_to_sql(o)))
    .bind(a.ratio.map(|(_, n)| decimal_to_sql(n)))
    .bind(a.special)
    .bind(a.other_symbol.as_deref())
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
    .fetch_optional(conn)
    .await?;
    Ok(match row {
        Some((true,)) => BarWrite::Inserted,
        Some((false,)) => BarWrite::Replaced,
        None => BarWrite::Unchanged,
    })
}

/// Stores one release-calendar fetch from `record`: the window it covered
/// for `release_key`, and the dates it stated (a snapshot; replaying the
/// record changes nothing).
pub async fn store_economic_release_dates(
    conn: &mut PgConnection,
    record: &RecordProvenance,
    release_key: &str,
    release_name: &str,
    category: &str,
    window: (NaiveDate, NaiveDate),
    dates: &[NaiveDate],
) -> Result<usize, StoreError> {
    sqlx::query(
        "INSERT INTO economic_calendar_windows
           (source_record_id, source_id, received_at, release_key, category, window_from, window_to)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (source_record_id) DO NOTHING",
    )
    .bind(record.id.0)
    .bind(record.provenance.source_id.as_str())
    .bind(record.provenance.received_at.as_datetime())
    .bind(release_key)
    .bind(category)
    .bind(window.0)
    .bind(window.1)
    .execute(&mut *conn)
    .await?;
    let mut written = 0;
    for d in dates {
        written += sqlx::query(
            "INSERT INTO economic_release_dates (source_record_id, release_key, release_name, release_date)
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(record.id.0)
        .bind(release_key)
        .bind(release_name)
        .bind(d)
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize;
    }
    Ok(written)
}

/// Stores an earnings report from `record`; a newer record replaces a report
/// of the same fiscal quarter whose date, time or figures changed.
pub async fn upsert_earnings(
    conn: &mut PgConnection,
    e: &EarningsReport,
    record: &RecordProvenance,
) -> Result<BarWrite, StoreError> {
    let row: Option<(bool,)> = sqlx::query_as(
        "INSERT INTO earnings_events
           (instrument_id, source_id, fiscal_year, fiscal_quarter, report_date, report_time,
            eps_estimate, eps_actual, revenue_estimate, revenue_actual, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7::numeric, $8::numeric, $9::numeric, $10::numeric, $11, $12)
         ON CONFLICT ON CONSTRAINT earnings_events_one_per_quarter DO UPDATE SET
           report_date = EXCLUDED.report_date, report_time = EXCLUDED.report_time,
           eps_estimate = EXCLUDED.eps_estimate, eps_actual = EXCLUDED.eps_actual,
           revenue_estimate = EXCLUDED.revenue_estimate, revenue_actual = EXCLUDED.revenue_actual,
           received_at = EXCLUDED.received_at, source_record_id = EXCLUDED.source_record_id
         WHERE earnings_events.received_at < EXCLUDED.received_at
           AND (earnings_events.report_date, earnings_events.report_time,
                earnings_events.eps_estimate, earnings_events.eps_actual,
                earnings_events.revenue_estimate, earnings_events.revenue_actual)
               IS DISTINCT FROM
               (EXCLUDED.report_date, EXCLUDED.report_time, EXCLUDED.eps_estimate,
                EXCLUDED.eps_actual, EXCLUDED.revenue_estimate, EXCLUDED.revenue_actual)
         RETURNING (xmax = 0)",
    )
    .bind(e.instrument.uuid())
    .bind(record.provenance.source_id.as_str())
    .bind(e.fiscal_year)
    .bind(i32::try_from(e.fiscal_quarter).unwrap_or(0))
    .bind(e.date)
    .bind(e.time.map(|t| t.as_str()))
    .bind(e.eps_estimate.map(decimal_to_sql))
    .bind(e.eps_actual.map(decimal_to_sql))
    .bind(e.revenue_estimate.map(decimal_to_sql))
    .bind(e.revenue_actual.map(decimal_to_sql))
    .bind(record.provenance.received_at.as_datetime())
    .bind(record.id.0)
    .fetch_optional(conn)
    .await?;
    Ok(match row {
        Some((true,)) => BarWrite::Inserted,
        Some((false,)) => BarWrite::Replaced,
        None => BarWrite::Unchanged,
    })
}
