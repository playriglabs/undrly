//! V1.3 ingestion (docs/v1.3-market-data.md): venue bars, perpetual
//! contexts and trading calendars.
//!
//! As for quotes: each response is stored raw first, then normalized into
//! rows that name it, in one transaction. What a source symbol prices comes
//! from the source's declared **venue** quote feeds (subject, unit, venue);
//! a symbol without one is ignored, never guessed.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    Bar, CorporateAction, EarningsReport, InstrumentId, ObservationBasis, PerpContext,
    PriceSubject, PriceUnit, TradingSession, VenueId, VenueSymbol,
};
use undrly_normalize::NormalizeError;
use undrly_normalize::market_data::{
    BarNormalizer, corporate_actions, earnings, economic_release_dates, perp_contexts,
    trading_sessions,
};
use undrly_provider::alpaca::{decode_calendar, decode_corporate_actions};
use undrly_provider::finnhub::FinnhubProvider;
use undrly_provider::fred::FredProvider;
use undrly_provider::hyperliquid::HyperliquidProvider;
use undrly_provider::{BarsProvider, Provider, QuoteProvider};
use undrly_store::market::quote_feeds_of_source;
use undrly_store::market_data::{
    BarWrite, insert_perp_context, store_economic_release_dates, store_trading_calendar,
    upsert_bar, upsert_corporate_action, upsert_earnings,
};
use undrly_store::sources::RecordProvenance;
use undrly_store::{StoreError, Write};

use crate::{IngestError, RawRecord, store_raw_record};

