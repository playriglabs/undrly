//! V1.2 FX (docs/v1.2-fx.md): the FX spec and its built curated file →
//! FX instruments, feeds and both universes; captured responses of every FX
//! source → observations → canonical quotes. No network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AggregationMethod, CanonicalId, DisplayName, ObservationBasis, PriceSubject, PriceType,
    PriceUnit, Redistribution, Source, SourceId, Timestamp, UniverseKey, VenueSymbol,
};
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::quotes::{QuoteIngestReport, ingest_quotes_for, refresh_canonical_quotes};
use undrly_ingest::{RawRecord, store_raw_record};
use undrly_normalize::QuoteNormalizer;
use undrly_normalize::fx::{
    BankIndonesiaNormalizer, BankOfCanadaNormalizer, BitstampNormalizer, BnmNormalizer,
    CbmNormalizer, EcbNormalizer, FedH10Normalizer,
};
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_provider::QuoteProvider;
use undrly_provider::bank_indonesia::BankIndonesiaProvider;
use undrly_provider::bank_of_canada::BankOfCanadaProvider;
use undrly_provider::bitstamp::BitstampProvider;
use undrly_provider::bnm::BnmProvider;
use undrly_provider::cbm::CbmProvider;
use undrly_provider::ecb::EcbProvider;
use undrly_provider::fed_h10::FedH10Provider;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::Write;
use undrly_store::market::{get_canonical_quote, get_observation};
use undrly_store::sources::insert_source;
use undrly_store::testing::{TestDb, fresh};
use undrly_store::universe::latest_universe_snapshot;

const SOURCES: [&str; 13] = [
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
];

fn repo(path: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn fixture(path: &str) -> Vec<u8> {
    repo(&format!("tests/fixtures/sources/{path}"))
}

fn raw(key: &str, payload: Vec<u8>, received_at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload,
        received_at: Timestamp::parse(received_at).unwrap(),
    }
}

/// The canonical id of an FX build key (`fx:USD/JPY`, `iso4217:JPY`) or a V1
/// key (`eur-usd`, `usd`).
fn id(key: &str) -> CanonicalId {
    let ids: serde_json::Value =
        serde_json::from_slice(&repo("data/reference/fx-ids.json")).unwrap();
    if let Some(id) = ids["ids"][key].as_str() {
        return CanonicalId::parse(id).unwrap();
    }
    let v1: serde_json::Value = serde_json::from_slice(&repo("data/demo/universe.json")).unwrap();
    for group in ["currencies", "venues", "instruments"] {
        for o in v1[group].as_array().unwrap() {
            if o["key"] == key {
                return CanonicalId::parse(o["id"].as_str().unwrap()).unwrap();
            }
        }
    }
    panic!("no key {key}")
}

/// The FX market `BASE/QUOTE` and its quote currency.
fn pair(p: &str) -> (PriceSubject, PriceUnit) {
    let quote = p.split_once('/').unwrap().1;
    (
        PriceSubject::Instrument(id(&format!("fx:{p}")).try_into().unwrap()),
        PriceUnit::Currency(id(&format!("iso4217:{quote}")).try_into().unwrap()),
    )
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for s in SOURCES {
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
    let at = "2026-09-26T00:00:00Z";
    ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            repo("data/demo/universe.json"),
            at,
        ),
    )
    .await
    .unwrap();
    // The spec is stored first: the memberships name it.
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
    let report = ingest_universe(
        &mut conn,
        &raw("data/reference/fx.json", repo("data/reference/fx.json"), at),
    )
    .await
    .unwrap();
    assert!(report.inserted > 0);
    Some((db, conn))
}

async fn ingest<P, N>(
    conn: &mut sqlx::PgConnection,
    provider: &P,
    normalizer: &N,
    key: &str,
    payload: Vec<u8>,
    at: &str,
) -> QuoteIngestReport
where
    P: QuoteProvider,
    N: QuoteNormalizer<Quote = P::Quote>,
{
    // One Bitstamp market per record: the record covers only that symbol.
    let requested: Option<Vec<VenueSymbol>> = key
        .strip_prefix("https://www.bitstamp.net/api/v2/ticker/")
        .map(|m| vec![VenueSymbol::new(m.trim_end_matches('/')).unwrap()]);
    let report = ingest_quotes_for(
        conn,
        provider,
        normalizer,
        &raw(key, payload, at),
        requested.as_deref(),
    )
    .await
    .unwrap();
    refresh_canonical_quotes(conn, &report.pairs, Timestamp::parse(at).unwrap())
        .await
        .unwrap();
    report
}

