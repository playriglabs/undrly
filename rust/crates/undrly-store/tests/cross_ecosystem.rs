//! V1.4 cross-ecosystem identity (docs/v1.4-cross-ecosystem-identity.md):
//! chains, deployments and the edges between them and instruments. All
//! chains, addresses and instruments here are fixtures, not production data.

mod common;

use common::{
    CHECK_VIOLATION, FOREIGN_KEY_VIOLATION, RECEIVED_AT, SOURCE, TestDb, UNIQUE_VIOLATION,
    assert_rejected,
};
use sqlx::types::Uuid;
use undrly_core::{
    AssetNamespace, Caip2, CanonicalId, Chain, ChainAsset, ChainId, ChainNamespace, CurrencyId,
    Deployment, DeploymentId, DisplayName, ExternalIdentifier, IdentifierAssignment, Instrument,
    InstrumentClass, InstrumentId, Isin, Provenance, Relationship, RelationshipType, SourceId,
    Timestamp, Validity,
};
use undrly_store::StoreError;
use undrly_store::Write;
use undrly_store::graph::{insert_relationship, relationships_from, relationships_to};
use undrly_store::identifiers::{AssignOutcome, assign_identifier};
use undrly_store::reference::{
    chain_by_caip2, deployment_by_asset, get_deployment, insert_chain, insert_deployment,
    insert_instrument,
};
use undrly_store::sources::SourceRecordId;

// Specification examples (CAIP-19, Solana CAIP-2/19 profiles), used as fixtures.
const EVM_ADDRESS: &str = "0x6b175474e89094c44da98b954eedeac495271d0f";
const SOLANA_MAINNET: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const SPL_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

fn provenance() -> Provenance {
    Provenance {
        source_id: SourceId::parse(SOURCE).unwrap(),
        received_at: Timestamp::parse(RECEIVED_AT).unwrap(),
    }
}

fn chain(name: &str, caip2: &str) -> Chain {
    Chain {
        id: ChainId::generate(),
        name: DisplayName::new(name).unwrap(),
        caip2: Caip2::parse(caip2).unwrap(),
    }
}

fn instrument(class: InstrumentClass, name: &str) -> Instrument {
    Instrument {
        id: InstrumentId::generate(),
        class,
        name: DisplayName::new(name).unwrap(),
        contract_multiplier: None,
        unit_of_measure: None,
        fx_pair: None,
    }
}

fn erc20(address: &str) -> ChainAsset {
    ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, address).unwrap()
}

fn edge(subject: CanonicalId, kind: RelationshipType, object: CanonicalId) -> Relationship {
    Relationship::new(subject, kind, object, provenance()).unwrap()
}

struct World {
    db: TestDb,
    record: SourceRecordId,
    ethereum: Chain,
    base: Chain,
    solana: Chain,
}

async fn world() -> Option<World> {
    let db = common::fresh().await?;
    let record = SourceRecordId(db.record);
    let (ethereum, base, solana) = (
        chain("Ethereum (fixture)", "eip155:1"),
        chain("Base (fixture)", "eip155:8453"),
        chain("Solana (fixture)", &format!("solana:{SOLANA_MAINNET}")),
    );
    let mut conn = db.pool.acquire().await.unwrap();
    for c in [&ethereum, &base, &solana] {
        assert_eq!(
            insert_chain(&mut conn, c, record).await.unwrap(),
            Write::Inserted
        );
    }
    drop(conn);
    Some(World {
        db,
        record,
        ethereum,
        base,
        solana,
    })
}

