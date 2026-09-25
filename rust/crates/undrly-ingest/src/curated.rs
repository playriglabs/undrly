//! Curated reference data: one raw record (the dataset file) → every object,
//! identifier, listing, edge, alias, and quote feed it declares, in one
//! transaction.
//!
//! Objects carry pinned canonical ids. Re-ingesting the same file is a replay
//! (nothing changes); an existing object with different content, or an
//! identifier already mapped elsewhere, fails the whole record: curated data
//! must agree with what is stored.

use sqlx::{Acquire, PgConnection};
use undrly_core::{
    Alias, CanonicalId, ExternalIdentifier, IdentifierAssignment, Listing, ListingSymbol,
    QuoteAggregation, QuoteFeed, Relationship, UniverseSnapshot, Validity,
};
use undrly_normalize::curated::normalize_universe;
use undrly_provider::curated::CuratedProvider;
use undrly_provider::{Provider, ReferenceDataProvider};
use undrly_store::identifiers::{AssignOutcome, assign_identifier};
use undrly_store::listing_symbols::{SymbolAssignOutcome, assign_listing_symbol};
use undrly_store::sources::{RecordProvenance, SourceRecordId};
use undrly_store::{Write, aliases, graph, market, reference, sources, universe};

use crate::{IngestError, RawRecord, store_raw_record};

/// What a curated ingestion wrote (`Inserted`) or found unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UniverseIngestReport {
    pub source_record: Option<(SourceRecordId, Write)>,
    pub inserted: usize,
    pub unchanged: usize,
}

impl UniverseIngestReport {
    fn count(&mut self, write: Write) {
        match write {
            Write::Inserted => self.inserted += 1,
            Write::Unchanged => self.unchanged += 1,
        }
    }
}

pub async fn ingest_universe(
    conn: &mut PgConnection,
    raw: &RawRecord,
) -> Result<UniverseIngestReport, IngestError> {
    ingest_universe_as(conn, CuratedProvider::new(), raw).await
}

