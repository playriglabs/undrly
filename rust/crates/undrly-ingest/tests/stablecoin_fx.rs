//! V1.9 live FX (docs/v1.9-live-fx.md): the stablecoin-fx file →
//! stablecoin/fiat feeds and declared crosses; captured venue responses →
//! observations → a stablecoin market's canonical quote → a derived
//! `USD/IDR` cross with its legs' provenance. No network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AggregationMethod, CanonicalId, DisplayName, ObservationBasis, PriceSubject, PriceType,
    PriceUnit, Redistribution, Source, SourceId, Timestamp, VenueSymbol, convert_rate, cross_rate,
};
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::quotes::{ingest_quotes_for, refresh_canonical_quotes, refresh_cross_quotes};
use undrly_ingest::{RawRecord, store_raw_record};
use undrly_normalize::QuoteNormalizer;
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_normalize::venues::{BookTickerNormalizer, IndodaxNormalizer};
use undrly_provider::QuoteProvider;
use undrly_provider::binance::BinanceProvider;
use undrly_provider::indodax::IndodaxProvider;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::market::{delete_canonical_quote, get_canonical_quote};
use undrly_store::sources::insert_source;
use undrly_store::testing::{TestDb, fresh};

/// Indodax's fixture states `server_time` 2026-10-01T05:25:12Z; every record
/// is received one second later, so all legs are within the 30 s window.
const AT: &str = "2026-10-01T05:25:13Z";

fn repo(path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn raw(key: &str, payload: Vec<u8>, at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload,
        received_at: Timestamp::parse(at).unwrap(),
    }
}

/// A canonical id from the FX id map or the V1 universe.
fn id(key: &str) -> CanonicalId {
    let fx: serde_json::Value =
        serde_json::from_slice(&repo("data/reference/fx-ids.json")).unwrap();
    if let Some(id) = fx["ids"][key].as_str() {
        return CanonicalId::parse(id).unwrap();
    }
    let v1: serde_json::Value = serde_json::from_slice(&repo("data/demo/universe.json")).unwrap();
    for o in v1["instruments"].as_array().unwrap() {
        if o["key"] == key {
            return CanonicalId::parse(o["id"].as_str().unwrap()).unwrap();
        }
    }
    panic!("no key {key}")
}

fn currency(code: &str) -> PriceUnit {
    PriceUnit::Currency(id(&format!("iso4217:{code}")).try_into().unwrap())
}

fn usdt() -> PriceSubject {
    PriceSubject::Instrument(id("usdt").try_into().unwrap())
}

fn usd_idr() -> (PriceSubject, PriceUnit) {
    (
        PriceSubject::Instrument(id("fx:USD/IDR").try_into().unwrap()),
        currency("IDR"),
    )
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for s in [
        "undrly-curated",
        "kraken",
        "coinbase",
        "hyperliquid",
        "gold-api",
        "alpaca",
        "bitstamp",
        "ecb",
        "bank-of-canada",
        "fed-h10",
        "bank-indonesia",
        "bnm",
        "cbm",
        "binance",
        "okx",
        "indodax",
        "bitkub",
        "coins-ph",
        "hashkey",
    ] {
        insert_source(
            &mut conn,
            &Source {
                id: SourceId::parse(s).unwrap(),
                name: DisplayName::new(s).unwrap(),
                redistribution: Redistribution::Unknown,
            },
        )
        .await
        .unwrap();
    }
    let at = "2026-10-01T00:00:00Z";
    for file in ["data/demo/universe.json"] {
        ingest_universe(&mut conn, &raw(file, repo(file), at))
            .await
            .unwrap();
    }
    store_raw_record(
        &mut conn,
        &SourceId::parse("undrly-curated").unwrap(),
        &raw(
            "data/reference/fx-spec.json",
            repo("data/reference/fx-spec.json"),
            at,
        ),
    )
    .await
    .unwrap();
    for file in [
        "data/reference/fx.json",
        "data/reference/stablecoin-fx.json",
    ] {
        let r = ingest_universe(&mut conn, &raw(file, repo(file), at))
            .await
            .unwrap();
        assert!(r.inserted > 0, "{file}");
    }
    Some((db, conn))
}