#[tokio::test]
async fn a_stablecoin_has_one_economic_identity_and_separate_deployments() {
    let Some(w) = world().await else {
        return;
    };
    let mut conn = w.db.pool.acquire().await.unwrap();
    let usdc = instrument(InstrumentClass::CryptoAsset, "USD Coin (fixture)");
    insert_instrument(&mut conn, &usdc, w.record).await.unwrap();

    let on_ethereum =
        Deployment::on(&w.ethereum, DeploymentId::generate(), erc20(EVM_ADDRESS)).unwrap();
    let on_solana = Deployment::on(
        &w.solana,
        DeploymentId::generate(),
        ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Token, SPL_MINT).unwrap(),
    )
    .unwrap();
    for d in [&on_ethereum, &on_solana] {
        insert_deployment(&mut conn, d, w.record).await.unwrap();
        insert_relationship(
            &mut conn,
            &edge(d.id.into(), RelationshipType::Represents, usdc.id.into()),
            w.record,
        )
        .await
        .unwrap();
    }

    // Two deployments, each its own node, both representing one asset.
    assert_ne!(on_ethereum.id, on_solana.id);
    let represented_by: Vec<CanonicalId> = relationships_to(
        &mut conn,
        usdc.id.into(),
        Some(RelationshipType::Represents),
    )
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.relationship.subject())
    .collect();
    assert_eq!(
        represented_by,
        vec![on_ethereum.id.canonical(), on_solana.id.canonical()]
    );
    // The asset is not a deployment, and a deployment has no instrument
    // column: what it represents is only ever the edge.
    assert_ne!(usdc.id.canonical(), on_ethereum.id.canonical());
    let reps = relationships_from(&mut conn, on_ethereum.id.into(), None)
        .await
        .unwrap();
    assert_eq!(reps.len(), 1);
    assert_eq!(
        reps[0].source_record, w.record,
        "the edge keeps its provenance"
    );

    // A deployment without a REPRESENTS edge is allowed: unknown stays unknown.
    let unknown = Deployment::on(
        &w.base,
        DeploymentId::generate(),
        erc20("0x8f8221afbb33998d8584a2b05749ba73c37a938a"),
    )
    .unwrap();
    insert_deployment(&mut conn, &unknown, w.record)
        .await
        .unwrap();
    assert!(
        relationships_from(&mut conn, unknown.id.into(), None)
            .await
            .unwrap()
            .is_empty()
    );
    drop(conn);
    w.db.teardown().await;
}

#[tokio::test]
async fn the_same_evm_address_on_two_chains_is_two_deployments() {
    let Some(w) = world().await else {
        return;
    };
    let mut conn = w.db.pool.acquire().await.unwrap();
    let asset = erc20(EVM_ADDRESS);
    let a = Deployment::on(&w.ethereum, DeploymentId::generate(), asset.clone()).unwrap();
    let b = Deployment::on(&w.base, DeploymentId::generate(), asset.clone()).unwrap();
    insert_deployment(&mut conn, &a, w.record).await.unwrap();
    insert_deployment(&mut conn, &b, w.record).await.unwrap();

    // Lookup needs the chain; the address alone selects nothing.
    assert_eq!(
        deployment_by_asset(&mut conn, w.ethereum.id, &asset)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(a.id)
    );
    assert_eq!(
        deployment_by_asset(&mut conn, w.base.id, &asset)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(b.id)
    );
    assert_eq!(
        deployment_by_asset(&mut conn, w.solana.id, &asset)
            .await
            .unwrap(),
        None
    );

    // On one chain, one address is one deployment: a second id is refused by
    // the repository and by the database.
    let twin = Deployment::on(&w.ethereum, DeploymentId::generate(), asset.clone()).unwrap();
    assert!(matches!(
        insert_deployment(&mut conn, &twin, w.record).await,
        Err(StoreError::ExistingRecordDiffers {
            what: "deployment",
            ..
        })
    ));
    let raw = sqlx::query(
        "WITH n AS (INSERT INTO nodes (id, category) VALUES ($1, 'deployment') RETURNING id)
         INSERT INTO deployments (id, chain_id, chain_namespace, asset_namespace, asset_reference, source_record_id)
         SELECT id, $2, 'eip155', 'erc20', $3, $4 FROM n",
    )
    .bind(twin.id.uuid())
    .bind(w.ethereum.id.uuid())
    .bind(EVM_ADDRESS)
    .bind(w.db.record)
    .execute(&w.db.pool)
    .await;
    assert_rejected(raw, UNIQUE_VIOLATION, Some("deployments_one_per_asset"));

    // Replaying the same deployment is idempotent.
    assert_eq!(
        insert_deployment(&mut conn, &a, w.record).await.unwrap(),
        Write::Unchanged
    );
    assert_eq!(get_deployment(&mut conn, a.id).await.unwrap(), Some(a));
    drop(conn);
    w.db.teardown().await;
}