/// [`ingest_universe`] for a file in the curated format published under
/// another source (the generated V1.1 snapshot uses `undrly-universe`).
pub async fn ingest_universe_as(
    conn: &mut PgConnection,
    provider: CuratedProvider,
    raw: &RawRecord,
) -> Result<UniverseIngestReport, IngestError> {
    let u = normalize_universe(&provider.decode_reference(&raw.payload)?)?;

    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let mut report = UniverseIngestReport {
        source_record: Some((record.id, write)),
        ..Default::default()
    };
    let r = record.id;

    for (currency, code) in &u.currencies {
        report.count(reference::insert_currency(&mut tx, currency, r).await?);
        assign(
            &mut tx,
            ExternalIdentifier::Iso4217(code.clone()),
            currency.id.into(),
            &record,
            &mut report,
        )
        .await?;
    }
    for (entity, lei, cik) in &u.entities {
        report.count(reference::insert_entity(&mut tx, entity, r).await?);
        if let Some(cik) = cik {
            assign(
                &mut tx,
                ExternalIdentifier::Cik(cik.clone()),
                entity.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
        if let Some(lei) = lei {
            assign(
                &mut tx,
                ExternalIdentifier::Lei(lei.clone()),
                entity.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
    }
    for (venue, mic) in &u.venues {
        report.count(reference::insert_venue(&mut tx, venue, r).await?);
        if let Some(mic) = mic {
            assign(
                &mut tx,
                ExternalIdentifier::Mic(mic.clone()),
                venue.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
    }
    for (instrument, isin, figi) in &u.instruments {
        report.count(reference::insert_instrument(&mut tx, instrument, r).await?);
        if let Some(isin) = isin {
            assign(
                &mut tx,
                ExternalIdentifier::Isin(isin.clone()),
                instrument.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
        if let Some(figi) = figi {
            assign(
                &mut tx,
                ExternalIdentifier::Figi(figi.clone()),
                instrument.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
    }
    for l in &u.listings {
        let listing = Listing {
            id: l.id,
            instrument_id: l.instrument,
            venue_id: l.venue,
            provenance: record.provenance.clone(),
        };
        report.count(reference::insert_listing(&mut tx, &listing, r).await?);
        if let Some(figi) = &l.figi {
            assign(
                &mut tx,
                ExternalIdentifier::Figi(figi.clone()),
                l.id.into(),
                &record,
                &mut report,
            )
            .await?;
        }
        let symbol = ListingSymbol {
            listing_id: l.id,
            venue_id: l.venue,
            symbol: l.symbol.clone(),
            valid_during: Validity::UNBOUNDED,
            provenance: record.provenance.clone(),
        };
        match assign_listing_symbol(&mut tx, &symbol, r).await? {
            SymbolAssignOutcome::Assigned(_) => report.count(Write::Inserted),
            SymbolAssignOutcome::Unchanged(_) => report.count(Write::Unchanged),
            other => {
                return Err(IngestError::CuratedDisagrees(format!(
                    "listing symbol {}: {other:?}",
                    l.symbol
                )));
            }
        }
    }
    for (subject, kind, object) in &u.relationships {
        let edge = Relationship::new(*subject, *kind, *object, record.provenance.clone())?;
        report.count(graph::insert_relationship(&mut tx, &edge, r).await?);
    }
    for (node, text, kind) in &u.aliases {
        let alias = Alias {
            node: *node,
            text: text.clone(),
            kind: *kind,
            provenance: record.provenance.clone(),
        };
        report.count(aliases::insert_alias(&mut tx, &alias, r).await?);
    }
    for f in &u.quote_feeds {
        let feed = QuoteFeed {
            feed_source: f.feed_source.clone(),
            symbol: f.symbol.clone(),
            subject: f.subject,
            unit: f.unit,
            basis: f.basis,
            price_type: f.price_type,
            stale_after_seconds: f.stale_after_seconds,
            provenance: record.provenance.clone(),
        };
        report.count(market::insert_quote_feed(&mut tx, &feed, r).await?.1);
    }
    for (subject, unit, method) in &u.quote_aggregations {
        let declared = QuoteAggregation {
            subject: *subject,
            unit: *unit,
            method: *method,
            provenance: record.provenance.clone(),
        };
        report.count(market::insert_quote_aggregation(&mut tx, &declared, r).await?);
    }
    // Memberships are asserted by the upstream universe file, stored earlier
    // as its own source record; the snapshot only references it.
    for x in &u.universes {
        let upstream = sources::find_source_record(&mut tx, &x.source, &x.record_key, &x.sha256)
            .await?
            .ok_or_else(|| {
                IngestError::CuratedDisagrees(format!(
                    "universe {}: raw record `{}` of `{}` is not stored",
                    x.key.as_str(),
                    x.record_key,
                    x.source
                ))
            })?;
        let snapshot = UniverseSnapshot {
            key: x.key,
            as_of: x.as_of,
            members: x.members.clone(),
            provenance: upstream.provenance.clone(),
        };
        report.count(
            universe::insert_universe_snapshot(&mut tx, &snapshot, upstream.id)
                .await?
                .1,
        );
    }
    tx.commit().await?;
    Ok(report)
}

async fn assign(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    node: CanonicalId,
    record: &RecordProvenance,
    report: &mut UniverseIngestReport,
) -> Result<(), IngestError> {
    let claim = IdentifierAssignment::new(
        identifier.clone(),
        node,
        Validity::UNBOUNDED,
        record.provenance.clone(),
    )?;
    match assign_identifier(tx, &claim, record.id).await? {
        AssignOutcome::Assigned(_) => report.count(Write::Inserted),
        AssignOutcome::Unchanged(_) => report.count(Write::Unchanged),
        other => {
            return Err(IngestError::CuratedDisagrees(format!(
                "{} {}: {other:?}",
                identifier.namespace(),
                identifier.value()
            )));
        }
    }
    Ok(())
}
