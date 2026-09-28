//! V1.6 (docs/v1.6-robinhood-chain.md): a tracker certificate issued as a
//! token (the NVIDIA Stock Token), over the V1 curated universe whose NVIDIA
//! common stock carries ISIN US67066G1040. The chain id responses and the
//! issuer registry are captured fixtures; the Final Terms document is replaced
//! by synthetic bytes with their own hash (the ingest only checks the hash).
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use sha2::{Digest, Sha256};
use undrly_core::{
    Caip2, CanonicalId, Category, CurrencyCode, DisplayName, InstrumentClass, Isin, Lei,
    Redistribution, RelationshipType, Source, SourceId, Timestamp,
};
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::onchain::{ChainBinding, ingest_evm_chain};
use undrly_ingest::tracker::{
    TrackerBinding, ingest_tracker_deployment, ingest_tracker_final_terms,
};
use undrly_ingest::{IngestError, RawRecord, Resolution};
use undrly_store::identifiers::identifiers_for_node;
use undrly_store::reference::{chain_by_caip2, get_deployment, get_entity, get_instrument};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph};

const FINAL_TERMS: &[u8] = b"%PDF-1.7 synthetic Final Terms fixture";
const NVDA_ISIN: &str = "US67066G1040";
const PRODUCT_ISIN: &str = "JE00BX9C6J83";
const RHJ_LEI: &str = "984500ADFHQZ9D6B9A29";
const MAINNET: &str = "eip155:4663";
const CONTRACT: &str = "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec";

fn repo(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(path),
    )
    .unwrap()
}

fn raw(key: &str, payload: Vec<u8>, at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload,
        received_at: Timestamp::parse(at).unwrap(),
    }
}

fn curated(key: &str) -> CanonicalId {
    let u: serde_json::Value = serde_json::from_slice(&repo("data/demo/universe.json")).unwrap();
    for group in [
        "currencies",
        "entities",
        "venues",
        "instruments",
        "listings",
    ] {
        for o in u[group].as_array().unwrap() {
            if o["key"] == key {
                return CanonicalId::parse(o["id"].as_str().unwrap()).unwrap();
            }
        }
    }
    panic!("no curated key {key}")
}

fn binding() -> TrackerBinding {
    TrackerBinding {
        final_terms_sha256: Sha256::digest(FINAL_TERMS)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        issuer_name: DisplayName::new("Robinhood Assets (Jersey) Limited").unwrap(),
        issuer_lei: Lei::parse(RHJ_LEI).unwrap(),
        product_name: DisplayName::new("NVIDIA • Robinhood Token (RHJ Series 1)").unwrap(),
        product_isin: Isin::parse(PRODUCT_ISIN).unwrap(),
        underlying_isin: Isin::parse(NVDA_ISIN).unwrap(),
        currency: CurrencyCode::parse("USD").unwrap(),
        chain: Caip2::parse(MAINNET).unwrap(),
    }
}

fn final_terms() -> RawRecord {
    raw(
        "https://cdn.robinhood.com/assets/robinhood/legal/rhj_final_terms_for_tokenised_debt_securities_linked_to_nvidia.pdf",
        FINAL_TERMS.to_vec(),
        "2026-09-28T15:00:00Z",
    )
}

fn chain_id(network: &str) -> RawRecord {
    raw(
        "POST https://rpc.mainnet.chain.robinhood.com {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"eth_chainId\",\"params\":[]}",
        repo(&format!(
            "tests/fixtures/sources/robinhood-chain/eth_chainId-{network}.json"
        )),
        "2026-09-28T15:00:01Z",
    )
}

fn registry() -> RawRecord {
    raw(
        "https://api.robinhood.com/rhj/assets",
        repo("tests/fixtures/sources/rhj/assets.json"),
        "2026-09-28T15:00:02Z",
    )
}