/// Inserts a deployment row directly (bypassing Rust validation) so the
/// database's own constraints can be tested.
async fn raw_deployment(
    db: &TestDb,
    chain: Uuid,
    chain_namespace: &str,
    asset_namespace: &str,
    reference: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "WITH n AS (INSERT INTO nodes (id, category) VALUES ($1, 'deployment') RETURNING id)
         INSERT INTO deployments (id, chain_id, chain_namespace, asset_namespace, asset_reference, source_record_id)
         SELECT id, $2, $3, $4, $5, $6 FROM n",
    )
    .bind(Uuid::now_v7())
    .bind(chain)
    .bind(chain_namespace)
    .bind(asset_namespace)
    .bind(reference)
    .bind(db.record)
    .execute(&db.pool)
    .await
}

#[tokio::test]
async fn solana_mints_and_evm_addresses_cannot_collide() {
    let Some(w) = world().await else {
        return;
    };
    let (eth, sol) = (w.ethereum.id.uuid(), w.solana.id.uuid());
    // Each namespace on its own chain family is accepted.
    raw_deployment(&w.db, eth, "eip155", "erc20", EVM_ADDRESS)
        .await
        .unwrap();
    raw_deployment(&w.db, sol, "solana", "token", SPL_MINT)
        .await
        .unwrap();
    raw_deployment(&w.db, sol, "solana", "slip44", "501")
        .await
        .unwrap();
    // An ERC-20 on Solana, an SPL mint on an EVM chain: rejected.
    assert_rejected(
        raw_deployment(&w.db, sol, "solana", "erc20", EVM_ADDRESS).await,
        CHECK_VIOLATION,
        Some("deployments_asset_namespace_on_chain"),
    );
    assert_rejected(
        raw_deployment(&w.db, eth, "eip155", "token", SPL_MINT).await,
        CHECK_VIOLATION,
        Some("deployments_asset_namespace_on_chain"),
    );
    // A deployment cannot claim a namespace its chain does not have.
    assert_rejected(
        raw_deployment(&w.db, sol, "eip155", "erc20", EVM_ADDRESS).await,
        FOREIGN_KEY_VIOLATION,
        Some("deployments_chain_id_chain_namespace_fkey"),
    );
    // Each namespace has its own reference shape; EVM text is canonical lowercase.
    for (ns, chain_ns, chain, reference) in [
        ("erc20", "eip155", eth, SPL_MINT),
        (
            "erc20",
            "eip155",
            eth,
            "0x6B175474E89094C44DA98B954EEDEAC495271D0F",
        ),
        ("token", "solana", sol, EVM_ADDRESS),
        ("slip44", "solana", sol, "0501"),
    ] {
        assert_rejected(
            raw_deployment(&w.db, chain, chain_ns, ns, reference).await,
            CHECK_VIOLATION,
            Some("deployments_asset_reference_shape"),
        );
    }
    // Rust refuses the same crossings before any SQL runs.
    assert!(ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Erc20, EVM_ADDRESS).is_err());
    assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Token, SPL_MINT).is_err());
    assert!(Deployment::on(&w.solana, DeploymentId::generate(), erc20(EVM_ADDRESS)).is_err());
    w.db.teardown().await;
}

