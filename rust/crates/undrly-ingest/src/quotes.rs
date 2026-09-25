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
    VenueSymbol, select_latest,
};
use undrly_normalize::QuoteNormalizer;
use undrly_provider::QuoteProvider;
use undrly_store::Write;
use undrly_store::market::{
    ObservationId, QuoteFeedId, StoredCanonicalQuote, insert_observation, latest_observations,
    quote_feeds_of_source, upsert_canonical_quote,
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
    let decoded = provider.decode_quote(&raw.payload)?;

    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let feeds = quote_feeds_of_source(&mut tx, provider.source_id()).await?;
    let symbols: Vec<VenueSymbol> = feeds.iter().map(|f| f.feed.symbol.clone()).collect();
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
    /// The selected observation; `None` when there is nothing to select.
    pub selected: Option<ObservationId>,
    pub eligible_count: usize,
    pub write: Option<Write>,
}

/// Recomputes the canonical quote of each pair with
/// [`AggregationMethod::LatestObservationV1`] and stores it. Each pair is its
/// own transaction; the canonical table is a derived cache.
pub async fn refresh_canonical_quotes(
    conn: &mut PgConnection,
    pairs: &[(PriceSubject, PriceUnit)],
    computed_at: Timestamp,
) -> Result<Vec<CanonicalRefresh>, IngestError> {
    let mut out = Vec::new();
    for &(subject, unit) in pairs {
        let mut tx = conn.begin().await?;
        let candidates = latest_observations(&mut tx, subject, unit).await?;
        let selected = select_latest(&candidates).map(|(id, _)| *id);
        let write = match selected {
            Some(observation) => Some(
                upsert_canonical_quote(
                    &mut tx,
                    &StoredCanonicalQuote {
                        subject,
                        unit,
                        observation,
                        method: AggregationMethod::LatestObservationV1,
                        eligible_count: i32::try_from(candidates.len()).unwrap_or(i32::MAX),
                        computed_at,
                    },
                )
                .await?,
            ),
            None => None,
        };
        tx.commit().await?;
        out.push(CanonicalRefresh {
            subject,
            unit,
            selected,
            eligible_count: candidates.len(),
            write,
        });
    }
    Ok(out)
}
