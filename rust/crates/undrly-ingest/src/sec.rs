//! SEC EDGAR: fetch one filer's submissions document and ingest it as an
//! entity record.
//!
//! ```text
//! SecClient::fetch_submissions (network, provider crate)
//!   → RawRecord (exact bytes, URL as record key, receipt time)
//!   → ingest_entity: [one transaction: raw record → entity by CIK/LEI → identifiers]
//! ```
//!
//! A failed fetch returns before any database access, so it writes nothing.
//! Upstream request metadata (`x-amzn-requestid`, `Date`) is returned in
//! [`FetchedRecord`] but not persisted: `source_records` has no column for it.

use sqlx::PgConnection;
use undrly_core::{Cik, ExternalIdentifier};
use undrly_normalize::EntityNormalizer;
use undrly_normalize::sec::SecNormalizer;
use undrly_provider::ReferenceDataProvider;
use undrly_provider::sec::SecProvider;
use undrly_provider::sec::http::{FetchedRecord, SecClient};

use crate::{EntityIngestReport, IngestError, RawRecord, ingest_entity};

/// A fetched response as a raw record: the requested URL is the record key.
pub fn raw_record(fetched: &FetchedRecord) -> RawRecord {
    RawRecord {
        record_key: fetched.record_key.clone(),
        payload: fetched.body.clone(),
        received_at: fetched.received_at,
    }
}

/// Fetches the submissions document for `cik` and ingests it. The record must
/// be about the requested CIK; otherwise nothing is written.
pub async fn fetch_and_ingest_company(
    conn: &mut PgConnection,
    client: &SecClient,
    cik: &Cik,
) -> Result<(FetchedRecord, EntityIngestReport), IngestError> {
    let fetched = client.fetch_submissions(cik).await?;
    let raw = raw_record(&fetched);
    let report = ingest_company(conn, cik, &raw).await?;
    Ok((fetched, report))
}

/// Ingests an already fetched submissions document for `cik`.
pub async fn ingest_company(
    conn: &mut PgConnection,
    cik: &Cik,
    raw: &RawRecord,
) -> Result<EntityIngestReport, IngestError> {
    let provider = SecProvider::new();
    let requested = ExternalIdentifier::Cik(cik.clone());
    let identifiers = SecNormalizer
        .normalize_entity(&provider.decode_reference(&raw.payload)?)?
        .identifiers;
    if !identifiers.contains(&requested) {
        return Err(IngestError::UnexpectedRecord {
            requested,
            found: identifiers,
        });
    }
    ingest_entity(conn, &provider, &SecNormalizer, raw).await
}