#[tokio::test]
async fn a_chain_is_one_node_per_caip2_id() {
    let Some(w) = world().await else {
        return;
    };
    let mut conn = w.db.pool.acquire().await.unwrap();
    assert_eq!(
        chain_by_caip2(&mut conn, &Caip2::parse("eip155:8453").unwrap())
            .await
            .unwrap()
            .map(|c| c.id),
        Some(w.base.id)
    );
    // Case-sensitive: a Solana genesis prefix is not case-folded.
    let lower = format!("solana:{}", SOLANA_MAINNET.to_lowercase());
    if let Ok(caip2) = Caip2::parse(&lower) {
        assert_eq!(chain_by_caip2(&mut conn, &caip2).await.unwrap(), None);
    }
    let again = chain("Ethereum again (fixture)", "eip155:1");
    assert!(matches!(
        insert_chain(&mut conn, &again, w.record).await,
        Err(StoreError::ExistingRecordDiffers { what: "chain", .. })
    ));
    let raw = sqlx::query(
        "WITH n AS (INSERT INTO nodes (id, category) VALUES ($1, 'chain') RETURNING id)
         INSERT INTO chains (id, name, caip2_namespace, caip2_reference, source_record_id)
         SELECT id, 'x', $2, $3, $4 FROM n",
    );
    assert_rejected(
        raw.bind(Uuid::now_v7())
            .bind("eip155")
            .bind("1")
            .bind(w.db.record)
            .execute(&w.db.pool)
            .await,
        UNIQUE_VIOLATION,
        Some("chains_one_per_caip2"),
    );
    for (ns, reference) in [("eip155", "01"), ("eip155", "0x1"), ("solana", "short")] {
        let raw = sqlx::query(
            "WITH n AS (INSERT INTO nodes (id, category) VALUES ($1, 'chain') RETURNING id)
             INSERT INTO chains (id, name, caip2_namespace, caip2_reference, source_record_id)
             SELECT id, 'x', $2, $3, $4 FROM n",
        )
        .bind(Uuid::now_v7())
        .bind(ns)
        .bind(reference)
        .bind(w.db.record)
        .execute(&w.db.pool)
        .await;
        assert_rejected(raw, CHECK_VIOLATION, Some("chains_caip2_reference_shape"));
    }
    drop(conn);
    w.db.teardown().await;
}

#[tokio::test]
async fn a_tokenized_security_is_not_the_security_it_tokenizes() {
    let Some(w) = world().await else {
        return;
    };
    let mut conn = w.db.pool.acquire().await.unwrap();
    // NVDA's ISIN is the repository's authoritative fixture (V1).
    let stock = instrument(InstrumentClass::Equity, "Common stock (fixture)");
    let token = instrument(
        InstrumentClass::TokenizedSecurity,
        "Share-backed token (fixture)",
    );
    insert_instrument(&mut conn, &stock, w.record)
        .await
        .unwrap();
    insert_instrument(&mut conn, &token, w.record)
        .await
        .unwrap();
    insert_relationship(
        &mut conn,
        &edge(
            token.id.into(),
            RelationshipType::Tokenizes,
            stock.id.into(),
        ),
        w.record,
    )
    .await
    .unwrap();
    let deployment =
        Deployment::on(&w.ethereum, DeploymentId::generate(), erc20(EVM_ADDRESS)).unwrap();
    insert_deployment(&mut conn, &deployment, w.record)
        .await
        .unwrap();
    insert_relationship(
        &mut conn,
        &edge(
            deployment.id.into(),
            RelationshipType::Represents,
            token.id.into(),
        ),
        w.record,
    )
    .await
    .unwrap();

    // The security's ISIN names the security; a claim that it names the
    // token is a conflict, quarantined, never a second mapping.
    let isin = ExternalIdentifier::Isin(Isin::parse("US67066G1040").unwrap());
    let claim = |node: InstrumentId| {
        IdentifierAssignment::new(isin.clone(), node.into(), Validity::UNBOUNDED, provenance())
            .unwrap()
    };
    assert!(matches!(
        assign_identifier(&mut conn, &claim(stock.id), w.record)
            .await
            .unwrap(),
        AssignOutcome::Assigned(_)
    ));
    assert!(matches!(
        assign_identifier(&mut conn, &claim(token.id), w.record)
            .await
            .unwrap(),
        AssignOutcome::Conflict { .. }
    ));
    // A deployment can never carry an ISIN at all.
    assert!(
        IdentifierAssignment::new(
            isin,
            deployment.id.into(),
            Validity::UNBOUNDED,
            provenance()
        )
        .is_err()
    );
    // Only the token tokenizes; the stock depends on nothing onchain.
    assert!(
        relationships_from(&mut conn, stock.id.into(), None)
            .await
            .unwrap()
            .is_empty()
    );
    drop(conn);
    w.db.teardown().await;
}

