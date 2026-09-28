//! Tracker certificates issued as tokens (V1.6, docs/v1.6-robinhood-chain.md):
//! a product of one issuer whose terms track another instrument's market
//! value, and its deployment on a chain.
//!
//! ```text
//! Final Terms (PDF) ─▶ raw record ─ SHA-256 = reviewed binding? ─▶ issuer (LEI), product (ISIN),
//!                                                                 ISSUED_BY, TRACKS underlying (ISIN),
//!                                                                 DENOMINATED_IN / SETTLES_IN currency
//! issuer registry   ─▶ raw record ─▶ asset with that Underlying ISIN on the chain ─▶ deployment
//!                                                                 └─ REPRESENTS product
//! ```
//!
//! Undrly does not parse PDFs. The product facts are a reviewed transcription
//! of the Final Terms ([`TrackerBinding`]), bound to the exact document by
//! its SHA-256: ingestion stores the fetched document raw and refuses the
//! facts if the bytes differ (a new version needs review). The facts name
//! that raw record. The binding never states a contract address: the
//! deployment comes only from the issuer's registry.
//!
//! Identity is by primary identifiers: the issuer by LEI, the product and the
//! underlying by ISIN, the deployment by (chain, address). The product never
//! receives the underlying's identifiers or issuer. An underlying that Undrly
//! cannot find by its ISIN leaves `TRACKS` unasserted (reported), never
//! guessed from a ticker.

use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgConnection};
use undrly_core::{
    Caip2, CanonicalId, CurrencyCode, Deployment, DeploymentId, DisplayName, Entity, EntityId,
    EntityKind, ExternalIdentifier, IdentifierAssignment, Instrument, InstrumentClass,
    InstrumentId, Isin, Lei, Relationship, RelationshipType, Validity,
};
use undrly_normalize::onchain::registry_deployment;
use undrly_provider::Provider;
use undrly_provider::rhj::{FINAL_TERMS_SOURCE_ID, RhjProvider};
use undrly_store::identifiers::{AssignOutcome, assign_identifier, resolve_identifier};
use undrly_store::sources::{RecordProvenance, SourceRecordId};
use undrly_store::{Write, graph, reference};

use crate::{IngestError, RawRecord, Resolution, store_raw_record};

