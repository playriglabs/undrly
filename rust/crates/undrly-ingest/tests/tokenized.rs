//! V1.10 (docs/v1.10-tokenized-stocks.md): a tokenized product's deployment
//! from its issuer's registry record, ingested after its chain.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use sha2::{Digest, Sha256};
use undrly_core::{
    AssetNamespace, Caip2, CanonicalId, ChainAsset, ChainNamespace, DeploymentId, DisplayName,
    Instrument, InstrumentClass, InstrumentId, Redistribution, RelationshipType, Source, SourceId,
    Timestamp,
};
use undrly_ingest::onchain::{
    ChainBinding, RegistryDeployment, RegistryDeploymentOutcome, ingest_registry_deployment,
    ingest_solana_chain,
};
use undrly_ingest::{IngestError, RawRecord, store_raw_record};
use undrly_store::testing::fresh;
use undrly_store::{Write, graph, reference};

const MAINNET: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const MINT: &str = "XsbEhLAtcf6HdfpFZ5xEMdqW8nfAvcsP5bdudRLJzJp";
const REGISTRY: &str = "https://api.backed.fi/api/v1/token";

fn genesis() -> RawRecord {
    RawRecord {
        record_key: "POST https://api.mainnet.solana.com {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getGenesisHash\"}".into(),
        payload: std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sources/solana/getGenesisHash-mainnet.json"),
        )
        .unwrap(),
        received_at: Timestamp::parse("2026-10-04T12:00:00Z").unwrap(),
    }
}

#[tokio::test]
async fn a_registry_deployment_represents_its_product_with_the_registry_as_provenance() {
    let Some(db) = fresh().await else {
        return;
    };
    let mut conn = db.pool.acquire().await.unwrap();
    for id in ["backed-api", "solana-mainnet-rpc"] {
        undrly_store::sources::insert_source(
            &mut conn,
            &Source {
                id: SourceId::parse(id).unwrap(),
                name: DisplayName::new(id).unwrap(),
                redistribution: Redistribution::Unknown,
            },
        )
        .await
        .unwrap();
    }
    // The registry record, as `seed` stores it, and the product the snapshot
    // declared from it.
    let payload = br#"{"nodes":[],"page":{"hasNextPage":false}}"#.to_vec();
    let sha256: [u8; 32] = Sha256::digest(&payload).into();
    let source = SourceId::parse("backed-api").unwrap();
    let (record, _) = store_raw_record(
        &mut conn,
        &source,
        &RawRecord {
            record_key: REGISTRY.into(),
            payload,
            received_at: Timestamp::parse("2026-10-04T11:00:00Z").unwrap(),
        },
    )
    .await
    .unwrap();
    let product = InstrumentId::generate();
    reference::insert_instrument(
        &mut conn,
        &Instrument {
            id: product,
            class: InstrumentClass::TokenizedSecurity,
            name: DisplayName::new("Example xStock").unwrap(),
            contract_multiplier: None,
            unit_of_measure: None,
            fx_pair: None,
        },
        record.id,
    )
    .await
    .unwrap();
    let row = RegistryDeployment {
        id: DeploymentId::generate(),
        chain: Caip2::parse(MAINNET).unwrap(),
        asset: ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Token, MINT).unwrap(),
        product,
        source: source.clone(),
        record_key: REGISTRY.into(),
        sha256,
    };

    // Before its chain: nothing written.
    assert_eq!(
        ingest_registry_deployment(&mut conn, &row).await.unwrap(),
        RegistryDeploymentOutcome::NoChain
    );
    ingest_solana_chain(
        &mut conn,
        &genesis(),
        &ChainBinding {
            name: DisplayName::new("Solana").unwrap(),
            caip2: Caip2::parse(MAINNET).unwrap(),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        ingest_registry_deployment(&mut conn, &row).await.unwrap(),
        RegistryDeploymentOutcome::Written {
            deployment: Write::Inserted,
            represents: Write::Inserted
        }
    );
    let edges = graph::relationships_from(&mut conn, row.id.into(), None)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].relationship.relationship_type(),
        RelationshipType::Represents
    );
    assert_eq!(edges[0].relationship.object(), CanonicalId::from(product));
    assert_eq!(
        edges[0].source_record, record.id,
        "asserted by the registry"
    );

    // Replay: unchanged. Another id for the same mint: a conflict, not a
    // second deployment.
    assert_eq!(
        ingest_registry_deployment(&mut conn, &row).await.unwrap(),
        RegistryDeploymentOutcome::Written {
            deployment: Write::Unchanged,
            represents: Write::Unchanged
        }
    );
    let other = RegistryDeployment {
        id: DeploymentId::generate(),
        ..row.clone()
    };
    assert_eq!(
        ingest_registry_deployment(&mut conn, &other).await.unwrap(),
        RegistryDeploymentOutcome::Conflict(row.id)
    );

    // An unstored registry record or a product that is not a tokenized
    // security writes nothing.
    let unseeded = RegistryDeployment {
        sha256: [0; 32],
        ..row.clone()
    };
    assert!(matches!(
        ingest_registry_deployment(&mut conn, &unseeded).await,
        Err(IngestError::CuratedDisagrees(_))
    ));
    let missing = RegistryDeployment {
        product: InstrumentId::generate(),
        ..row.clone()
    };
    assert!(matches!(
        ingest_registry_deployment(&mut conn, &missing).await,
        Err(IngestError::MissingObject(_))
    ));
    drop(conn);
    db.teardown().await;
}
