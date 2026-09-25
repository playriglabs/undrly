//! Reference-data ingestion:
//!
//! ```text
//! raw payload → decode (provider) → normalize → [one transaction:
//!   store raw record → resolve identities → create missing nodes →
//!   assign identifiers (quarantine conflicts) → assert relationships]
//! ```
//!
//! Two record shapes are supported: [`ingest_reference`] for a security with
//! its issuer, currency and listing, and [`ingest_entity`] for an entity on
//! its own (a source authoritative for identity only, such as SEC EDGAR;
//! see [`sec`]).
//!
//! Decode and normalize are pure and run before any write, so a malformed
//! record writes nothing. Everything else is one transaction: any error rolls
//! back every write of the record, leaving no partial canonical state.
//! Quarantined conflicts are an outcome, not an error, and are committed.
//!
//! # Provenance
//!
//! Every fact written (node objects, listing, identifier and symbol
//! assignments, edges, quarantined claims) names the stored raw record as its
//! `source_record_id`, and carries that record's source and stored receipt
//! time. A replay of a stored record therefore reuses its original receipt
//! time. A fact that already exists is left as is: a second record asserting
//! it is not recorded as supporting evidence (corroboration is deferred to
//! reconciliation).
//!
//! # Identity resolution
//!
//! Each object is found by its **primary identifiers** only: entity by LEI or
//! SEC CIK, instrument by ISIN, venue by MIC, currency by ISO 4217 code, and a
//! listing by (instrument, venue). No names, symbols, or fuzzy matching are
//! used. If the record's primary identifiers map to no node, a new canonical
//! id is minted; if they map to more than one, the record is rejected as
//! ambiguous. Every primary identifier in the record is then assigned to the
//! resolved node (an entity record with both an LEI and a CIK links them); a
//! conflict on one rejects the record. Secondary
//! identifiers (FIGIs) and the venue symbol are claims assigned to the resolved
//! node; a conflicting claim is quarantined and never changes which node the
//! record resolved to.
//!
//! Existing nodes are not modified: if a source's display name differs from
//! the stored one, the stored one is kept. Reconciling such differences is
//! reconciliation's job, not ingestion's.

pub mod sec;

use sqlx::{Acquire, PgConnection};
use undrly_core::identifier::AssignmentError;
use undrly_core::{
    CanonicalId, Currency, CurrencyId, DisplayName, Entity, EntityId, EntityKind,
    ExternalIdentifier, IdentifierAssignment, Instrument, InstrumentId, Listing, ListingId,
    ListingSymbol, Namespace, Relationship, RelationshipError, RelationshipType, SourceId,
    Timestamp, Validity, Venue, VenueId,
};
use undrly_normalize::{
    EntityNormalizer, NormalizeError, NormalizedEntityRecord, NormalizedReference,
    ReferenceNormalizer,
};
use undrly_provider::{DecodeError, ReferenceDataProvider};
use undrly_store::conflicts::ConflictId;
use undrly_store::identifiers::{AssignOutcome, assign_identifier, identifier_history};
use undrly_store::listing_symbols::{SymbolAssignOutcome, assign_listing_symbol};
use undrly_store::sources::{
    RecordProvenance, SourceRecord, SourceRecordId, get_source, insert_source_record,
};
use undrly_store::{StoreError, Write, graph, reference};

/// A payload as received from a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub record_key: String,
    pub payload: Vec<u8>,
    /// When Undrly received the payload. Becomes `received_at` provenance on
    /// every fact written from this record.
    pub received_at: Timestamp,
}

/// Whether ingestion found an existing canonical node or minted a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution<T> {
    Existing(T),
    Created(T),
}

impl<T: Copy> Resolution<T> {
    pub fn id(&self) -> T {
        match self {
            Resolution::Existing(id) | Resolution::Created(id) => *id,
        }
    }
}

/// Everything one ingestion did, including quarantined conflicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestReport {
    pub source_record: (SourceRecordId, Write),
    pub entity: Resolution<EntityId>,
    pub instrument: Resolution<InstrumentId>,
    pub currency: Resolution<CurrencyId>,
    pub venue: Resolution<VenueId>,
    pub listing: Resolution<ListingId>,
    pub identifiers: Vec<(ExternalIdentifier, AssignOutcome)>,
    pub listing_symbol: SymbolAssignOutcome,
    pub relationships: Vec<(RelationshipType, Write)>,
}

