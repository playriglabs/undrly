//! Cross-market v1 (docs/hackathon-v1.md): curated universe + quote feeds →
//! observations → canonical quotes, from captured source fixtures. No
//! network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    CanonicalId, DisplayName, ObservationBasis, PriceSubject, PriceType, PriceUnit, Redistribution,
    Source, SourceId, Timestamp,
};
use undrly_ingest::RawRecord;
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::quotes::{ingest_quotes, refresh_canonical_quotes};
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::market::{get_canonical_quote, get_observation};
use undrly_store::sources::{facts_from_source_record, get_source_record, insert_source};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph};

pub const SOURCES: [&str; 5] = [
    "undrly-curated",
    "kraken",
    "hyperliquid",
    "gold-api",
    "alpaca",
];

fn repo(path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn raw(key: &str, payload: Vec<u8>, received_at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload,
        received_at: Timestamp::parse(received_at).unwrap(),
    }
}

/// Canonical id of a curated key, read from the dataset itself.
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

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in SOURCES {
        insert_source(
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
    let report = ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            repo("data/demo/universe.json"),
            "2026-09-25T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    assert_eq!(report.unchanged, 0, "fresh database: everything is new");
    Some((db, conn))
}

fn subject(key: &str) -> PriceSubject {
    let id = curated(key);
    match id.category() {
        undrly_core::Category::Currency => PriceSubject::Currency(id.try_into().unwrap()),
        _ => PriceSubject::Instrument(id.try_into().unwrap()),
    }
}

fn unit(key: &str) -> PriceUnit {
    let id = curated(key);
    match id.category() {
        undrly_core::Category::Currency => PriceUnit::Currency(id.try_into().unwrap()),
        _ => PriceUnit::Asset(id.try_into().unwrap()),
    }
}

#[tokio::test]
async fn curated_universe_is_idempotent_and_traceable() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let replay = ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            repo("data/demo/universe.json"),
            "2026-09-26T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    assert_eq!(replay.inserted, 0);
    let (record, write) = replay.source_record.unwrap();
    assert_eq!(write, Write::Unchanged);
    let facts = facts_from_source_record(&mut conn, record).await.unwrap();
    assert_eq!(
        facts.nodes.len(),
        2 + 1 + 4 + 5 + 1,
        "currencies, entity, venues, instruments, listing"
    );
    assert_eq!(facts.relationships.len(), 7);

    // BTC perpetual → DERIVES_FROM Bitcoin, SETTLES_IN USDC, TRADES_ON Hyperliquid.
    let edges: Vec<(String, CanonicalId)> =
        graph::relationships_from(&mut conn, curated("btc-perp"), None)
            .await
            .unwrap()
            .into_iter()
            .map(|e| {
                (
                    e.relationship.relationship_type().to_string(),
                    e.relationship.object(),
                )
            })
            .collect();
    assert_eq!(
        edges,
        vec![
            ("DERIVES_FROM".to_owned(), curated("btc")),
            ("SETTLES_IN".to_owned(), curated("usdc")),
            ("TRADES_ON".to_owned(), curated("hyperliquid")),
        ]
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn kraken_ticker_becomes_canonical_btc_and_eur_quotes() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let payload = repo("tests/fixtures/sources/kraken/ticker.json");
    let key = KrakenProvider::ticker_url(&["XXBTZUSD", "ZEURZUSD"]);
    let report = ingest_quotes(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        &raw(&key, payload.clone(), "2026-09-25T03:52:18.500Z"),
    )
    .await
    .unwrap();
    assert_eq!(report.observations.len(), 2);
    assert!(report.missing.is_empty());
    assert_eq!(
        get_source_record(&mut conn, report.source_record.0)
            .await
            .unwrap()
            .unwrap()
            .payload,
        payload,
        "raw bytes stored unchanged"
    );

    let refreshed = refresh_canonical_quotes(
        &mut conn,
        &report.pairs,
        Timestamp::parse("2026-09-25T03:52:19Z").unwrap(),
    )
    .await
    .unwrap();
    assert!(refreshed.iter().all(|r| r.eligible_count == 1));

    let btc = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    let o = get_observation(&mut conn, btc.observation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.price().to_string(), "84143.10000");
    assert_eq!(o.price_type(), PriceType::Last);
    assert_eq!(
        o.basis(),
        ObservationBasis::Venue(curated("kraken").try_into().unwrap())
    );
    assert_eq!(o.observed_at(), None, "Kraken states no time");
    let ba = o.bid_ask().unwrap();
    assert_eq!(
        (ba.bid.to_string(), ba.ask.to_string()),
        ("84145.90000".into(), "84146.00000".into())
    );

    // FX: EUR (a currency) priced in USD.
    let eur = get_canonical_quote(&mut conn, subject("eur"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    let o = get_observation(&mut conn, eur.observation)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(o.subject(), PriceSubject::Currency(_)));
    assert_eq!(o.price().to_string(), "1.13680");

    // Replaying the same response changes nothing.
    let replay = ingest_quotes(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        &raw(&key, payload, "2026-09-25T03:53:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(
        replay.source_record,
        (report.source_record.0, Write::Unchanged)
    );
    assert!(
        replay
            .observations
            .iter()
            .all(|(_, _, w)| *w == Write::Unchanged)
    );
    drop(conn);
    db.teardown().await;
}

/// Every representative market, from fixtures, through one path.
#[tokio::test]
async fn all_five_markets_have_canonical_quotes() {
    use undrly_normalize::alpaca::AlpacaNormalizer;
    use undrly_normalize::gold_api::GoldApiNormalizer;
    use undrly_normalize::hyperliquid::HyperliquidNormalizer;
    use undrly_provider::alpaca::AlpacaProvider;
    use undrly_provider::gold_api::GoldApiProvider;
    use undrly_provider::hyperliquid::HyperliquidProvider;

    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let t = "2026-09-25T03:52:18.500Z";
    let mut pairs = Vec::new();
    let kraken = ingest_quotes(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        &raw(
            "kraken",
            repo("tests/fixtures/sources/kraken/ticker.json"),
            t,
        ),
    )
    .await
    .unwrap();
    pairs.extend(kraken.pairs);
    let hl = ingest_quotes(
        &mut conn,
        &HyperliquidProvider::new(),
        &HyperliquidNormalizer,
        &raw(
            "hyperliquid",
            repo("tests/fixtures/sources/hyperliquid/metaAndAssetCtxs.json"),
            t,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        hl.observations.len(),
        1,
        "only the BTC feed, of every market"
    );
    pairs.extend(hl.pairs);
    let gold_payload = repo("tests/fixtures/sources/gold-api/price-XAU.json");
    let gold = ingest_quotes(
        &mut conn,
        &GoldApiProvider::new(),
        &GoldApiNormalizer,
        &raw("gold-api", gold_payload.clone(), t),
    )
    .await
    .unwrap();
    pairs.extend(gold.pairs);
    let nvda = ingest_quotes(
        &mut conn,
        &AlpacaProvider::new(),
        &AlpacaNormalizer,
        &raw(
            "alpaca",
            repo("tests/fixtures/sources/alpaca/snapshots-NVDA.json"),
            t,
        ),
    )
    .await
    .unwrap();
    pairs.extend(nvda.pairs);
    assert_eq!(pairs.len(), 5);
    refresh_canonical_quotes(&mut conn, &pairs, Timestamp::parse(t).unwrap())
        .await
        .unwrap();

    // BTC perpetual: mark price in USDC (an asset), at Hyperliquid.
    let perp = canonical(&mut conn, "btc-perp", "usdc").await;
    assert_eq!(perp.price_type(), PriceType::Mark);
    assert!(matches!(perp.unit(), PriceUnit::Asset(_)));
    assert_eq!(
        perp.basis(),
        ObservationBasis::Venue(curated("hyperliquid").try_into().unwrap())
    );
    // Gold: aggregated reference price with the source's time.
    let xau = canonical(&mut conn, "gold", "usd").await;
    assert_eq!(xau.basis(), ObservationBasis::Aggregated);
    assert_eq!(xau.price().to_string(), "4273.600098");
    assert_eq!(
        xau.observed_at().unwrap(),
        Timestamp::parse("2026-09-25T03:51:57Z").unwrap()
    );
    // NVDA: an IEX venue quote (venue IEX, source alpaca).
    let equity = canonical(&mut conn, "nvda", "usd").await;
    assert_eq!(
        equity.basis(),
        ObservationBasis::Venue(curated("iex").try_into().unwrap())
    );
    assert_eq!(equity.source_id().as_str(), "alpaca");
    assert_eq!(equity.price().to_string(), "223.71");
    assert_eq!(equity.price_type(), PriceType::Last);
    assert_eq!(
        equity.observed_at().unwrap(),
        Timestamp::parse("2026-09-24T20:45:15.183009Z").unwrap(),
        "IEX trade time, truncated from nanoseconds"
    );

    // gold-api restating the same update time in a new response (its
    // `updatedAtReadable` text changes) is a new raw record but the same
    // observation.
    let restated = String::from_utf8(gold_payload)
        .unwrap()
        .replace("a few seconds ago", "a minute ago");
    let again = ingest_quotes(
        &mut conn,
        &GoldApiProvider::new(),
        &GoldApiNormalizer,
        &raw("gold-api", restated.into_bytes(), "2026-09-25T03:53:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(again.source_record.1, Write::Inserted);
    assert_eq!(again.observations[0].1, gold.observations[0].1);
    assert_eq!(again.observations[0].2, Write::Unchanged);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn bad_quote_payloads_write_nothing() {
    use undrly_normalize::gold_api::GoldApiNormalizer;
    use undrly_provider::gold_api::GoldApiProvider;

    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let before = count(&mut conn).await;
    for payload in [
        &b"<html>502</html>"[..],
        br#"{"symbol":"XAU","currency":"EUR","price":1.5,"updatedAt":"2026-09-25T03:51:57Z"}"#,
        br#"{"symbol":"XAU","currency":"USD","price":1e3,"updatedAt":"2026-09-25T03:51:57Z"}"#,
    ] {
        let result = ingest_quotes(
            &mut conn,
            &GoldApiProvider::new(),
            &GoldApiNormalizer,
            &raw("gold-api", payload.to_vec(), "2026-09-25T03:52:18Z"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(count(&mut conn).await, before);
    }
    drop(conn);
    db.teardown().await;
}

async fn canonical(
    conn: &mut sqlx::PgConnection,
    s: &str,
    u: &str,
) -> undrly_core::MarketObservation {
    let q = get_canonical_quote(conn, subject(s), unit(u))
        .await
        .unwrap()
        .unwrap();
    get_observation(conn, q.observation).await.unwrap().unwrap()
}

async fn count(conn: &mut sqlx::PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM source_records) + (SELECT count(*) FROM market_observations)",
    )
    .fetch_one(conn)
    .await
    .unwrap()
}
