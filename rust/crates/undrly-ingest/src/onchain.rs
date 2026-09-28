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
    Alias, AliasKind, AssetNamespace, Caip2, CanonicalId, Chain, ChainAsset, ChainId, CurrencyCode,
    Deployment, DeploymentId, DisplayName, ExternalIdentifier, Instrument, InstrumentClass,
    InstrumentId, Relationship, RelationshipType,
};
use undrly_normalize::onchain::{evm_caip2, issuer_deployment, solana_caip2};
use undrly_provider::Provider;
use undrly_provider::circle::CircleProvider;
use undrly_provider::evm_rpc::EvmRpcProvider;
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
    ingest_chain(conn, &provider, raw, caip2, binding).await
}

/// Ingests an EVM chain's `eth_chainId` response (from its RPC, under
/// `source_id`) as the chain `binding` names (V1.6).
pub async fn ingest_evm_chain(
    conn: &mut PgConnection,
    raw: &RawRecord,
    source_id: &str,
    binding: &ChainBinding,
) -> Result<ChainReport, IngestError> {
    let provider = EvmRpcProvider::new(source_id)
        .map_err(|e| IngestError::CuratedDisagrees(format!("source id: {e}")))?;
    let chain_id = provider.decode_chain_id(&raw.payload)?;
    let caip2 = evm_caip2(chain_id)?;
    ingest_chain(conn, &provider, raw, caip2, binding).await
}

async fn ingest_chain(
    conn: &mut PgConnection,
    provider: &impl Provider,
    raw: &RawRecord,
    caip2: Caip2,
    binding: &ChainBinding,
) -> Result<ChainReport, IngestError> {
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

/// A TIP-20 asset bound for ingestion (V1.7): where it is (a protocol
/// constant, e.g. a predeploy) and what the chain must report about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip20Binding {
    pub chain: Caip2,
    pub asset: ChainAsset,
    pub name: String,
    pub symbol: String,
    /// The TIP-20 `currency()`: the reference asset one unit is designed to
    /// be worth, as an ISO 4217 code.
    pub currency: CurrencyCode,
    pub decimals: u8,
    pub instrument_name: DisplayName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip20Report {
    pub source_record: (SourceRecordId, Write),
    pub instrument: Resolution<InstrumentId>,
    pub deployment: Resolution<DeploymentId>,
    pub caip19: String,
    pub writes: Vec<(&'static str, Write)>,
}

/// Ingests a batched TIP-20 metadata response (chain id and the token's
/// `name`, `symbol`, `currency`, `decimals`) from the chain's RPC. The
/// response must describe the bound chain and match the bound metadata
/// exactly; otherwise nothing is written. The asset is found by its
/// deployment (it exists natively only there), else minted as a crypto
/// asset; its deployment `REPRESENTS` it; it `TRACKS` the declared currency.
/// Every fact names the response's raw record.
pub async fn ingest_tip20_asset(
    conn: &mut PgConnection,
    raw: &RawRecord,
    source_id: &str,
    binding: &Tip20Binding,
) -> Result<Tip20Report, IngestError> {
    let provider = EvmRpcProvider::new(source_id)
        .map_err(|e| IngestError::CuratedDisagrees(format!("source id: {e}")))?;
    let m = provider.decode_tip20_metadata(&raw.payload)?;
    let caip2 = evm_caip2(m.chain_id)?;
    if caip2 != binding.chain {
        return Err(IngestError::UnexpectedChain {
            expected: binding.chain.to_string(),
            found: caip2.to_string(),
        });
    }
    let found = (
        m.name.as_str(),
        m.symbol.as_str(),
        m.currency.as_str(),
        m.decimals,
    );
    let expected = (
        binding.name.as_str(),
        binding.symbol.as_str(),
        binding.currency.as_str(),
        binding.decimals,
    );
    if found != expected {
        return Err(IngestError::CuratedDisagrees(format!(
            "{} on {caip2}: chain reports {found:?}, binding expects {expected:?}; review",
            binding.asset.reference()
        )));
    }

    let mut tx = conn.begin().await?;
    let (record, record_write) = store_raw_record(&mut tx, provider.source_id(), raw).await?;
    let chain = reference::chain_by_caip2(&mut tx, &binding.chain)
        .await?
        .ok_or_else(|| IngestError::MissingChain(binding.chain.to_string()))?;
    let currency = undrly_store::identifiers::resolve_identifier(
        &mut tx,
        &ExternalIdentifier::Iso4217(binding.currency.clone()),
        record.provenance.received_at,
    )
    .await?
    .map(|s| s.assignment.node())
    .ok_or_else(|| {
        IngestError::CuratedDisagrees(format!("currency {} is not stored", binding.currency))
    })?;

    // The asset: whatever its (native) deployment already represents.
    let existing = reference::deployment_by_asset(&mut tx, chain.id, &binding.asset).await?;
    let mut represented = Vec::new();
    if let Some(d) = &existing {
        for e in graph::relationships_from(&mut tx, d.id.into(), Some(RelationshipType::Represents))
            .await?
        {
            let object = e.relationship.object();
            if !represented.contains(&object) {
                represented.push(object);
            }
        }
    }
    let instrument = match represented.as_slice() {
        [] => {
            let id = InstrumentId::generate();
            reference::insert_instrument(
                &mut tx,
                &Instrument {
                    id,
                    class: InstrumentClass::CryptoAsset,
                    name: binding.instrument_name.clone(),
                    contract_multiplier: None,
                    unit_of_measure: None,
                    fx_pair: None,
                },
                record.id,
            )
            .await?;
            Resolution::Created(id)
        }
        [one] => Resolution::Existing(
            InstrumentId::try_from(*one).map_err(|_| IngestError::MissingObject(*one))?,
        ),
        several => {
            return Err(IngestError::Ambiguous {
                what: format!(
                    "{} represents several instruments",
                    binding.asset.reference()
                ),
                candidates: several.to_vec(),
            });
        }
    };
    let deployment = match &existing {
        Some(d) => Resolution::Existing(d.id),
        None => Resolution::Created(DeploymentId::generate()),
    };
    let mut writes = Vec::new();
    let d = Deployment::on(&chain, deployment.id(), binding.asset.clone())
        .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
    writes.push((
        "deployment",
        reference::insert_deployment(&mut tx, &d, record.id).await?,
    ));
    // The token's own onchain symbol, as a search term (never identity).
    writes.push((
        "alias",
        undrly_store::aliases::insert_alias(
            &mut tx,
            &Alias {
                node: instrument.id().into(),
                text: DisplayName::new(&m.symbol)
                    .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?,
                kind: AliasKind::Symbol,
                provenance: record.provenance.clone(),
            },
            record.id,
        )
        .await?,
    ));
    for (kind, subject, object) in [
        (
            RelationshipType::Represents,
            deployment.id().into(),
            instrument.id().into(),
        ),
        (RelationshipType::Tracks, instrument.id().into(), currency),
    ] {
        let edge = Relationship::new(subject, kind, object, record.provenance.clone())?;
        writes.push((
            kind.as_str(),
            graph::insert_relationship(&mut tx, &edge, record.id).await?,
        ));
    }
    let caip19 = binding
        .asset
        .caip19(&chain.caip2)
        .map_err(|e| IngestError::CuratedDisagrees(e.to_string()))?;
    tx.commit().await?;
    Ok(Tip20Report {
        source_record: (record.id, record_write),
        instrument,
        deployment,
        caip19,
        writes,
    })
}