/// What a symbol prices: (subject, unit, venue), from the source's venue feeds.
async fn venue_markets(
    conn: &mut PgConnection,
    source: &undrly_core::SourceId,
    requested: &[VenueSymbol],
) -> Result<BTreeMap<VenueSymbol, (PriceSubject, PriceUnit, VenueId)>, IngestError> {
    let mut out = BTreeMap::new();
    for f in quote_feeds_of_source(conn, source).await? {
        let f = f.feed;
        if !requested.contains(&f.symbol) {
            continue;
        }
        if let ObservationBasis::Venue(venue) = f.basis {
            let market = (f.subject, f.unit, venue);
            if let Some(existing) = out.insert(f.symbol.clone(), market)
                && existing != market
            {
                return Err(IngestError::Normalize(NormalizeError::Invalid {
                    field: "symbol",
                    reason: format!("{}: feeds for two markets", f.symbol),
                }));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarIngestReport {
    pub source_record: Option<(undrly_store::sources::SourceRecordId, Write)>,
    pub inserted: usize,
    pub replaced: usize,
    pub unchanged: usize,
    /// Requested symbols with no venue feed (not stored).
    pub unmapped: Vec<VenueSymbol>,
}

/// Ingests one bars response for the `requested` symbols, atomically.
pub async fn ingest_bars<P, N>(
    conn: &mut PgConnection,
    provider: &P,
    normalizer: &N,
    raw: &RawRecord,
    requested: &[VenueSymbol],
) -> Result<BarIngestReport, IngestError>
where
    P: BarsProvider,
    N: BarNormalizer<Bars = P::Bars>,
{
    let decoded = provider.decode_bars(&raw.payload)?;
    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let markets = venue_markets(&mut tx, provider.source_id(), requested).await?;
    let mut report = BarIngestReport {
        source_record: Some((record.id, write)),
        unmapped: requested
            .iter()
            .filter(|s| !markets.contains_key(*s))
            .cloned()
            .collect(),
        ..Default::default()
    };
    let symbols: Vec<VenueSymbol> = markets.keys().cloned().collect();
    for b in normalizer.normalize_bars(&decoded, &symbols)? {
        let (subject, unit, venue) = markets[&b.symbol];
        let bar = Bar {
            subject,
            unit,
            venue,
            interval: b.interval,
            open_time: b.open_time,
            close_time: b.close_time,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
            trade_count: b.trade_count,
        };
        bar.validate().map_err(|e| {
            IngestError::Normalize(NormalizeError::Invalid {
                field: "bar",
                reason: e.to_string(),
            })
        })?;
        match upsert_bar(&mut tx, &bar, &record).await? {
            BarWrite::Inserted => report.inserted += 1,
            BarWrite::Replaced => report.replaced += 1,
            BarWrite::Unchanged => report.unchanged += 1,
        }
    }
    tx.commit().await?;
    Ok(report)
}

/// The instrument a feed prices (perpetuals, equities).
fn perpetual(subject: PriceSubject) -> Result<InstrumentId, IngestError> {
    match subject {
        PriceSubject::Instrument(id) => Ok(id),
        PriceSubject::Currency(_) => Err(IngestError::Store(StoreError::Corrupt {
            what: "perpetual feed",
            detail: "a perpetual feed prices a currency".into(),
        })),
    }
}

/// Ingests perpetual contexts from a Hyperliquid `metaAndAssetCtxs` record
/// (the same record the mark-price quotes come from; replaying it is a no-op).
pub async fn ingest_perp_contexts(
    conn: &mut PgConnection,
    raw: &RawRecord,
    requested: &[VenueSymbol],
) -> Result<usize, IngestError> {
    let provider = HyperliquidProvider::new();
    let decoded = provider.decode_quote(&raw.payload)?;
    let mut tx = conn.begin().await?;
    let (record, _) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let markets = venue_markets(&mut tx, provider.source_id(), requested).await?;
    let symbols: Vec<VenueSymbol> = markets.keys().cloned().collect();
    let mut written = 0;
    for c in perp_contexts(&decoded, &symbols)? {
        let (subject, unit, venue) = markets[&c.symbol];
        let context = PerpContext {
            perpetual: perpetual(subject)?,
            unit,
            venue,
            mark_price: c.mark_price,
            oracle_price: c.oracle_price,
            mid_price: c.mid_price,
            funding_rate: c.funding_rate_hourly.map(|r| (r, 1)),
            open_interest: c.open_interest,
            volume_24h_base: c.volume_24h_base,
            volume_24h_notional: c.volume_24h_notional,
            price_24h_ago: c.price_24h_ago,
        };
        if insert_perp_context(&mut tx, &context, &record).await? == Write::Inserted {
            written += 1;
        }
    }
    tx.commit().await?;
    Ok(written)
}

/// Ingests Alpaca's US equity calendar for `venue`, covering
/// `first..=last` (the request's range).
pub async fn ingest_calendar(
    conn: &mut PgConnection,
    source: &undrly_core::SourceId,
    raw: &RawRecord,
    venue: VenueId,
    first: NaiveDate,
    last: NaiveDate,
) -> Result<usize, IngestError> {
    let days = decode_calendar(&raw.payload)?;
    let sessions: Vec<TradingSession> = trading_sessions(&days)?
        .into_iter()
        .map(|s| TradingSession {
            venue,
            date: s.date,
            pre_open_at: s.pre_open_at,
            open_at: s.open_at,
            close_at: s.close_at,
            post_close_at: s.post_close_at,
        })
        .collect();
    let mut tx = conn.begin().await?;
    let (record, _): (RecordProvenance, _) = store_raw_record(&mut tx, source, raw).await?;
    let n = store_trading_calendar(&mut tx, venue, first, last, &sessions, &record).await?;
    tx.commit().await?;
    Ok(n)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorporateActionReport {
    pub inserted: usize,
    pub replaced: usize,
    pub unchanged: usize,
    /// Actions of types Undrly does not model (not stored).
    pub skipped: usize,
}

/// Ingests one Alpaca corporate-actions page for the `requested` symbols
/// (equities with an Alpaca venue feed), atomically.
pub async fn ingest_corporate_actions(
    conn: &mut PgConnection,
    raw: &RawRecord,
    requested: &[VenueSymbol],
) -> Result<CorporateActionReport, IngestError> {
    let page = decode_corporate_actions(&raw.payload)?;
    let source =
        undrly_core::SourceId::parse(undrly_provider::alpaca::SOURCE_ID).expect("valid source id");
    let mut tx = conn.begin().await?;
    let (record, _) = store_raw_record(&mut tx, &source, raw).await?;
    let markets = venue_markets(&mut tx, &source, requested).await?;
    let symbols: Vec<VenueSymbol> = markets.keys().cloned().collect();
    let (actions, skipped) = corporate_actions(&page, &symbols)?;
    let mut report = CorporateActionReport {
        skipped,
        ..Default::default()
    };
    for a in actions {
        let (subject, _, _) = markets[&a.symbol];
        let action = CorporateAction {
            instrument: perpetual(subject)?,
            source_action_id: a.source_action_id,
            action_type: a.action_type,
            role: a.role,
            ex_date: a.ex_date,
            record_date: a.record_date,
            payable_date: a.payable_date,
            effective_date: a.effective_date,
            process_date: a.process_date,
            cash_amount: a.cash_amount,
            stock_rate: a.stock_rate,
            ratio: a.ratio,
            special: a.special,
            other_symbol: a.other_symbol,
        };
        match upsert_corporate_action(&mut tx, &action, &record).await? {
            BarWrite::Inserted => report.inserted += 1,
            BarWrite::Replaced => report.replaced += 1,
            BarWrite::Unchanged => report.unchanged += 1,
        }
    }
    tx.commit().await?;
    Ok(report)
}

/// Ingests one FRED release-dates response for `release_key` covering
/// `window`, atomically.
pub async fn ingest_economic_release_dates(
    conn: &mut PgConnection,
    raw: &RawRecord,
    release_key: &str,
    release_name: &str,
    category: &str,
    window: (NaiveDate, NaiveDate),
) -> Result<usize, IngestError> {
    let decoded = FredProvider::new().decode_quote(&raw.payload)?;
    let dates = economic_release_dates(&decoded, release_key)?;
    let mut tx = conn.begin().await?;
    let (record, _) = store_raw_record(&mut tx, FredProvider::new().source_id(), raw).await?;
    let n = store_economic_release_dates(
        &mut tx,
        &record,
        release_key,
        release_name,
        category,
        window,
        &dates,
    )
    .await?;
    tx.commit().await?;
    Ok(n)
}

/// Ingests one Finnhub earnings-calendar response for the equities with an
/// Alpaca venue feed among `requested`, atomically.
pub async fn ingest_earnings(
    conn: &mut PgConnection,
    raw: &RawRecord,
    requested: &[VenueSymbol],
) -> Result<CorporateActionReport, IngestError> {
    let decoded = FinnhubProvider::new().decode_quote(&raw.payload)?;
    let alpaca_source =
        undrly_core::SourceId::parse(undrly_provider::alpaca::SOURCE_ID).expect("valid source id");
    let mut tx = conn.begin().await?;
    let (record, _) = store_raw_record(&mut tx, FinnhubProvider::new().source_id(), raw).await?;
    // What a ticker is: the equity whose IEX feed Undrly declares under it.
    let markets = venue_markets(&mut tx, &alpaca_source, requested).await?;
    let symbols: Vec<VenueSymbol> = markets.keys().cloned().collect();
    let mut report = CorporateActionReport::default();
    for e in earnings(&decoded, &symbols)? {
        let (subject, _, _) = markets[&e.symbol];
        let r = EarningsReport {
            instrument: perpetual(subject)?,
            fiscal_year: e.fiscal_year,
            fiscal_quarter: e.fiscal_quarter,
            date: e.date,
            time: e.time,
            eps_estimate: e.eps_estimate,
            eps_actual: e.eps_actual,
            revenue_estimate: e.revenue_estimate,
            revenue_actual: e.revenue_actual,
        };
        match upsert_earnings(&mut tx, &r, &record).await? {
            BarWrite::Inserted => report.inserted += 1,
            BarWrite::Replaced => report.replaced += 1,
            BarWrite::Unchanged => report.unchanged += 1,
        }
    }
    tx.commit().await?;
    Ok(report)
}