impl IngestReport {
    /// Conflict rows recorded (or found, on replay) by this ingestion.
    pub fn quarantined(&self) -> Vec<ConflictId> {
        let mut ids: Vec<ConflictId> = self
            .identifiers
            .iter()
            .filter_map(|(_, outcome)| match outcome {
                AssignOutcome::Conflict { quarantined, .. } => Some(quarantined.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        if let SymbolAssignOutcome::Conflict { quarantined, .. } = &self.listing_symbol {
            ids.extend(quarantined);
        }
        ids
    }
}

/// Everything one entity-record ingestion did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityIngestReport {
    pub source_record: (SourceRecordId, Write),
    pub entity: Resolution<EntityId>,
    pub identifiers: Vec<(ExternalIdentifier, AssignOutcome)>,
}

/// Namespaces that resolve an entity. Adding one is an architectural
/// decision (`AGENT.md` §10).
pub const ENTITY_PRIMARY_NAMESPACES: [Namespace; 2] = [Namespace::Lei, Namespace::Cik];

impl From<sqlx::Error> for IngestError {
    fn from(err: sqlx::Error) -> Self {
        IngestError::Store(StoreError::Database(err))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Normalize(#[from] NormalizeError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Assignment(#[from] AssignmentError),
    #[error(transparent)]
    Relationship(#[from] RelationshipError),
    #[error("source `{0}` is not registered")]
    UnknownSource(SourceId),
    /// The primary identifier maps to several nodes, or a listing key matches
    /// several listings. Ingestion never guesses.
    #[error("{what} is ambiguous: {candidates:?}")]
    Ambiguous {
        what: String,
        candidates: Vec<CanonicalId>,
    },
    /// The primary identifier maps to a node whose object row is missing.
    #[error("node `{0}` has no stored object")]
    MissingObject(CanonicalId),
    /// Assigning a primary identifier to the node it resolved to conflicted.
    #[error("primary identifier {identifier:?} could not be assigned: {outcome:?}")]
    PrimaryIdentifierRejected {
        identifier: ExternalIdentifier,
        outcome: Box<AssignOutcome>,
    },
    /// An entity record carried no primary identifier, or an identifier that
    /// cannot resolve an entity.
    #[error("entity record identifiers {0:?} are not usable primary identifiers")]
    NoEntityPrimaryIdentifier(Vec<ExternalIdentifier>),
    #[error(transparent)]
    Fetch(#[from] undrly_provider::sec::http::FetchError),
    /// A fetched record describes something other than what was requested.
    #[error("requested {requested:?}, but the record is for {found:?}")]
    UnexpectedRecord {
        requested: ExternalIdentifier,
        found: Vec<ExternalIdentifier>,
    },
}

/// Ingests one reference record atomically. See the crate docs.
pub async fn ingest_reference<P, N>(
    conn: &mut PgConnection,
    provider: &P,
    normalizer: &N,
    raw: &RawRecord,
) -> Result<IngestReport, IngestError>
where
    P: ReferenceDataProvider,
    N: ReferenceNormalizer<Record = P::Record>,
{
    let record = provider.decode_reference(&raw.payload)?;
    let reference = normalizer.normalize(&record)?;

    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let report = persist(&mut tx, &reference, &record, write).await?;
    tx.commit().await?;
    Ok(report)
}

/// Ingests one entity record atomically: the raw record, the entity (found by
/// its primary identifiers or minted), and its identifier assignments.
pub async fn ingest_entity<P, N>(
    conn: &mut PgConnection,
    provider: &P,
    normalizer: &N,
    raw: &RawRecord,
) -> Result<EntityIngestReport, IngestError>
where
    P: ReferenceDataProvider,
    N: EntityNormalizer<Record = P::Record>,
{
    let record = provider.decode_reference(&raw.payload)?;
    let normalized: NormalizedEntityRecord = normalizer.normalize_entity(&record)?;
    check_entity_identifiers(&normalized.identifiers)?;

    let mut tx = conn.begin().await?;
    let (record, write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let mut identifiers = Vec::new();
    let entity = resolve_entity(
        &mut tx,
        normalized.kind,
        &normalized.name,
        &normalized.identifiers,
        &record,
        &mut identifiers,
    )
    .await?;
    tx.commit().await?;
    Ok(EntityIngestReport {
        source_record: (record.id, write),
        entity,
        identifiers,
    })
}

fn check_entity_identifiers(identifiers: &[ExternalIdentifier]) -> Result<(), IngestError> {
    if identifiers.is_empty()
        || identifiers
            .iter()
            .any(|i| !ENTITY_PRIMARY_NAMESPACES.contains(&i.namespace()))
    {
        return Err(IngestError::NoEntityPrimaryIdentifier(identifiers.to_vec()));
    }
    Ok(())
}

/// Stores the raw record first: every fact of the record derives from it.
async fn store_raw_record(
    tx: &mut PgConnection,
    source_id: &SourceId,
    raw: &RawRecord,
) -> Result<(RecordProvenance, Write), IngestError> {
    if get_source(tx, source_id).await?.is_none() {
        return Err(IngestError::UnknownSource(source_id.clone()));
    }
    Ok(insert_source_record(
        tx,
        &SourceRecord {
            source_id: source_id.clone(),
            record_key: raw.record_key.clone(),
            payload: raw.payload.clone(),
            received_at: raw.received_at,
        },
    )
    .await?)
}

/// Finds the entity by its primary identifiers, or mints one, then assigns
/// every primary identifier to it.
async fn resolve_entity(
    tx: &mut PgConnection,
    kind: EntityKind,
    name: &DisplayName,
    identifiers: &[ExternalIdentifier],
    record: &RecordProvenance,
    report: &mut Vec<(ExternalIdentifier, AssignOutcome)>,
) -> Result<Resolution<EntityId>, IngestError> {
    let entity = match resolve_primary(tx, identifiers).await? {
        Some(node) => Resolution::Existing(existing::<EntityId>(tx, node).await?),
        None => {
            let id = EntityId::generate();
            reference::insert_entity(
                tx,
                &Entity {
                    id,
                    kind,
                    name: name.clone(),
                },
                record.id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    for identifier in identifiers {
        assign_primary(tx, identifier.clone(), entity.id().into(), record, report).await?;
    }
    Ok(entity)
}

async fn persist(
    tx: &mut PgConnection,
    r: &NormalizedReference,
    record: &RecordProvenance,
    record_write: Write,
) -> Result<IngestReport, IngestError> {
    let (record_id, provenance) = (record.id, &record.provenance);
    let mut identifiers = Vec::new();

    // Entity, by LEI.
    let entity = resolve_entity(
        tx,
        r.issuer.kind,
        &r.issuer.name,
        &[ExternalIdentifier::Lei(r.issuer.lei.clone())],
        record,
        &mut identifiers,
    )
    .await?;

    // Instrument, by ISIN.
    let isin = ExternalIdentifier::Isin(r.instrument.isin.clone());
    let instrument = match resolve_primary(tx, std::slice::from_ref(&isin)).await? {
        Some(node) => Resolution::Existing(existing::<InstrumentId>(tx, node).await?),
        None => {
            let id = InstrumentId::generate();
            reference::insert_instrument(
                tx,
                &Instrument {
                    id,
                    class: r.instrument.class,
                    name: r.instrument.name.clone(),
                },
                record_id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    assign_primary(tx, isin, instrument.id().into(), record, &mut identifiers).await?;
    if let Some(figi) = &r.instrument.share_class_figi {
        let figi = ExternalIdentifier::Figi(figi.clone());
        assign_secondary(tx, figi, instrument.id().into(), record, &mut identifiers).await?;
    }

    // Currency, by ISO 4217 code.
    let code = ExternalIdentifier::Iso4217(r.denomination.code.clone());
    let currency = match resolve_primary(tx, std::slice::from_ref(&code)).await? {
        Some(node) => Resolution::Existing(existing::<CurrencyId>(tx, node).await?),
        None => {
            let id = CurrencyId::generate();
            reference::insert_currency(
                tx,
                &Currency {
                    id,
                    name: r.denomination.name.clone(),
                },
                record_id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    assign_primary(tx, code, currency.id().into(), record, &mut identifiers).await?;

    // Venue, by MIC.
    let mic = ExternalIdentifier::Mic(r.listing.mic.clone());
    let venue = match resolve_primary(tx, std::slice::from_ref(&mic)).await? {
        Some(node) => Resolution::Existing(existing::<VenueId>(tx, node).await?),
        None => {
            let id = VenueId::generate();
            reference::insert_venue(
                tx,
                &Venue {
                    id,
                    name: r.listing.venue_name.clone(),
                },
                record_id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    assign_primary(tx, mic, venue.id().into(), record, &mut identifiers).await?;

    // Listing, by (instrument, venue).
    let candidates: Vec<Listing> = reference::listings_for_instrument(tx, instrument.id())
        .await?
        .into_iter()
        .filter(|l| l.venue_id == venue.id())
        .collect();
    let listing = match candidates.as_slice() {
        [] => {
            let id = ListingId::generate();
            reference::insert_listing(
                tx,
                &Listing {
                    id,
                    instrument_id: instrument.id(),
                    venue_id: venue.id(),
                    provenance: provenance.clone(),
                },
                record_id,
            )
            .await?;
            Resolution::Created(id)
        }
        [one] => Resolution::Existing(one.id),
        many => {
            return Err(IngestError::Ambiguous {
                what: format!("listing of {} on {}", instrument.id(), venue.id()),
                candidates: many.iter().map(|l| l.id.canonical()).collect(),
            });
        }
    };
    if let Some(figi) = &r.listing.exchange_figi {
        let figi = ExternalIdentifier::Figi(figi.clone());
        assign_secondary(tx, figi, listing.id().into(), record, &mut identifiers).await?;
    }
    let listing_symbol = assign_listing_symbol(
        tx,
        &ListingSymbol {
            listing_id: listing.id(),
            venue_id: venue.id(),
            symbol: r.listing.symbol.clone(),
            valid_during: r.listing.symbol_valid_during,
            provenance: provenance.clone(),
        },
        record_id,
    )
    .await?;

    // Relationships asserted by the record, in canonical direction.
    let mut relationships = Vec::new();
    for (kind, object) in [
        (RelationshipType::IssuedBy, entity.id().canonical()),
        (RelationshipType::DenominatedIn, currency.id().canonical()),
    ] {
        let edge = Relationship::new(
            instrument.id().canonical(),
            kind,
            object,
            provenance.clone(),
        )?;
        relationships.push((
            kind,
            graph::insert_relationship(tx, &edge, record_id).await?,
        ));
    }

    Ok(IngestReport {
        source_record: (record_id, record_write),
        entity,
        instrument,
        currency,
        venue,
        listing,
        identifiers,
        listing_symbol,
        relationships,
    })
}

/// The single node the primary identifiers map to in any period, if any.
async fn resolve_primary(
    tx: &mut PgConnection,
    identifiers: &[ExternalIdentifier],
) -> Result<Option<CanonicalId>, IngestError> {
    let mut nodes: Vec<CanonicalId> = Vec::new();
    for identifier in identifiers {
        nodes.extend(
            identifier_history(tx, identifier)
                .await?
                .into_iter()
                .map(|s| s.assignment.node()),
        );
    }
    nodes.sort();
    nodes.dedup();
    match nodes.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        many => Err(IngestError::Ambiguous {
            what: identifiers
                .iter()
                .map(|i| format!("{} {}", i.namespace(), i.value()))
                .collect::<Vec<_>>()
                .join(", "),
            candidates: many.to_vec(),
        }),
    }
}

/// Converts a resolved node to its typed id and checks its object row exists.
async fn existing<T>(tx: &mut PgConnection, node: CanonicalId) -> Result<T, IngestError>
where
    T: TryFrom<CanonicalId>,
{
    let id = T::try_from(node).map_err(|_| IngestError::MissingObject(node))?;
    if reference::has_object(tx, node).await? {
        Ok(id)
    } else {
        Err(IngestError::MissingObject(node))
    }
}

async fn assign(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    node: CanonicalId,
    record: &RecordProvenance,
) -> Result<AssignOutcome, IngestError> {
    let claim = IdentifierAssignment::new(
        identifier,
        node,
        Validity::UNBOUNDED,
        record.provenance.clone(),
    )?;
    Ok(assign_identifier(tx, &claim, record.id).await?)
}

/// A primary identifier must end up mapped to the node it resolved to.
async fn assign_primary(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    node: CanonicalId,
    record: &RecordProvenance,
    report: &mut Vec<(ExternalIdentifier, AssignOutcome)>,
) -> Result<(), IngestError> {
    let outcome = assign(tx, identifier.clone(), node, record).await?;
    if let AssignOutcome::Conflict { .. } = outcome {
        return Err(IngestError::PrimaryIdentifierRejected {
            identifier,
            outcome: Box::new(outcome),
        });
    }
    report.push((identifier, outcome));
    Ok(())
}

/// A secondary identifier is a claim: conflicts are quarantined and reported.
async fn assign_secondary(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    node: CanonicalId,
    record: &RecordProvenance,
    report: &mut Vec<(ExternalIdentifier, AssignOutcome)>,
) -> Result<(), IngestError> {
    let outcome = assign(tx, identifier.clone(), node, record).await?;
    report.push((identifier, outcome));
    Ok(())
}
