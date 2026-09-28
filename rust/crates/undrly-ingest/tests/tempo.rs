//! V1.7 (docs/v1.7-tempo.md): Tempo Mainnet from its own `eth_chainId`, and
//! pathUSD from the chain's own answers about its predeploy, over the V1
//! curated universe (USD, USD Coin, Tether). Captured responses only.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AssetNamespace, Caip2, CanonicalId, Category, ChainAsset, ChainNamespace, CurrencyCode,
    DisplayName, InstrumentClass, Redistribution, RelationshipType, Source, SourceId, Timestamp,
};
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::onchain::{ChainBinding, Tip20Binding, ingest_evm_chain, ingest_tip20_asset};
use undrly_ingest::{IngestError, RawRecord, Resolution};
use undrly_store::reference::{get_deployment, get_instrument};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph};

const MAINNET: &str = "eip155:4217";
const PATH_USD: &str = "0x20c0000000000000000000000000000000000000";

fn repo(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(path),
    )
    .unwrap()
}

fn raw(key: &str, file: &str, at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload: repo(&format!("tests/fixtures/sources/tempo/{file}")),
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

fn tempo() -> ChainBinding {
    ChainBinding {
        name: DisplayName::new("Tempo").unwrap(),
        caip2: Caip2::parse(MAINNET).unwrap(),
    }
}

fn path_usd() -> Tip20Binding {
    Tip20Binding {
        chain: Caip2::parse(MAINNET).unwrap(),
        asset: ChainAsset::new(ChainNamespace::Eip155, AssetNamespace::Erc20, PATH_USD).unwrap(),
        name: "pathUSD".into(),
        symbol: "pathUSD".into(),
        currency: CurrencyCode::parse("USD").unwrap(),
        decimals: 6,
        instrument_name: DisplayName::new("pathUSD").unwrap(),
    }
}

fn metadata(network: &str) -> RawRecord {
    raw(
        &format!("POST https://rpc.tempo.xyz tip20 {PATH_USD} ({network})"),
        &format!("tip20-pathUSD-{network}.json"),
        "2026-09-28T16:00:00Z",
    )
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in [
        "undrly-curated",
        "tempo-rpc",
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
        &RawRecord {
            record_key: "data/demo/universe.json".into(),
            payload: repo("data/demo/universe.json"),
            received_at: Timestamp::parse("2026-09-26T00:00:00Z").unwrap(),
        },
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
async fn path_usd_is_its_own_asset_that_tracks_usd() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let chain = ingest_evm_chain(
        &mut conn,
        &raw(
            "POST https://rpc.tempo.xyz eth_chainId",
            "eth_chainId-mainnet.json",
            "2026-09-28T15:59:59Z",
        ),
        "tempo-rpc",
        &tempo(),
    )
    .await
    .unwrap();
    let r = ingest_tip20_asset(&mut conn, &metadata("mainnet"), "tempo-rpc", &path_usd())
        .await
        .unwrap();
    assert!(matches!(r.instrument, Resolution::Created(_)));
    assert_eq!(r.caip19, format!("{MAINNET}/erc20:{PATH_USD}"));
    let asset: CanonicalId = r.instrument.id().into();

    // A crypto asset of its own: not USD, not USD Coin, not Tether.
    let (usd, usdc, usdt) = (curated("usd"), curated("usdc"), curated("usdt"));
    assert_eq!(usd.category(), Category::Currency);
    for other in [usd, usdc, usdt] {
        assert_ne!(asset, other);
    }
    let i = get_instrument(&mut conn, r.instrument.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (i.class, i.name.as_str()),
        (InstrumentClass::CryptoAsset, "pathUSD")
    );

    // pathUSD TRACKS USD (its declared reference currency); nothing else,
    // and no issuer (unresolved), all asserted by the chain's own record.
    let out: Vec<(RelationshipType, CanonicalId, String)> =
        graph::relationships_from(&mut conn, asset, None)
            .await
            .unwrap()
            .into_iter()
            .map(|e| {
                (
                    e.relationship.relationship_type(),
                    e.relationship.object(),
                    e.relationship.provenance().source_id.as_str().to_owned(),
                )
            })
            .collect();
    assert_eq!(
        out,
        vec![(RelationshipType::Tracks, usd, "tempo-rpc".into())]
    );
    // Its deployment on Tempo represents it; nothing represents USD Coin here.
    let d = get_deployment(&mut conn, r.deployment.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d.chain_id, chain.chain.id());
    let reps = graph::relationships_from(&mut conn, d.id.into(), None)
        .await
        .unwrap();
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].relationship.object(), asset);
    assert_eq!(reps[0].source_record, r.source_record.0);
    assert!(
        graph::relationships_to(&mut conn, usdc, Some(RelationshipType::Represents))
            .await
            .unwrap()
            .is_empty()
    );

    // Replay: the same asset is found through its deployment; nothing changes.
    let again = ingest_tip20_asset(&mut conn, &metadata("mainnet"), "tempo-rpc", &path_usd())
        .await
        .unwrap();
    assert_eq!(again.instrument, Resolution::Existing(r.instrument.id()));
    assert!(again.writes.iter().all(|(_, w)| *w == Write::Unchanged));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn testnet_and_mismatched_metadata_are_never_written() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let records = count(&mut conn, "source_records").await;
    // The testnet chain id is not Tempo Mainnet.
    assert!(matches!(
        ingest_evm_chain(
            &mut conn,
            &raw("POST https://rpc.moderato.tempo.xyz eth_chainId", "eth_chainId-testnet.json", "2026-09-28T16:00:00Z"),
            "tempo-rpc",
            &tempo(),
        )
        .await,
        Err(IngestError::UnexpectedChain { ref found, .. }) if found == "eip155:42431"
    ));
    // Testnet pathUSD (same address, "PathUSD") is refused as mainnet.
    assert!(matches!(
        ingest_tip20_asset(&mut conn, &metadata("testnet"), "tempo-rpc", &path_usd()).await,
        Err(IngestError::UnexpectedChain { .. })
    ));
    // A binding the chain contradicts (another currency) fails closed.
    let mut eur = path_usd();
    eur.currency = CurrencyCode::parse("EUR").unwrap();
    assert!(matches!(
        ingest_tip20_asset(&mut conn, &metadata("mainnet"), "tempo-rpc", &eur).await,
        Err(IngestError::CuratedDisagrees(ref m)) if m.contains("review")
    ));
    // Without the chain from its own source, no deployment.
    assert!(matches!(
        ingest_tip20_asset(&mut conn, &metadata("mainnet"), "tempo-rpc", &path_usd()).await,
        Err(IngestError::MissingChain(_))
    ));
    assert_eq!(count(&mut conn, "source_records").await, records);
    assert_eq!(count(&mut conn, "deployments").await, 0);
    assert_eq!(count(&mut conn, "instruments").await, 7, "V1's only");
    drop(conn);
    db.teardown().await;
}
