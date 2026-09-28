//! Cross-market v1 (docs/hackathon-v1.md): curated universe + quote feeds →
//! observations → canonical quotes, from captured source fixtures. No
//! network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AggregationMethod, CanonicalId, DisplayName, ObservationBasis, PriceSubject, PriceType,
    PriceUnit, Redistribution, Source, SourceId, Timestamp, VenueSymbol,
};
use undrly_ingest::RawRecord;
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::quotes::{QuoteIngestReport, ingest_quotes, refresh_canonical_quotes};
use undrly_normalize::coinbase::CoinbaseNormalizer;
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_provider::coinbase::CoinbaseProvider;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::market::{
    ObservationId, get_canonical_quote, get_observation, latest_observations,
};
use undrly_store::sources::{facts_from_source_record, get_source_record, insert_source};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph};

pub const SOURCES: [&str; 6] = [
    "coinbase",
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
        2 + 1 + 5 + 7 + 1,
        "currencies, entity, venues, instruments (incl. the EUR/USD FX market and Tether), listing"
    );
    assert_eq!(facts.relationships.len(), 10);

    // BTC perpetual (Hyperliquid's contract specification): derives from
    // Bitcoin, is priced in USDT, pays its cash flows in and is margined in
    // USDC, trades on Hyperliquid. USD, USDT and USDC are three nodes.
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
            ("DENOMINATED_IN".to_owned(), curated("usdt")),
            ("SETTLES_IN".to_owned(), curated("usdc")),
            ("MARGINED_IN".to_owned(), curated("usdc")),
            ("TRADES_ON".to_owned(), curated("hyperliquid")),
        ]
    );
    let stablecoins = [curated("usd"), curated("usdt"), curated("usdc")];
    assert_eq!(
        stablecoins
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(curated("usd").category(), undrly_core::Category::Currency);
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
    assert!(refreshed.iter().all(|r| r.inputs.len() == 1));

    // BTC/USD is declared `mean-venue-mid-v1`: with Kraken alone, the
    // canonical quote is Kraken's mid, labelled aggregated (one input).
    let btc = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(btc.method, AggregationMethod::MeanVenueMidV1);
    assert_eq!(btc.basis, ObservationBasis::Aggregated);
    assert_eq!(btc.price.to_string(), "84145.9500000");
    assert_eq!(btc.inputs.len(), 1);
    // The Kraken venue observation itself is kept as reported.
    let o = get_observation(&mut conn, btc.inputs[0].0)
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

    // FX: the EUR/USD market (an FX instrument, base EUR) priced in USD.
    let eur = get_canonical_quote(&mut conn, subject("eur-usd"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(eur.method, AggregationMethod::LatestObservationV1);
    let o = get_observation(&mut conn, eur.inputs[0].0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.subject(), subject("eur-usd"));
    assert_eq!(o.unit(), unit("usd"));
    assert_eq!(o.price().to_string(), "1.13680");
    // Nothing prices the EUR currency itself any more.
    assert!(
        get_canonical_quote(&mut conn, subject("eur"), unit("usd"))
            .await
            .unwrap()
            .is_none()
    );

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

    // BTC perpetual: mark price in its USDT denomination (an asset, not USD
    // and not its USDC margin asset), at Hyperliquid.
    let perp = canonical(&mut conn, "btc-perp", "usdt").await;
    assert_eq!(perp.price_type(), PriceType::Mark);
    assert_eq!(
        perp.unit(),
        PriceUnit::Asset(curated("usdt").try_into().unwrap())
    );
    assert!(
        get_canonical_quote(&mut conn, subject("btc-perp"), unit("usdc"))
            .await
            .unwrap()
            .is_none(),
        "no BTC-PERP quote in USDC"
    );
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
async fn iex_trade_and_iex_book_are_two_feeds_and_the_newest_is_canonical() {
    use undrly_normalize::alpaca::AlpacaNormalizer;
    use undrly_provider::alpaca::AlpacaProvider;

    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let snapshot = |trade_t: &str, quote_t: &str| {
        format!(
            r#"{{"NVDA":{{"latestTrade":{{"t":"{trade_t}","x":"V","p":225.05}},"latestQuote":{{"ap":225.07,"ax":"V","bp":225.04,"bx":"V","t":"{quote_t}"}}}}}}"#
        )
        .into_bytes()
    };
    let ingest = async |conn: &mut sqlx::PgConnection, payload: Vec<u8>, at: &str| {
        let report = ingest_quotes(
            conn,
            &AlpacaProvider::new(),
            &AlpacaNormalizer,
            &raw("alpaca NVDA", payload, at),
        )
        .await
        .unwrap();
        refresh_canonical_quotes(conn, &report.pairs, Timestamp::parse(at).unwrap())
            .await
            .unwrap();
        report
    };

    // In session (15:59 New York), the book (19:59:49Z) is newer than the
    // trade (19:59:34Z): canonical is
    // IEX's mid, with IEX's bid and ask.
    let first = ingest(
        &mut conn,
        snapshot(
            "2026-09-25T19:59:34.720445259Z",
            "2026-09-25T19:59:49.048604722Z",
        ),
        "2026-09-25T19:59:50Z",
    )
    .await;
    assert_eq!(first.observations.len(), 2, "last and mid, one feed each");
    assert!(first.missing.is_empty());
    let q = get_canonical_quote(&mut conn, subject("nvda"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.method, AggregationMethod::LatestObservationV1);
    assert_eq!(q.price_type, PriceType::Mid);
    assert_eq!(q.price.to_string(), "225.055");
    assert_eq!(
        q.basis,
        ObservationBasis::Venue(curated("iex").try_into().unwrap())
    );
    let ba = q.bid_ask.unwrap();
    assert_eq!(
        (ba.bid.to_string(), ba.ask.to_string()),
        ("225.04".into(), "225.07".into())
    );
    assert_eq!(
        q.as_of,
        Timestamp::parse("2026-09-25T19:59:49.048604Z").unwrap()
    );

    // A later trade (19:59:55Z) is newer than that book: canonical is the
    // last trade, without bid/ask. Both observations stay stored.
    ingest(
        &mut conn,
        snapshot("2026-09-25T19:59:55Z", "2026-09-25T19:59:49.048604722Z"),
        "2026-09-25T19:59:56Z",
    )
    .await;
    let q = get_canonical_quote(&mut conn, subject("nvda"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.price_type, PriceType::Last);
    assert_eq!(q.bid_ask, None);

    // After the close (16:59:49 New York) the book is newer still, but it is
    // not a market: no mid observation, and canonical stays the last trade.
    let after = ingest(
        &mut conn,
        snapshot("2026-09-25T19:59:55Z", "2026-09-25T20:59:49.048604722Z"),
        "2026-09-25T21:00:00Z",
    )
    .await;
    assert_eq!(after.missing, vec![VenueSymbol::new("NVDA").unwrap()]);
    let q = get_canonical_quote(&mut conn, subject("nvda"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.price_type, PriceType::Last);
    assert_eq!(q.price.to_string(), "225.05");
    assert_eq!(
        latest_observations(&mut conn, subject("nvda"), unit("usd"))
            .await
            .unwrap()
            .len(),
        2
    );
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
    assert_eq!(q.method, AggregationMethod::LatestObservationV1);
    get_observation(conn, q.inputs[0].0).await.unwrap().unwrap()
}

async fn count(conn: &mut sqlx::PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM source_records) + (SELECT count(*) FROM market_observations)",
    )
    .fetch_one(conn)
    .await
    .unwrap()
}

// --- multi-source BTC/USD (mean-venue-mid-v1) -------------------------------------

const COINBASE_BOOK: &str = "tests/fixtures/sources/coinbase/book-BTC-USD-level1.json";

async fn ingest_kraken(conn: &mut sqlx::PgConnection, received_at: &str) -> QuoteIngestReport {
    ingest_quotes(
        conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        &raw(
            &KrakenProvider::ticker_url(&["XXBTZUSD", "ZEURZUSD"]),
            repo("tests/fixtures/sources/kraken/ticker.json"),
            received_at,
        ),
    )
    .await
    .unwrap()
}

async fn ingest_coinbase(
    conn: &mut sqlx::PgConnection,
    payload: Vec<u8>,
    received_at: &str,
) -> Result<QuoteIngestReport, undrly_ingest::IngestError> {
    ingest_quotes(
        conn,
        &CoinbaseProvider::new(),
        &CoinbaseNormalizer,
        &raw(&CoinbaseProvider::book_url("BTC-USD"), payload, received_at),
    )
    .await
}

async fn refresh_btc(
    conn: &mut sqlx::PgConnection,
    at: &str,
) -> undrly_ingest::quotes::CanonicalRefresh {
    refresh_canonical_quotes(
        conn,
        &[(subject("btc"), unit("usd"))],
        Timestamp::parse(at).unwrap(),
    )
    .await
    .unwrap()
    .remove(0)
}

#[tokio::test]
async fn two_venues_coexist_and_aggregate_with_exact_provenance() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    // Kraken's ticker states no time: effective = receipt. Coinbase's book
    // time is 07:28:06.386017Z.
    let kraken = ingest_kraken(&mut conn, "2026-09-25T07:28:07Z").await;
    let coinbase = ingest_coinbase(&mut conn, repo(COINBASE_BOOK), "2026-09-25T07:28:07.100Z")
        .await
        .unwrap();
    assert_eq!(coinbase.observations.len(), 1);

    // Both venue observations exist independently, as each venue reported.
    let latest = latest_observations(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap();
    assert_eq!(latest.len(), 2);
    let by_source = |s: &str| {
        latest
            .iter()
            .find(|(_, o)| o.source_id().as_str() == s)
            .unwrap()
    };
    let (kraken_id, k) = by_source("kraken");
    let (coinbase_id, c) = by_source("coinbase");
    assert_eq!(
        k.basis(),
        ObservationBasis::Venue(curated("kraken").try_into().unwrap())
    );
    assert_eq!(
        c.basis(),
        ObservationBasis::Venue(curated("coinbase").try_into().unwrap())
    );
    assert_eq!(
        (k.price_type(), c.price_type()),
        (PriceType::Last, PriceType::Mid)
    );
    assert_eq!(c.price().to_string(), "84006.455");

    let refresh = refresh_btc(&mut conn, "2026-09-25T07:28:10Z").await;
    assert_eq!(refresh.write, Some(Write::Inserted));
    let q = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.method, AggregationMethod::MeanVenueMidV1);
    assert_eq!(
        q.basis,
        ObservationBasis::Aggregated,
        "attributed to no venue"
    );
    assert_eq!(q.price_type, PriceType::Mid);
    // (84145.950000 + 84006.455) / 2
    assert_eq!(q.price.to_string(), "84076.2025000");
    // Mean bid (84145.90000 + 84006.45) / 2 and mean ask
    // (84146.00000 + 84006.46) / 2 of the same inputs; stored and read back.
    let ba = q.bid_ask.expect("mean bid and ask");
    assert_eq!(
        (ba.bid.to_string(), ba.ask.to_string()),
        ("84076.1750000".to_owned(), "84076.2300000".to_owned())
    );
    assert!(ba.bid <= q.price && q.price <= ba.ask);
    assert_eq!(
        q.as_of,
        Timestamp::parse("2026-09-25T07:28:06.386017Z").unwrap(),
        "oldest input"
    );
    assert_eq!(q.eligible_count(), 2);

    // Provenance: exactly the two observations, each from its raw record.
    let mut used: Vec<(ObservationId, String)> = q
        .inputs
        .iter()
        .map(|(id, p)| (*id, p.to_string()))
        .collect();
    used.sort();
    let mut expected = vec![
        (*kraken_id, "84145.950000".to_owned()),
        (*coinbase_id, "84006.455".to_owned()),
    ];
    expected.sort();
    assert_eq!(used, expected);
    let records: Vec<i64> = sqlx::query_scalar(
        "SELECT o.source_record_id FROM canonical_quote_inputs i
         JOIN market_observations o ON o.id = i.observation_id
         WHERE i.subject_id = $1 ORDER BY o.source_record_id",
    )
    .bind(curated("btc").uuid())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let mut want = vec![kraken.source_record.0.0, coinbase.source_record.0.0];
    want.sort();
    assert_eq!(records, want);

    // Replay: same records, same observations, same canonical quote.
    let again_k = ingest_kraken(&mut conn, "2026-09-25T07:28:08Z").await;
    let again_c = ingest_coinbase(&mut conn, repo(COINBASE_BOOK), "2026-09-25T07:28:08Z")
        .await
        .unwrap();
    assert_eq!(
        again_k.source_record,
        (kraken.source_record.0, Write::Unchanged)
    );
    assert_eq!(
        again_c.source_record,
        (coinbase.source_record.0, Write::Unchanged)
    );
    assert!(
        again_k
            .observations
            .iter()
            .chain(&again_c.observations)
            .all(|o| o.2 == Write::Unchanged)
    );
    assert_eq!(
        refresh_btc(&mut conn, "2026-09-25T07:28:10Z").await.write,
        Some(Write::Unchanged)
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn aggregation_is_independent_of_ingestion_order() {
    let mut results = Vec::new();
    for kraken_first in [true, false] {
        let Some((db, mut conn)) = seeded().await else {
            return;
        };
        if kraken_first {
            ingest_kraken(&mut conn, "2026-09-25T07:28:07Z").await;
            ingest_coinbase(&mut conn, repo(COINBASE_BOOK), "2026-09-25T07:28:07.100Z")
                .await
                .unwrap();
        } else {
            ingest_coinbase(&mut conn, repo(COINBASE_BOOK), "2026-09-25T07:28:07.100Z")
                .await
                .unwrap();
            ingest_kraken(&mut conn, "2026-09-25T07:28:07Z").await;
        }
        refresh_btc(&mut conn, "2026-09-25T07:28:10Z").await;
        let q = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
            .await
            .unwrap()
            .unwrap();
        let mut inputs: Vec<(String, String)> = Vec::new();
        for (id, p) in &q.inputs {
            let o = get_observation(&mut conn, *id).await.unwrap().unwrap();
            inputs.push((o.source_id().to_string(), p.to_string()));
        }
        inputs.sort();
        let ba = q.bid_ask.unwrap();
        results.push((
            (q.price.to_string(), ba.bid.to_string(), ba.ask.to_string()),
            q.as_of,
            inputs,
        ));
        drop(conn);
        db.teardown().await;
    }
    assert_eq!(results[0], results[1]);
}

#[tokio::test]
async fn stale_observations_are_excluded_with_documented_fallback() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest_coinbase(&mut conn, repo(COINBASE_BOOK), "2026-09-25T07:28:07Z")
        .await
        .unwrap();
    ingest_kraken(&mut conn, "2026-09-25T07:28:30Z").await;

    // 07:28:40: Coinbase (07:28:06.386) is 33.6 s old → excluded. One fresh
    // venue remains: the canonical quote is its mid, still `aggregated`.
    let one = refresh_btc(&mut conn, "2026-09-25T07:28:40Z").await;
    assert_eq!(one.inputs.len(), 1);
    let q = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.eligible_count(), 1);
    assert_eq!(q.basis, ObservationBasis::Aggregated);
    assert_eq!(q.price.to_string(), "84145.9500000");
    // Kraken's own bid and ask; stale Coinbase contributes nothing.
    let ba = q.bid_ask.unwrap();
    assert_eq!(
        (ba.bid.to_string(), ba.ask.to_string()),
        ("84145.9000000".to_owned(), "84146.0000000".to_owned())
    );
    let o = get_observation(&mut conn, q.inputs[0].0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.source_id().as_str(), "kraken");

    // 07:29:01: both are older than 30 s → no canonical quote at all.
    let none = refresh_btc(&mut conn, "2026-09-25T07:29:01Z").await;
    assert!(none.inputs.is_empty() && none.write.is_none());
    assert!(
        get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
            .await
            .unwrap()
            .is_none()
    );
    // The observations themselves are untouched.
    assert_eq!(
        latest_observations(&mut conn, subject("btc"), unit("usd"))
            .await
            .unwrap()
            .len(),
        2
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn a_failed_provider_leaves_the_other_venue_intact() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest_kraken(&mut conn, "2026-09-25T07:28:07Z").await;
    let before = count(&mut conn).await;
    for payload in [
        &br#"{"message":"Internal server error"}"#[..],
        br#"{"bids":[],"asks":[["84006.46","1",1]],"sequence":1,"time":"2026-09-25T07:28:06Z"}"#,
    ] {
        assert!(
            ingest_coinbase(&mut conn, payload.to_vec(), "2026-09-25T07:28:08Z")
                .await
                .is_err()
        );
    }
    assert_eq!(
        count(&mut conn).await,
        before,
        "the failed responses wrote nothing"
    );
    let refresh = refresh_btc(&mut conn, "2026-09-25T07:28:10Z").await;
    assert_eq!(refresh.inputs.len(), 1);
    let q = get_canonical_quote(&mut conn, subject("btc"), unit("usd"))
        .await
        .unwrap()
        .unwrap();
    let o = get_observation(&mut conn, q.inputs[0].0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.source_id().as_str(), "kraken");
    drop(conn);
    db.teardown().await;
}