async fn ingest<P, N>(
    conn: &mut sqlx::PgConnection,
    provider: &P,
    normalizer: &N,
    key: &str,
    fixture: &str,
    requested: &[&str],
) where
    P: QuoteProvider,
    N: QuoteNormalizer<Quote = P::Quote>,
{
    let requested: Vec<VenueSymbol> = requested
        .iter()
        .map(|s| VenueSymbol::new(s).unwrap())
        .collect();
    let report = ingest_quotes_for(
        conn,
        provider,
        normalizer,
        &raw(key, repo(&format!("tests/fixtures/sources/{fixture}")), AT),
        Some(&requested),
    )
    .await
    .unwrap();
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    refresh_canonical_quotes(conn, &report.pairs, Timestamp::parse(AT).unwrap())
        .await
        .unwrap();
}

async fn legs_and_cross(conn: &mut sqlx::PgConnection) {
    ingest(
        conn,
        &BinanceProvider::new(),
        &BookTickerNormalizer,
        "https://api.binance.com/api/v3/ticker/bookTicker?symbols=fixture",
        "binance/book-ticker.json",
        &["USDTIDR", "USDCIDR", "USDTAED", "USDTBRL"],
    )
    .await;
    ingest(
        conn,
        &IndodaxProvider::new(),
        &IndodaxNormalizer,
        "https://indodax.com/api/ticker/usdtidr",
        "indodax/ticker-usdtidr.json",
        &["usdtidr"],
    )
    .await;
    ingest(
        conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        "https://api.kraken.com/0/public/Ticker?pair=USDTZUSD",
        "kraken/ticker-USDTZUSD.json",
        &["USDTZUSD"],
    )
    .await;
}