#[tokio::test]
async fn stablecoins_are_never_fiat_and_edges_keep_their_endpoints() {
    let Some(w) = world().await else {
        return;
    };
    let usd = w.db.node(undrly_core::Category::Currency).await;
    let usdc = w.db.crypto_asset().await;
    let deployment =
        Deployment::on(&w.ethereum, DeploymentId::generate(), erc20(EVM_ADDRESS)).unwrap();
    let mut conn = w.db.pool.acquire().await.unwrap();
    insert_deployment(&mut conn, &deployment, w.record)
        .await
        .unwrap();
    drop(conn);

    let usd_id: CanonicalId = CurrencyId::from_uuid(usd).unwrap().into();
    let usdc_id: CanonicalId = InstrumentId::from_uuid(usdc).unwrap().into();
    // Core: a deployment represents an instrument (USDC), never a fiat
    // currency (USD); only deployments represent; nothing is DEPLOYED_ON.
    assert!(
        Relationship::new(
            deployment.id.into(),
            RelationshipType::Represents,
            usd_id,
            provenance()
        )
        .is_err()
    );
    assert!(
        Relationship::new(usdc_id, RelationshipType::Represents, usd_id, provenance()).is_err()
    );
    assert!(Relationship::new(usdc_id, RelationshipType::Tokenizes, usd_id, provenance()).is_err());
    assert!(
        Relationship::new(
            usdc_id,
            RelationshipType::AvailableOn,
            w.ethereum.id.into(),
            provenance()
        )
        .is_err()
    );
    // Database: the same rows are refused by relationship_rules.
    for (subject, subject_category, kind, object, object_category) in [
        (
            deployment.id.uuid(),
            "deployment",
            "REPRESENTS",
            usd,
            "currency",
        ),
        (usdc, "instrument", "REPRESENTS", usd, "currency"),
        (
            usdc,
            "instrument",
            "AVAILABLE_ON",
            w.ethereum.id.uuid(),
            "chain",
        ),
        (
            deployment.id.uuid(),
            "deployment",
            "DEPLOYED_ON",
            w.ethereum.id.uuid(),
            "chain",
        ),
    ] {
        let raw = sqlx::query(
            "INSERT INTO graph_edges (subject_id, subject_category, relationship_type, object_id,
               object_category, source_id, received_at, source_record_id)
             SELECT $1, $2, $3, $4, $5, source_id, received_at, id FROM source_records WHERE id = $6",
        )
        .bind(subject)
        .bind(subject_category)
        .bind(kind)
        .bind(object)
        .bind(object_category)
        .bind(w.db.record)
        .execute(&w.db.pool)
        .await;
        assert_rejected(
            raw,
            FOREIGN_KEY_VIOLATION,
            Some("graph_edges_relationship_type_subject_category_object_cate_fkey"),
        );
    }
    w.db.teardown().await;
}