#[tokio::test]
async fn fx_universes_are_seeded_idempotently_with_shared_instruments() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let replay = ingest_universe(
        &mut conn,
        &raw(
            "data/reference/fx.json",
            repo("data/reference/fx.json"),
            "2026-09-26T01:00:00Z",
        ),
    )
    .await
    .unwrap();
    assert_eq!(replay.inserted, 0, "re-seeding writes nothing");
    assert_eq!(replay.source_record.unwrap().1, Write::Unchanged);

    let major = latest_universe_snapshot(&mut conn, UniverseKey::FxMajor)
        .await
        .unwrap()
        .unwrap();
    let sea = latest_universe_snapshot(&mut conn, UniverseKey::FxSoutheastAsia)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(major.snapshot.members.len(), 29);
    // V1.9 moved USD/PHP from excluded to a member (priced as a derived cross);
    // V1.10 added USD/VND, USD/BND, USD/KHR and USD/LAK (CBM table crosses).
    assert_eq!(sea.snapshot.members.len(), 14);
    let global = latest_universe_snapshot(&mut conn, UniverseKey::FxGlobal)
        .await
        .unwrap()
        .unwrap();
    // V1.10 added USD/CNY (Fed H.10) and USD/SAR (a CBM table cross).
    assert_eq!(global.snapshot.members.len(), 6);
    let usd_sgd = id("fx:USD/SGD");
    for u in [&major, &sea] {
        assert!(u.snapshot.members.iter().any(|m| m.node == usd_sgd));
    }
    assert!(
        major
            .snapshot
            .members
            .iter()
            .any(|m| m.node == id("eur-usd")),
        "V1 EUR/USD is the fx-major member"
    );
    // One instrument per pair, whatever its universes.
    let (fx, pairs): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT (base_currency_id, quote_currency_id))
         FROM instruments WHERE instrument_class = 'fx'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    // V1.9 adds USD/PHP, USD/HKD, USD/AED, USD/BRL and USD/MXN; V1.10 adds
    // USD/CNY, USD/SAR, USD/VND, USD/BND, USD/KHR and USD/LAK.
    assert_eq!((fx, pairs), (48, 48));
    // The memberships name the spec record.
    let (key,): (String,) = sqlx::query_as(
        "SELECT r.record_key FROM universe_snapshots s JOIN source_records r
         ON r.id = s.source_record_id WHERE s.universe_key = 'fx-southeast-asia'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(key, "data/reference/fx-spec.json");
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn eur_usd_is_a_mean_of_two_venue_mids_with_complete_provenance() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    ingest(
        &mut conn,
        &BitstampProvider::new(),
        &BitstampNormalizer,
        "https://www.bitstamp.net/api/v2/ticker/eurusd/",
        fixture("bitstamp/ticker-eurusd.json"),
        "2026-09-26T03:30:01Z",
    )
    .await;
    ingest(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        "https://api.kraken.com/0/public/Ticker?pair=XXBTZUSD,ZEURZUSD",
        fixture("kraken/ticker.json"),
        "2026-09-26T03:30:05Z",
    )
    .await;
    let (subject, unit) = pair("EUR/USD");
    let q = get_canonical_quote(&mut conn, subject, unit)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.method, AggregationMethod::MeanVenueMidV1);
    assert_eq!(q.basis, ObservationBasis::Aggregated);
    assert_eq!(q.price_type, PriceType::Mid);
    assert_eq!(q.inputs.len(), 2);
    // Kraken 1.13679/1.13680 (mid 1.136795), Bitstamp 1.13882/1.13883
    // (mid 1.138825): the means at one scale.
    assert_eq!(q.price.to_string(), "1.1378100");
    let ba = q.bid_ask.unwrap();
    assert_eq!(
        (ba.bid.to_string(), ba.ask.to_string()),
        ("1.1378050".into(), "1.1378150".into())
    );
    // Every input traces to a raw record of a registered source.
    let chain: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT o.source_id, r.record_key, s.id, o.inverted
         FROM canonical_quote_inputs i
         JOIN market_observations o ON o.id = i.observation_id
         JOIN source_records r ON r.id = o.source_record_id
         JOIN sources s ON s.id = r.source_id
         WHERE i.subject_id = $1 ORDER BY o.source_id",
    )
    .bind(subject.canonical().uuid())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].0, "bitstamp");
    assert_eq!(chain[1].0, "kraken");
    assert!(chain.iter().all(|c| c.0 == c.2 && !c.3));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn reference_rates_have_no_bid_ask_and_inversions_are_recorded() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let at = "2026-09-26T03:31:00Z";
    ingest(
        &mut conn,
        &BankOfCanadaProvider::new(),
        &BankOfCanadaNormalizer,
        "https://www.bankofcanada.ca/valet/observations/FXEURCAD,FXGBPCAD,FXAUDCAD,FXNZDCAD,FXJPYCAD,FXCHFCAD/json?recent=5",
        fixture("bank-of-canada/observations.json"),
        at,
    )
    .await;
    // CAD/JPY = 1 / (JPY/CAD 0.008990), at the source's 4 significant digits.
    let (subject, unit) = pair("CAD/JPY");
    let q = get_canonical_quote(&mut conn, subject, unit)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.price.to_string(), "111.2");
    assert_eq!(q.price_type, PriceType::Reference);
    assert_eq!(q.basis, ObservationBasis::Aggregated);
    assert!(q.bid_ask.is_none());
    let o = get_observation(&mut conn, q.inputs[0].0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        o.observed_at().unwrap().to_string(),
        "2026-09-25T00:00:00Z",
        "the source's date, not Undrly's clock"
    );
    let inverted: Vec<(String, bool, String)> = sqlx::query_as(
        "SELECT i.name::text, o.inverted, r.record_key FROM market_observations o
         JOIN instruments i ON i.id = o.subject_id
         JOIN source_records r ON r.id = o.source_record_id
         WHERE o.source_id = 'bank-of-canada' ORDER BY i.name",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let names: Vec<(&str, bool)> = inverted.iter().map(|(n, i, _)| (n.as_str(), *i)).collect();
    assert_eq!(
        names,
        vec![
            ("AUD/CAD", false),
            ("CAD/CHF", true),
            ("CAD/JPY", true),
            ("EUR/CAD", false),
            ("GBP/CAD", false),
            ("NZD/CAD", false),
        ]
    );
    // The raw record keeps the source's own JPY/CAD value.
    let (payload,): (Vec<u8>,) = sqlx::query_as(
        "SELECT r.payload FROM market_observations o JOIN source_records r
         ON r.id = o.source_record_id WHERE o.id = $1",
    )
    .bind(q.inputs[0].0.0)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(String::from_utf8(payload).unwrap().contains("0.008990"));
    let (subject, unit) = pair("CAD/CHF");
    let q = get_canonical_quote(&mut conn, subject, unit)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.price.to_string(), "0.58575");
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn every_reference_source_prices_its_pairs() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let at = "2026-09-26T03:31:00Z";
    ingest(
        &mut conn,
        &EcbProvider::new(),
        &EcbNormalizer,
        "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml",
        fixture("ecb/eurofxref-daily.xml"),
        at,
    )
    .await;
    ingest(
        &mut conn,
        &FedH10Provider::new(),
        &FedH10Normalizer,
        undrly_provider::fed_h10::PACKAGE_URL,
        fixture("fed-h10/h10-lastobs10.csv"),
        at,
    )
    .await;
    ingest(
        &mut conn,
        &BankIndonesiaProvider::new(),
        &BankIndonesiaNormalizer,
        "https://www.bi.go.id/biwebservice/wskursbi.asmx/getSubKursJisdor3?mts=USD&startDate=2026-09-12&endDate=2026-09-26",
        fixture("bank-indonesia/jisdor-usd.xml"),
        at,
    )
    .await;
    ingest(
        &mut conn,
        &BankIndonesiaProvider::new(),
        &BankIndonesiaNormalizer,
        "https://www.bi.go.id/biwebservice/wskursbi.asmx/getSubKursLokal3?mts=SGD&startdate=2026-09-12&enddate=2026-09-26",
        fixture("bank-indonesia/kurs-sgd.xml"),
        at,
    )
    .await;
    // BNM after H.10: USD/MYR has both; BNM's rate is the newer.
    ingest(
        &mut conn,
        &BnmProvider::new(),
        &BnmNormalizer,
        undrly_provider::bnm::RATES_URL,
        fixture("bnm/exchange-rate-1700.json"),
        at,
    )
    .await;
    ingest(
        &mut conn,
        &CbmProvider::new(),
        &CbmNormalizer,
        undrly_provider::cbm::LATEST_URL,
        fixture("cbm/latest.json"),
        at,
    )
    .await;
    for (p, price, source) in [
        ("EUR/JPY", "179.70", "ecb"),
        ("EUR/SGD", "1.4563", "ecb"),
        ("USD/JPY", "156.8700", "fed-h10"),
        ("NZD/USD", "0.5710", "fed-h10"),
        ("USD/SGD", "1.2775", "fed-h10"),
        ("USD/THB", "33.3600", "fed-h10"),
        ("USD/IDR", "17917.00", "bank-indonesia"),
        ("SGD/IDR", "13992.105", "bank-indonesia"),
        ("USD/MYR", "4.0735", "bnm"),
        ("SGD/MYR", "3.1878", "bnm"),
        ("USD/MMK", "2100.00", "cbm"),
    ] {
        let (subject, unit) = pair(p);
        let q = get_canonical_quote(&mut conn, subject, unit)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{p}: no canonical quote"));
        assert_eq!(q.price.to_string(), price, "{p}");
        assert_eq!(q.price_type, PriceType::Reference, "{p}");
        assert_eq!(q.basis, ObservationBasis::Aggregated, "{p}");
        assert!(q.bid_ask.is_none(), "{p}: a reference rate has no bid/ask");
        assert_eq!(q.method, AggregationMethod::LatestObservationV1, "{p}");
        let o = get_observation(&mut conn, q.inputs[0].0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(o.source_id().as_str(), source, "{p}");
    }
    drop(conn);
    db.teardown().await;
}
