//! V1.5 (docs/v1.5-solana.md): Solana mainnet from its own genesis hash and
//! Circle's USDC mint from Circle's own address list, over the V1 curated
//! universe (whose USD Coin is reused). Captured responses only; no network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AssetNamespace, Caip2, CanonicalId, Category, ChainAsset, ChainNamespace, DisplayName,
    InstrumentId, Redistribution, RelationshipType, Source, SourceId, Timestamp,
};
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::onchain::{
    ChainBinding, IssuerBinding, IssuerRow, ingest_circle_usdc, ingest_solana_chain,
};
use undrly_ingest::{IngestError, RawRecord, Resolution};
use undrly_store::reference::{chain_by_caip2, deployment_by_asset, get_deployment};
use undrly_store::sources::get_source_record;
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph};

const MAINNET: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
const DEVNET: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const CIRCLE_URL: &str = "https://developers.circle.com/stablecoins/usdc-contract-addresses.md";

fn repo(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(path),
    )
    .unwrap()
}

fn raw(key: &str, path: &str, at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload: repo(path),
        received_at: Timestamp::parse(at).unwrap(),
    }
}

fn curated(key: &str) -> CanonicalId {
    let u: serde_json::Value = serde_json::from_slice(&repo("data/demo/universe.json")).unwrap();
    for group in ["currencies", "instruments"] {
        for o in u[group].as_array().unwrap() {
            if o["key"] == key {
                return CanonicalId::parse(o["id"].as_str().unwrap()).unwrap();
            }
        }
    }
    panic!("no curated key {key}")
}

fn genesis(cluster: &str) -> RawRecord {
    raw(
        "POST https://api.mainnet.solana.com {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getGenesisHash\"}",
        &format!("tests/fixtures/sources/solana/getGenesisHash-{cluster}.json"),
        "2026-09-28T12:00:00Z",
    )
}

fn circle_page() -> RawRecord {
    raw(
        CIRCLE_URL,
        "tests/fixtures/sources/circle/usdc-contract-addresses.md",
        "2026-09-28T12:00:01Z",
    )
}

fn solana_binding() -> ChainBinding {
    ChainBinding {
        name: DisplayName::new("Solana").unwrap(),
        caip2: Caip2::parse(MAINNET).unwrap(),
    }
}