/// A reviewed transcription of one product's Final Terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerBinding {
    /// SHA-256 (lowercase hex) of the exact Final Terms document reviewed.
    pub final_terms_sha256: String,
    pub issuer_name: DisplayName,
    pub issuer_lei: Lei,
    pub product_name: DisplayName,
    /// The product's own ISIN ("ISIN of the Product").
    pub product_isin: Isin,
    /// The Underlying's ISIN ("Security Codes of the Underlying").
    pub underlying_isin: Isin,
    /// The Product currency (and the currency redemption is paid in).
    pub currency: CurrencyCode,
    /// The Supported Blockchain System, as its CAIP-2 id.
    pub chain: Caip2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerReport {
    pub source_record: (SourceRecordId, Write),
    pub issuer: Resolution<EntityId>,
    pub product: Resolution<InstrumentId>,
    /// The tracked instrument, or `None` when Undrly has no node with the
    /// Underlying's ISIN (then no `TRACKS` edge is asserted).
    pub underlying: Option<InstrumentId>,
    pub relationships: Vec<(RelationshipType, Write)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerDeploymentReport {
    pub source_record: (SourceRecordId, Write),
    pub deployment: Resolution<DeploymentId>,
    pub caip19: String,
    pub deployment_write: Write,
    pub represents: Write,
    /// Other deployments on the same chain that already represent the
    /// product (the registry changed): kept, reported for review.
    pub others: Vec<(DeploymentId, String)>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The node holding `identifier` at the record's time, if any.
async fn node_of(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    record: &RecordProvenance,
) -> Result<Option<CanonicalId>, IngestError> {
    Ok(
        resolve_identifier(tx, &identifier, record.provenance.received_at)
            .await?
            .map(|s| s.assignment.node()),
    )
}

async fn assign(
    tx: &mut PgConnection,
    identifier: ExternalIdentifier,
    node: CanonicalId,
    record: &RecordProvenance,
) -> Result<(), IngestError> {
    let claim = IdentifierAssignment::new(
        identifier.clone(),
        node,
        Validity::UNBOUNDED,
        record.provenance.clone(),
    )?;
    match assign_identifier(tx, &claim, record.id).await? {
        AssignOutcome::Assigned(_) | AssignOutcome::Unchanged(_) => Ok(()),
        outcome => Err(IngestError::PrimaryIdentifierRejected {
            identifier,
            outcome: Box::new(outcome),
        }),
    }
}

/// Ingests a product's Final Terms (see the module docs).
pub async fn ingest_tracker_final_terms(
    conn: &mut PgConnection,
    raw: &RawRecord,
    binding: &TrackerBinding,
) -> Result<TrackerReport, IngestError> {
    let found = hex(&Sha256::digest(&raw.payload));
    if found != binding.final_terms_sha256 {
        return Err(IngestError::CuratedDisagrees(format!(
            "Final Terms changed: SHA-256 {found}, reviewed {}; review the binding",
            binding.final_terms_sha256
        )));
    }
    let source = undrly_core::SourceId::parse(FINAL_TERMS_SOURCE_ID).expect("valid source id");
    let mut tx = conn.begin().await?;
    let (record, record_write) = store_raw_record(&mut tx, &source, raw).await?;

    // The product's issuer, by LEI.
    let lei = ExternalIdentifier::Lei(binding.issuer_lei.clone());
    let issuer = match node_of(&mut tx, lei.clone(), &record).await? {
        Some(node) => Resolution::Existing(
            EntityId::try_from(node).map_err(|_| IngestError::MissingObject(node))?,
        ),
        None => {
            let id = EntityId::generate();
            reference::insert_entity(
                &mut tx,
                &Entity {
                    id,
                    kind: EntityKind::Company,
                    name: binding.issuer_name.clone(),
                },
                record.id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    assign(&mut tx, lei, issuer.id().into(), &record).await?;

    // The product, by its own ISIN; it must be a tokenized security.
    let isin = ExternalIdentifier::Isin(binding.product_isin.clone());
    let product = match node_of(&mut tx, isin.clone(), &record).await? {
        Some(node) => {
            let id = InstrumentId::try_from(node).map_err(|_| IngestError::MissingObject(node))?;
            match reference::get_instrument(&mut tx, id).await? {
                Some(i) if i.class == InstrumentClass::TokenizedSecurity => {}
                _ => {
                    return Err(IngestError::CuratedDisagrees(format!(
                        "{} names {node}, which is not a tokenized security",
                        binding.product_isin
                    )));
                }
            }
            Resolution::Existing(id)
        }
        None => {
            let id = InstrumentId::generate();
            reference::insert_instrument(
                &mut tx,
                &Instrument {
                    id,
                    class: InstrumentClass::TokenizedSecurity,
                    name: binding.product_name.clone(),
                    contract_multiplier: None,
                    unit_of_measure: None,
                    fx_pair: None,
                },
                record.id,
            )
            .await?;
            Resolution::Created(id)
        }
    };
    assign(&mut tx, isin, product.id().into(), &record).await?;

    // The underlying, by its ISIN only; never by ticker.
    let underlying = match node_of(
        &mut tx,
        ExternalIdentifier::Isin(binding.underlying_isin.clone()),
        &record,
    )
    .await?
    {
        Some(node) => {
            Some(InstrumentId::try_from(node).map_err(|_| IngestError::MissingObject(node))?)
        }
        None => None,
    };
    let currency = node_of(
        &mut tx,
        ExternalIdentifier::Iso4217(binding.currency.clone()),
        &record,
    )
    .await?
    .ok_or_else(|| {
        IngestError::CuratedDisagrees(format!("currency {} is not stored", binding.currency))
    })?;

    let product_id: CanonicalId = product.id().into();
    let mut edges = vec![
        (RelationshipType::IssuedBy, issuer.id().into()),
        (RelationshipType::DenominatedIn, currency),
        (RelationshipType::SettlesIn, currency),
    ];
    if let Some(u) = underlying {
        edges.insert(1, (RelationshipType::Tracks, u.into()));
    }
    let mut relationships = Vec::new();
    for (kind, object) in edges {
        let edge = Relationship::new(product_id, kind, object, record.provenance.clone())?;
        relationships.push((
            kind,
            graph::insert_relationship(&mut tx, &edge, record.id).await?,
        ));
    }
    tx.commit().await?;
    Ok(TrackerReport {
        source_record: (record.id, record_write),
        issuer,
        product,
        underlying,
        relationships,
    })
}

/// Ingests the issuer's registry for the bound product: its deployment on
/// the bound chain `REPRESENTS` the product (which must already exist, from
/// its Final Terms), asserted by the registry record.
pub async fn ingest_tracker_deployment(
    conn: &mut PgConnection,
    raw: &RawRecord,
    binding: &TrackerBinding,
) -> Result<TrackerDeploymentReport, IngestError> {
    let provider = RhjProvider::new();
    let assets = provider.decode_assets(&raw.payload)?;
    let asset = registry_deployment(&assets, binding.underlying_isin.as_str(), &binding.chain)?;

    let mut tx = conn.begin().await?;
    let (record, record_write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let chain = reference::chain_by_caip2(&mut tx, &binding.chain)
        .await?
        .ok_or_else(|| IngestError::MissingChain(binding.chain.to_string()))?;
    let product = node_of(
        &mut tx,
        ExternalIdentifier::Isin(binding.product_isin.clone()),
        &record,
    )
    .await?
    .ok_or_else(|| {
        IngestError::CuratedDisagrees(format!(
            "product {} is not stored; ingest its Final Terms first",
            binding.product_isin
        ))
    })?;
    let id = match reference::deployment_by_asset(&mut tx, chain.id, &asset).await? {
        Some(existing) => Resolution::Existing(existing.id),
        None => Resolution::Created(DeploymentId::generate()),
    };
    let deployment = Deployment::on(&chain, id.id(), asset.clone())
        .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
    let deployment_write = reference::insert_deployment(&mut tx, &deployment, record.id).await?;
    let edge = Relationship::new(
        id.id().into(),
        RelationshipType::Represents,
        product,
        record.provenance.clone(),
    )?;
    let represents = graph::insert_relationship(&mut tx, &edge, record.id).await?;
    let caip19 = asset
        .caip19(&chain.caip2)
        .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;

    let mut others = Vec::new();
    for e in graph::relationships_to(&mut tx, product, Some(RelationshipType::Represents)).await? {
        let Ok(other) = DeploymentId::try_from(e.relationship.subject()) else {
            continue;
        };
        if other == id.id() {
            continue;
        }
        if let Some(d) = reference::get_deployment(&mut tx, other).await?
            && d.chain_id == chain.id
        {
            let text = d
                .asset
                .caip19(&chain.caip2)
                .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
            others.push((other, text));
        }
    }
    tx.commit().await?;
    Ok(TrackerDeploymentReport {
        source_record: (record.id, record_write),
        deployment: id,
        caip19,
        deployment_write,
        represents,
        others,
    })
}