fn robinhood_chain() -> ChainBinding {
    ChainBinding {
        name: DisplayName::new("Robinhood Chain").unwrap(),
        caip2: Caip2::parse(MAINNET).unwrap(),
    }
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in [
        "undrly-curated",
        "robinhood-chain-rpc",
        "rhj-api",
        "rhj-final-terms",
        // Sources the V1 curated file declares feeds for.
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
            repo("data/demo/universe.json"),
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

async fn edges(
    conn: &mut sqlx::PgConnection,
    subject: CanonicalId,
) -> Vec<(RelationshipType, CanonicalId, String)> {
    graph::relationships_from(conn, subject, None)
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
        .collect()
}

#[tokio::test]
async fn the_token_is_its_own_security_tracking_the_share() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let nvda = curated("nvda");
    let nvidia = curated("nvidia");
    let usd = curated("usd");

    let report = ingest_tracker_final_terms(&mut conn, &final_terms(), &binding())
        .await
        .unwrap();
    assert!(matches!(report.product, Resolution::Created(_)));
    assert!(matches!(report.issuer, Resolution::Created(_)));
    assert_eq!(report.underlying.map(CanonicalId::from), Some(nvda));
    let product: CanonicalId = report.product.id().into();
    let issuer: CanonicalId = report.issuer.id().into();

    // Distinct objects: token ≠ share; token issuer ≠ share issuer.
    assert_ne!(product, nvda);
    assert_ne!(issuer, nvidia);
    let token = get_instrument(&mut conn, report.product.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token.class, InstrumentClass::TokenizedSecurity);
    assert_eq!(
        get_entity(&mut conn, report.issuer.id())
            .await
            .unwrap()
            .unwrap()
            .name
            .as_str(),
        "Robinhood Assets (Jersey) Limited"
    );

    // Its own facts, each asserted by the Final Terms record.
    assert_eq!(
        edges(&mut conn, product).await,
        vec![
            (RelationshipType::IssuedBy, issuer, "rhj-final-terms".into()),
            (RelationshipType::Tracks, nvda, "rhj-final-terms".into()),
            (
                RelationshipType::DenominatedIn,
                usd,
                "rhj-final-terms".into()
            ),
            (RelationshipType::SettlesIn, usd, "rhj-final-terms".into()),
        ]
    );
    // The share keeps its own issuer and gains nothing from the token.
    let share_edges = edges(&mut conn, nvda).await;
    assert!(share_edges.contains(&(RelationshipType::IssuedBy, nvidia, "undrly-curated".into())));
    assert!(
        share_edges
            .iter()
            .all(|(t, o, _)| !(*t == RelationshipType::IssuedBy && *o == issuer))
    );

    // Identifiers stay on their own objects.
    let ids = |v: Vec<undrly_store::identifiers::StoredIdentifier>| -> Vec<String> {
        v.iter()
            .map(|s| {
                let i = s.assignment.identifier();
                format!("{}:{}", i.namespace().as_str(), i.value())
            })
            .collect()
    };
    let token_ids = ids(identifiers_for_node(&mut conn, product).await.unwrap());
    assert_eq!(token_ids, vec![format!("isin:{PRODUCT_ISIN}")]);
    let share_ids = ids(identifiers_for_node(&mut conn, nvda).await.unwrap());
    assert!(share_ids.contains(&format!("isin:{NVDA_ISIN}")));
    assert!(!share_ids.contains(&format!("isin:{PRODUCT_ISIN}")));
    let issuer_ids = ids(identifiers_for_node(&mut conn, issuer).await.unwrap());
    assert_eq!(issuer_ids, vec![format!("lei:{RHJ_LEI}")]);

    // Replay: nothing changes, the same nodes are found.
    let again = ingest_tracker_final_terms(&mut conn, &final_terms(), &binding())
        .await
        .unwrap();
    assert_eq!(again.source_record.1, Write::Unchanged);
    assert!(matches!(again.product, Resolution::Existing(_)));
    assert!(
        again
            .relationships
            .iter()
            .all(|(_, w)| *w == Write::Unchanged)
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn the_deployment_on_robinhood_chain_represents_the_token() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest_evm_chain(
        &mut conn,
        &chain_id("mainnet"),
        "robinhood-chain-rpc",
        &robinhood_chain(),
    )
    .await
    .unwrap();
    let chain = chain_by_caip2(&mut conn, &Caip2::parse(MAINNET).unwrap())
        .await
        .unwrap()
        .unwrap();
    // The deployment needs its product first.
    assert!(matches!(
        ingest_tracker_deployment(&mut conn, &registry(), &binding()).await,
        Err(IngestError::CuratedDisagrees(_))
    ));
    let product = ingest_tracker_final_terms(&mut conn, &final_terms(), &binding())
        .await
        .unwrap()
        .product
        .id();
    let d = ingest_tracker_deployment(&mut conn, &registry(), &binding())
        .await
        .unwrap();
    assert_eq!(d.caip19, format!("{MAINNET}/erc20:{CONTRACT}"));
    assert!(d.others.is_empty());
    let deployment = get_deployment(&mut conn, d.deployment.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(deployment.chain_id, chain.id);
    assert_eq!(
        CanonicalId::from(deployment.id).category(),
        Category::Deployment
    );
    // Deployment ≠ token ≠ share; REPRESENTS the token (not the share),
    // asserted by the registry record.
    assert_eq!(
        edges(&mut conn, d.deployment.id().into()).await,
        vec![(
            RelationshipType::Represents,
            product.into(),
            "rhj-api".into()
        )]
    );
    assert_ne!(CanonicalId::from(product), curated("nvda"));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn nothing_is_written_without_the_reviewed_document_or_the_right_chain() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let records = count(&mut conn, "source_records").await;
    // Different bytes than reviewed: refused before anything is stored.
    let mut changed = final_terms();
    changed.payload = b"%PDF-1.7 a revised document".to_vec();
    assert!(matches!(
        ingest_tracker_final_terms(&mut conn, &changed, &binding()).await,
        Err(IngestError::CuratedDisagrees(ref m)) if m.contains("review")
    ));
    // The testnet answer is never Robinhood Chain mainnet.
    assert!(matches!(
        ingest_evm_chain(&mut conn, &chain_id("testnet"), "robinhood-chain-rpc", &robinhood_chain())
            .await,
        Err(IngestError::UnexpectedChain { ref found, .. }) if found == "eip155:46630"
    ));
    assert_eq!(count(&mut conn, "source_records").await, records);
    assert_eq!(count(&mut conn, "chains").await, 0);
    assert_eq!(count(&mut conn, "instruments").await, 7, "V1's only");
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn identifiers_never_move_between_the_share_and_the_token() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    // A binding that claims the share's ISIN for the product is refused:
    // that ISIN names NVIDIA common stock, which is not a tokenized security.
    let mut wrong = binding();
    wrong.product_isin = Isin::parse(NVDA_ISIN).unwrap();
    assert!(matches!(
        ingest_tracker_final_terms(&mut conn, &final_terms(), &wrong).await,
        Err(IngestError::CuratedDisagrees(_))
    ));
    assert_eq!(
        count(&mut conn, "graph_edges WHERE relationship_type = 'TRACKS'").await,
        0
    );

    // An underlying Undrly cannot find by ISIN (Apple's; V1.1 AAPL has none)
    // leaves TRACKS unasserted: never matched by ticker.
    let mut apple = binding();
    apple.product_isin = Isin::parse("JE00BX9H9M76").unwrap();
    apple.underlying_isin = Isin::parse("US0378331005").unwrap();
    let r = ingest_tracker_final_terms(&mut conn, &final_terms(), &apple)
        .await
        .unwrap();
    assert_eq!(r.underlying, None);
    assert!(
        r.relationships
            .iter()
            .all(|(t, _)| *t != RelationshipType::Tracks)
    );
    drop(conn);
    db.teardown().await;
}