fn usdc_binding() -> IssuerBinding {
    IssuerBinding {
        instrument: curated("usdc").try_into().unwrap(),
        deployments: vec![IssuerRow {
            label: "Solana".into(),
            chain: Caip2::parse(MAINNET).unwrap(),
            namespace: AssetNamespace::Token,
        }],
    }
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in [
        "undrly-curated",
        "circle",
        "solana-mainnet-rpc",
        "kraken",
        "coinbase",
        "hyperliquid",
        "gold-api",
        "alpaca",
    ] {
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
    ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            "data/demo/universe.json",
            "2026-09-26T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    Some((db, conn))
}

async fn count(conn: &mut sqlx::PgConnection, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(conn)
        .await
        .unwrap()
}

#[tokio::test]
async fn solana_usdc_is_a_deployment_of_the_existing_usd_coin() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let instruments_before = count(&mut conn, "instruments").await;

    let chain = ingest_solana_chain(&mut conn, &genesis("mainnet"), &solana_binding())
        .await
        .unwrap();
    assert!(matches!(chain.chain, Resolution::Created(_)));
    let solana = chain_by_caip2(&mut conn, &Caip2::parse(MAINNET).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(solana.caip2.to_string(), MAINNET);
    assert_eq!(solana.name.as_str(), "Solana");

    let report = ingest_circle_usdc(&mut conn, &circle_page(), &usdc_binding())
        .await
        .unwrap();
    let [(deployment, caip19, Write::Inserted, Write::Inserted)] = report.deployments.as_slice()
    else {
        panic!("{report:?}");
    };
    assert_eq!(caip19, &format!("{MAINNET}/token:{USDC_MINT}"));
    assert!(report.others.is_empty());

    // No new economic asset: the deployment is its own node, USD Coin is V1's.
    assert_eq!(count(&mut conn, "instruments").await, instruments_before);
    let usdc = curated("usdc");
    assert_ne!(CanonicalId::from(deployment.id()), usdc);
    assert_eq!(
        CanonicalId::from(deployment.id()).category(),
        Category::Deployment
    );
    let stored = get_deployment(&mut conn, deployment.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.chain_id, solana.id);
    assert_eq!(stored.asset.reference(), USDC_MINT);

    // REPRESENTS USD Coin, asserted by Circle's raw record (provenance), and
    // nothing asserts USD, USDT or anything else.
    let edges = graph::relationships_from(&mut conn, deployment.id().into(), None)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1);
    let edge = &edges[0];
    assert_eq!(
        edge.relationship.relationship_type(),
        RelationshipType::Represents
    );
    assert_eq!(edge.relationship.object(), usdc);
    assert_ne!(edge.relationship.object(), curated("usd"));
    assert_ne!(edge.relationship.object(), curated("usdt"));
    assert_eq!(edge.relationship.provenance().source_id.as_str(), "circle");
    assert_eq!(edge.source_record, report.source_record.0);
    let record = get_source_record(&mut conn, report.source_record.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.record_key, CIRCLE_URL);
    assert_eq!(
        record.payload,
        repo("tests/fixtures/sources/circle/usdc-contract-addresses.md")
    );
    // The chain names the RPC response it was derived from.
    let chain_record: i64 = sqlx::query_scalar("SELECT source_record_id FROM chains WHERE id = $1")
        .bind(solana.id.uuid())
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(chain_record, chain.source_record.0.0);

    // Replays change nothing.
    let again = ingest_circle_usdc(&mut conn, &circle_page(), &usdc_binding())
        .await
        .unwrap();
    assert_eq!(again.source_record.1, Write::Unchanged);
    assert!(matches!(
        again.deployments[0],
        (
            Resolution::Existing(_),
            _,
            Write::Unchanged,
            Write::Unchanged
        )
    ));
    let again = ingest_solana_chain(&mut conn, &genesis("mainnet"), &solana_binding())
        .await
        .unwrap();
    assert_eq!(again.chain, Resolution::Existing(solana.id));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn devnet_is_never_recorded_as_mainnet() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let records = count(&mut conn, "source_records").await;
    let err = ingest_solana_chain(&mut conn, &genesis("devnet"), &solana_binding())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, IngestError::UnexpectedChain { expected, found }
            if expected == MAINNET && found == DEVNET),
        "{err}"
    );
    assert_eq!(count(&mut conn, "chains").await, 0);
    assert_eq!(
        count(&mut conn, "source_records").await,
        records,
        "the devnet response is not stored"
    );

    // Without the chain from its own source, no deployment is written.
    let err = ingest_circle_usdc(&mut conn, &circle_page(), &usdc_binding())
        .await
        .unwrap_err();
    assert!(
        matches!(err, IngestError::MissingChain(ref c) if c == MAINNET),
        "{err}"
    );
    assert_eq!(count(&mut conn, "deployments").await, 0);
    assert_eq!(count(&mut conn, "source_records").await, records);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn the_same_mint_on_another_cluster_is_another_deployment() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest_solana_chain(&mut conn, &genesis("mainnet"), &solana_binding())
        .await
        .unwrap();
    // Devnet, bound as devnet (as a test fixture only).
    let devnet = ChainBinding {
        name: DisplayName::new("Solana Devnet (fixture)").unwrap(),
        caip2: Caip2::parse(DEVNET).unwrap(),
    };
    ingest_solana_chain(&mut conn, &genesis("devnet"), &devnet)
        .await
        .unwrap();
    let mut binding = usdc_binding();
    binding.deployments.push(IssuerRow {
        label: "Solana".into(),
        chain: Caip2::parse(DEVNET).unwrap(),
        namespace: AssetNamespace::Token,
    });
    let report = ingest_circle_usdc(&mut conn, &circle_page(), &binding)
        .await
        .unwrap();
    let ids: Vec<_> = report.deployments.iter().map(|(d, ..)| d.id()).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "same mint, two clusters, two deployments");

    // Lookup needs the chain; a mint is never an EVM address.
    let asset = ChainAsset::new(ChainNamespace::Solana, AssetNamespace::Token, USDC_MINT).unwrap();
    let mainnet = chain_by_caip2(&mut conn, &Caip2::parse(MAINNET).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        deployment_by_asset(&mut conn, mainnet.id, &asset)
            .await
            .unwrap()
            .map(|d| d.id),
        Some(ids[0])
    );
    assert!(ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, USDC_MINT).is_err());
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn a_binding_to_a_missing_row_or_a_non_asset_writes_nothing() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest_solana_chain(&mut conn, &genesis("mainnet"), &solana_binding())
        .await
        .unwrap();
    let records = count(&mut conn, "source_records").await;
    // A label the page does not have (labels match exactly; no guessing).
    let mut binding = usdc_binding();
    binding.deployments[0].label = "solana".into();
    assert!(
        ingest_circle_usdc(&mut conn, &circle_page(), &binding)
            .await
            .is_err()
    );
    // An instrument that is not a crypto asset (the BTC perpetual).
    let mut binding = usdc_binding();
    binding.instrument = InstrumentId::try_from(curated("btc-perp")).unwrap();
    assert!(
        ingest_circle_usdc(&mut conn, &circle_page(), &binding)
            .await
            .is_err()
    );
    assert_eq!(count(&mut conn, "source_records").await, records);
    assert_eq!(count(&mut conn, "deployments").await, 0);
    drop(conn);
    db.teardown().await;
}