#[tokio::test]
async fn declares_feeds_aggregations_and_crosses() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let (feeds, aggregations, derivations): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM quote_feeds WHERE feed_source_id IN
                   ('binance','okx','indodax','bitkub','coins-ph','hashkey')),
                (SELECT count(*) FROM quote_aggregations),
                (SELECT count(*) FROM quote_derivations)",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(feeds >= 20, "{feeds} new venue feeds");
    assert!(aggregations > 0);
    assert_eq!(derivations, 8);
    // Re-seeding the file writes nothing.
    let replay = ingest_universe(
        &mut conn,
        &raw(
            "data/reference/stablecoin-fx.json",
            repo("data/reference/stablecoin-fx.json"),
            "2026-10-01T01:00:00Z",
        ),
    )
    .await
    .unwrap();
    assert_eq!(replay.inserted, 0);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn usd_idr_is_the_cross_of_two_venue_legs_with_their_provenance() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    legs_and_cross(&mut conn).await;

    // USDT/IDR: Binance and Indodax books, a mean of venue mids.
    let idr = get_canonical_quote(&mut conn, usdt(), currency("IDR"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(idr.method, AggregationMethod::MeanVenueMidV1);
    assert_eq!(idr.inputs.len(), 2);
    let usd = get_canonical_quote(&mut conn, usdt(), currency("USD"))
        .await
        .unwrap()
        .unwrap();

    let (subject, unit) = usd_idr();
    let q = get_canonical_quote(&mut conn, subject, unit)
        .await
        .unwrap()
        .expect("USD/IDR is derived once both legs exist");
    assert_eq!(q.method, AggregationMethod::CrossViaStablecoinV1);
    assert_eq!(q.basis, ObservationBasis::Derived);
    assert_eq!(q.price_type, PriceType::Mid);
    assert_eq!(q.price, cross_rate(idr.price, usd.price).unwrap());
    assert_eq!(q.as_of, idr.as_of.min(usd.as_of));
    // Every leg input is kept, with the pair it priced.
    let legs: Vec<(String, String)> = sqlx::query_as(
        "SELECT o.source_id, l.leg_unit_id::text FROM canonical_quote_legs l
         JOIN market_observations o ON o.id = l.observation_id
         WHERE l.subject_id = $1 ORDER BY o.source_id",
    )
    .bind(subject.canonical().uuid())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let sources: Vec<&str> = legs.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(sources, ["binance", "indodax", "kraken"]);
    assert_eq!(q.inputs.len(), 3);

    // Refreshing the pair itself (as a reference rate arriving would) keeps
    // the cross: a derived pair is never priced by its own observations.
    let again =
        refresh_canonical_quotes(&mut conn, &[(subject, unit)], Timestamp::parse(AT).unwrap())
            .await
            .unwrap();
    assert!(
        again
            .iter()
            .all(|r| r.method == AggregationMethod::CrossViaStablecoinV1)
    );
    assert_eq!(
        get_canonical_quote(&mut conn, subject, unit)
            .await
            .unwrap()
            .unwrap()
            .method,
        AggregationMethod::CrossViaStablecoinV1
    );

    // Without a leg there is no cross: it is removed, never left stale.
    delete_canonical_quote(&mut conn, usdt(), currency("USD"))
        .await
        .unwrap();
    let gone = refresh_cross_quotes(&mut conn, &[(subject, unit)], Timestamp::parse(AT).unwrap())
        .await
        .unwrap();
    assert_eq!(gone.len(), 1);
    assert!(gone[0].write.is_none() && gone[0].inputs.is_empty());
    assert!(
        get_canonical_quote(&mut conn, subject, unit)
            .await
            .unwrap()
            .is_none()
    );
    drop(conn);
    db.teardown().await;
}

/// V1.10 (docs/v1.10-tokenized-stocks.md §12): a market quoted only in USDT
/// restated in US dollars, `X/USD = X/USDT × USDT/USD`. BTC stands in for a
/// USDT-only coin; the declaration prices BTC/USD by its legs alone.
#[tokio::test]
async fn a_usdt_market_is_converted_to_usd_through_usdt_usd() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let fx: serde_json::Value =
        serde_json::from_slice(&repo("data/reference/stablecoin-fx.json")).unwrap();
    let binance = fx["venues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["key"] == "venue:binance")
        .unwrap()["id"]
        .clone();
    let btc = id("btc").to_string();
    let usdt_id = id("usdt").to_string();
    let usd_id = id("iso4217:USD").to_string();
    let declared = serde_json::json!({
        "dataset": "test-convert",
        "version": 1,
        "description": "BTC/USDT on Binance, restated in USD",
        "currencies": [], "entities": [], "venues": [], "instruments": [], "listings": [],
        "relationships": [], "aliases": [],
        "quoteFeeds": [{
            "source": "binance", "symbol": "BTCUSDT", "subject": btc, "unit": usdt_id,
            "basis": "venue", "venue": binance, "priceType": "mid"
        }],
        "quoteDerivations": [{
            "subject": btc, "unit": usd_id, "method": "convert-via-stablecoin-v1", "via": usdt_id
        }]
    });
    ingest_universe(
        &mut conn,
        &raw(
            "test/convert.json",
            serde_json::to_vec(&declared).unwrap(),
            AT,
        ),
    )
    .await
    .unwrap();
    legs_and_cross(&mut conn).await;
    let book = br#"[{"symbol":"BTCUSDT","bidPrice":"62000.00","bidQty":"1","askPrice":"62001.00","askQty":"1"}]"#;
    let report = ingest_quotes_for(
        &mut conn,
        &BinanceProvider::new(),
        &BookTickerNormalizer,
        &raw(
            "https://api.binance.com/api/v3/ticker/bookTicker?symbols=BTCUSDT",
            book.to_vec(),
            AT,
        ),
        Some(&[VenueSymbol::new("BTCUSDT").unwrap()]),
    )
    .await
    .unwrap();
    refresh_canonical_quotes(&mut conn, &report.pairs, Timestamp::parse(AT).unwrap())
        .await
        .unwrap();

    let btc_subject = PriceSubject::Instrument(id("btc").try_into().unwrap());
    let in_usdt = get_canonical_quote(
        &mut conn,
        btc_subject,
        PriceUnit::Asset(id("usdt").try_into().unwrap()),
    )
    .await
    .unwrap()
    .expect("BTC/USDT is its own market");
    let usdt_usd = get_canonical_quote(&mut conn, usdt(), currency("USD"))
        .await
        .unwrap()
        .unwrap();
    let q = get_canonical_quote(&mut conn, btc_subject, currency("USD"))
        .await
        .unwrap()
        .expect("BTC/USD is derived once both legs exist");
    assert_eq!(q.method, AggregationMethod::ConvertViaStablecoinV1);
    assert_eq!(q.basis, ObservationBasis::Derived);
    assert_eq!(
        q.price,
        convert_rate(in_usdt.price, usdt_usd.price).unwrap()
    );
    assert_eq!(q.as_of, in_usdt.as_of.min(usdt_usd.as_of));
    let legs: Vec<(String,)> = sqlx::query_as(
        "SELECT o.source_id FROM canonical_quote_legs l
         JOIN market_observations o ON o.id = l.observation_id
         WHERE l.subject_id = $1 AND l.unit_id = $2 ORDER BY o.source_id",
    )
    .bind(btc_subject.canonical().uuid())
    .bind(currency("USD").canonical().uuid())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        legs.iter().map(|(s,)| s.as_str()).collect::<Vec<_>>(),
        ["binance", "kraken"]
    );
    drop(conn);
    db.teardown().await;
}
