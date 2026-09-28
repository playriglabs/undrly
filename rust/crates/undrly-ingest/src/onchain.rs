//! Chains and issuer deployments from authoritative sources (V1.5).
//!
//! ```text
//! Solana RPC getGenesisHash ──▶ raw record ──▶ CAIP-2 ──▶ chain
//! Circle USDC address page  ──▶ raw record ──▶ (chain, mint) ──▶ deployment
//!                                                 └─ REPRESENTS ──▶ USD Coin
//! ```
//!
//! Each record is ingested in one transaction, raw record first; decoding
//! and normalization run before any write, so a malformed or unexpected
//! record writes nothing.
//!
//! Identity is resolved by natural keys, as for every node: a chain by its
//! CAIP-2 id, a deployment by (chain, asset namespace, asset reference). No
//! match mints a new id; nothing is ever matched by name or symbol.
//!
//! A [`ChainBinding`] says which chain a source is expected to describe: a
//! response for another cluster (devnet answering on a mainnet URL) is
//! rejected, never recorded as mainnet. An [`IssuerBinding`] says which
//! existing Undrly instrument the issuer's list is about, and which of its
//! rows (by the issuer's own label) is which chain. The chain must already
//! exist, asserted by its own source.

use sqlx::{Acquire, PgConnection};
use undrly_core::{
    AssetNamespace, Caip2, CanonicalId, Chain, ChainId, Deployment, DeploymentId, DisplayName,
    InstrumentClass, InstrumentId, Relationship, RelationshipType,
};
use undrly_normalize::onchain::{issuer_deployment, solana_caip2};
use undrly_provider::Provider;
use undrly_provider::circle::CircleProvider;
use undrly_provider::solana::SolanaRpcProvider;
use undrly_store::sources::SourceRecordId;
use undrly_store::{Write, graph, reference};

use crate::{IngestError, RawRecord, Resolution, store_raw_record};

/// The chain a source is expected to describe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBinding {
    pub name: DisplayName,
    pub caip2: Caip2,
}

/// Which instrument an issuer's address list is about, and which of its rows
/// (the issuer's label, exactly) is which chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerBinding {
    pub instrument: InstrumentId,
    pub deployments: Vec<IssuerRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerRow {
    pub label: String,
    pub chain: Caip2,
    pub namespace: AssetNamespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    pub source_record: (SourceRecordId, Write),
    pub chain: Resolution<ChainId>,
    pub write: Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentReport {
    pub source_record: (SourceRecordId, Write),
    /// Per bound row: the deployment, its CAIP-19 id, and the writes of the
    /// deployment and its `REPRESENTS` edge.
    pub deployments: Vec<(Resolution<DeploymentId>, String, Write, Write)>,
    /// Other deployments on the same chains that already represent the
    /// instrument (e.g. the issuer changed its address). Kept, reported for
    /// review; nothing is retracted automatically.
    pub others: Vec<(DeploymentId, String)>,
}

/// Ingests a Solana `getGenesisHash` response as the chain `binding` names.
pub async fn ingest_solana_chain(
    conn: &mut PgConnection,
    raw: &RawRecord,
    binding: &ChainBinding,
) -> Result<ChainReport, IngestError> {
    let provider = SolanaRpcProvider::new();
    let genesis = provider.decode_genesis_hash(&raw.payload)?;
    let caip2 = solana_caip2(&genesis)?;
    if caip2 != binding.caip2 {
        return Err(IngestError::UnexpectedChain {
            expected: binding.caip2.to_string(),
            found: caip2.to_string(),
        });
    }
    let mut tx = conn.begin().await?;
    let (record, record_write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let chain = match reference::chain_by_caip2(&mut tx, &caip2).await? {
        Some(existing) => Resolution::Existing(existing.id),
        None => Resolution::Created(ChainId::generate()),
    };
    let write = reference::insert_chain(
        &mut tx,
        &Chain {
            id: chain.id(),
            name: binding.name.clone(),
            caip2,
        },
        record.id,
    )
    .await
    .or_else(|e| match (&chain, e) {
        // An existing chain keeps its first name; a later source's differing
        // display name is not an identity change.
        (Resolution::Existing(_), undrly_store::StoreError::ExistingRecordDiffers { .. }) => {
            Ok(Write::Unchanged)
        }
        (_, e) => Err(e),
    })?;
    tx.commit().await?;
    Ok(ChainReport {
        source_record: (record.id, record_write),
        chain,
        write,
    })
}

/// Ingests Circle's USDC address page for the rows `binding` names: each
/// becomes a deployment on its (existing) chain that `REPRESENTS` the bound
/// instrument, asserted by the Circle record.
pub async fn ingest_circle_usdc(
    conn: &mut PgConnection,
    raw: &RawRecord,
    binding: &IssuerBinding,
) -> Result<DeploymentReport, IngestError> {
    let provider = CircleProvider::new();
    let rows = provider.decode_usdc_mainnet(&raw.payload)?;
    let assets = binding
        .deployments
        .iter()
        .map(|b| issuer_deployment(&rows, &b.label, &b.chain, b.namespace).map(|a| (b, a)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut tx = conn.begin().await?;
    match reference::get_instrument(&mut tx, binding.instrument).await? {
        Some(i) if i.class == InstrumentClass::CryptoAsset => {}
        _ => {
            return Err(IngestError::MissingObject(binding.instrument.canonical()));
        }
    }
    let (record, record_write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let mut deployments = Vec::new();
    let mut chains = Vec::new();
    for (row, asset) in assets {
        let chain = reference::chain_by_caip2(&mut tx, &row.chain)
            .await?
            .ok_or_else(|| IngestError::MissingChain(row.chain.to_string()))?;
        let id = match reference::deployment_by_asset(&mut tx, chain.id, &asset).await? {
            Some(existing) => Resolution::Existing(existing.id),
            None => Resolution::Created(DeploymentId::generate()),
        };
        let deployment = Deployment::on(&chain, id.id(), asset.clone())
            .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
        let written = reference::insert_deployment(&mut tx, &deployment, record.id).await?;
        let edge = Relationship::new(
            id.id().into(),
            RelationshipType::Represents,
            binding.instrument.into(),
            record.provenance.clone(),
        )?;
        let represents = graph::insert_relationship(&mut tx, &edge, record.id).await?;
        let caip19 = asset
            .caip19(&chain.caip2)
            .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
        deployments.push((id, caip19, written, represents));
        chains.push(chain);
    }
    let mut others = Vec::new();
    for edge in graph::relationships_to(
        &mut tx,
        binding.instrument.into(),
        Some(RelationshipType::Represents),
    )
    .await?
    {
        let Ok(id) = DeploymentId::try_from(edge.relationship.subject()) else {
            continue;
        };
        if deployments.iter().any(|(d, ..)| d.id() == id) {
            continue;
        }
        let Some(d) = reference::get_deployment(&mut tx, id).await? else {
            return Err(IngestError::MissingObject(CanonicalId::from(id)));
        };
        if let Some(chain) = chains.iter().find(|c| c.id == d.chain_id)
            && !others.iter().any(|(o, _)| *o == id)
        {
            let caip19 = d
                .asset
                .caip19(&chain.caip2)
                .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
            others.push((id, caip19));
        }
    }
    tx.commit().await?;
    Ok(DeploymentReport {
        source_record: (record.id, record_write),
        deployments,
        others,
    })
}
