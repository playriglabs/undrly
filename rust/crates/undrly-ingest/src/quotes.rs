//! Quote ingestion and aggregation:
//!
//! ```text
//! raw response → decode (provider) → [one transaction: store raw record →
//!   load the source's quote feeds → normalize the feeds' symbols →
//!   one observation per matched feed]
//! then, separately: aggregate each touched (subject, unit) → canonical quote
//! ```
//!
//! A payload that fails to decode writes nothing. A normalization error rolls
//! back the whole record, raw record included. A feed whose symbol is absent
//! from the response is reported, not an error. Symbols without a feed
//! (Hyperliquid returns every market) are ignored.

use sqlx::{Acquire, PgConnection};
use undrly_core::{
    AggregationMethod, MarketObservation, ObservationError, PriceSubject, PriceUnit, Timestamp,
    VenueSymbol, aggregate,
};
use undrly_normalize::QuoteNormalizer;
use undrly_provider::QuoteProvider;
use undrly_store::Write;
use undrly_store::market::{
    ObservationId, QuoteFeedId, StoredCanonicalQuote, aggregation_method_of,
    delete_canonical_quote, insert_observation, latest_observations, quote_feeds_of_source,
    upsert_canonical_quote,
};
use undrly_store::sources::SourceRecordId;

use crate::{IngestError, RawRecord, store_raw_record};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteIngestReport {
    pub source_record: (SourceRecordId, Write),
    pub observations: Vec<(QuoteFeedId, ObservationId, Write)>,
    /// Feeds whose symbol the response did not contain.
    pub missing: Vec<VenueSymbol>,
    /// The (subject, unit) pairs observed: the aggregation work list.
    pub pairs: Vec<(PriceSubject, PriceUnit)>,
}

impl From<ObservationError> for IngestError {
    fn from(err: ObservationError) -> Self {
        IngestError::Observation(err)
    }
}

/// Ingests one quote response atomically. See the module docs.
pub async fn ingest_quotes<P, N>(
    conn: &mut PgConnection,
    provider: &P,
    normalizer: &N,
    raw: &RawRecord,
) -> Result<QuoteIngestReport, IngestError>
where
    P: QuoteProvider,
    N: QuoteNormalizer<Quote = P::Quote>,
{
    ingest_quotes_for(conn, provider, normalizer, raw, None).await
}

/// [`ingest_quotes`] for a record that covers only the `requested` symbols
/// (one Coinbase product, one Alpaca batch): the source's other feeds are
/// neither matched nor reported missing. `None` means every feed.
pub async fn ingest_quotes_for<P, N>(
    conn: &mut PgConnection,
    provider: &P,
    normalizer: &N,
    raw: &RawRecord,
    requested: Option<&[VenueSymbol]>,
) -> Result<QuoteIngestReport, IngestError>
where
    P: QuoteProvider,
    N: QuoteNormalizer<Quote = P::Quote>,
{
    let decoded = provider.decode_quote(&raw.payload)?;

    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let mut feeds = quote_feeds_of_source(&mut tx, provider.source_id()).await?;
    if let Some(requested) = requested {
        feeds.retain(|f| requested.contains(&f.feed.symbol));
    }
    let mut symbols: Vec<VenueSymbol> = Vec::new();
    for f in &feeds {
        if !symbols.contains(&f.feed.symbol) {
            symbols.push(f.feed.symbol.clone());
        }
    }
    let quotes = normalizer.normalize_quotes(&decoded, &symbols)?;

    let mut report = QuoteIngestReport {
        source_record: (record.id, write),
        observations: Vec::new(),
        missing: Vec::new(),
        pairs: Vec::new(),
    };
    for stored in &feeds {
        let feed = &stored.feed;
        let Some(q) = quotes
            .iter()
            .find(|q| q.symbol == feed.symbol && q.price_type == feed.price_type)
        else {
            report.missing.push(feed.symbol.clone());
            continue;
        };
        let observation = MarketObservation::new(
            feed.subject,
            feed.basis,
            q.price_type,
            q.price,
            q.bid_ask,
            feed.unit,
            record.provenance.source_id.clone(),
            q.observed_at,
            record.provenance.received_at,
        )?;
        let (id, write) = insert_observation(&mut tx, &observation, record.id).await?;
        report.observations.push((stored.id, id, write));
        if !report.pairs.contains(&(feed.subject, feed.unit)) {
            report.pairs.push((feed.subject, feed.unit));
        }
    }
    tx.commit().await?;
    Ok(report)
}

/// The outcome of aggregating one (subject, unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRefresh {
    pub subject: PriceSubject,
    pub unit: PriceUnit,
    pub method: AggregationMethod,
    /// Exactly the observations used; empty when nothing was eligible.
    pub inputs: Vec<ObservationId>,
    /// `Some` when a canonical quote was written or found unchanged; `None`
    /// when nothing was eligible (any existing canonical quote was removed).
    pub write: Option<Write>,
}

/// Recomputes the canonical quote of each pair at `computed_at` with the
/// pair's declared aggregation method and stores it, or removes it when no
/// observation is eligible. Each pair is its own transaction; the canonical
/// table is a derived cache. Observations are never modified.
pub async fn refresh_canonical_quotes(
    conn: &mut PgConnection,
    pairs: &[(PriceSubject, PriceUnit)],
    computed_at: Timestamp,
) -> Result<Vec<CanonicalRefresh>, IngestError> {
    let mut out = Vec::new();
    for &(subject, unit) in pairs {
        let mut tx = conn.begin().await?;
        let method = aggregation_method_of(&mut tx, subject, unit).await?;
        let candidates = latest_observations(&mut tx, subject, unit).await?;
        let (inputs, write) = match aggregate(method, &candidates, computed_at) {
            Some(a) => {
                let quote = StoredCanonicalQuote {
                    subject,
                    unit,
                    method,
                    price: a.price,
                    price_type: a.price_type,
                    basis: a.basis,
                    bid_ask: a.bid_ask,
                    as_of: a.as_of,
                    computed_at,
                    inputs: a.inputs,
                };
                let write = upsert_canonical_quote(&mut tx, &quote).await?;
                (
                    quote.inputs.iter().map(|(id, _)| *id).collect(),
                    Some(write),
                )
            }
            None => {
                delete_canonical_quote(&mut tx, subject, unit).await?;
                (Vec::new(), None)
            }
        };
        tx.commit().await?;
        out.push(CanonicalRefresh {
            subject,
            unit,
            method,
            inputs,
            write,
        });
    }
    Ok(out)
}
