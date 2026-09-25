//! V1.1 universe (docs/v1.1-universe.md): a snapshot in the curated format
//! seeded beside V1, memberships attributed to their upstream record,
//! symbol-scoped quote ingestion, and `mean-venue-mid-v1` for a second pair.
//! All inputs are **synthetic** (made-up assets and holdings). No network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{
    AggregationMethod, CanonicalId, DisplayName, ExternalIdentifier, InstrumentId,
    ObservationBasis, PriceSubject, PriceUnit, Redistribution, Source, SourceId, Timestamp,
    UniverseKey, VenueSymbol,
};
use undrly_ingest::curated::{ingest_universe, ingest_universe_as};
use undrly_ingest::quotes::{ingest_quotes_for, refresh_canonical_quotes};
use undrly_ingest::{IngestError, RawRecord, store_raw_record};
use undrly_normalize::alpaca::AlpacaNormalizer;
use undrly_normalize::coinbase::CoinbaseNormalizer;
use undrly_normalize::kraken::KrakenNormalizer;
use undrly_provider::alpaca::AlpacaProvider;
use undrly_provider::coinbase::CoinbaseProvider;
use undrly_provider::curated::CuratedProvider;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::identifiers::identifiers_for_node;
use undrly_store::market::get_canonical_quote;
use undrly_store::reference::get_instrument;
use undrly_store::sources::insert_source;
use undrly_store::testing::{TestDb, fresh};
use undrly_store::universe::latest_universe_snapshot;

const SOURCES: [&str; 6] = [
    "undrly-curated",
    "undrly-universe",
    "ssga",
    "kraken",
    "coinbase",
    "alpaca",
];

fn repo(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(path),
    )
    .unwrap()
}

fn v1(key: &str) -> String {
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
                return o["id"].as_str().unwrap().to_owned();
            }
        }
    }
    panic!("no V1 key {key}")
}

fn raw(key: &str, payload: &[u8], at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload: payload.to_vec(),
        received_at: Timestamp::parse(at).unwrap(),
    }
}

const SPY: &[u8] = b"synthetic SPY holdings file";

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const EXC: &str = "undrly:instrument:01m3byza2bfs3sfwfg9g4ffe58";
const EXA: &str = "undrly:instrument:01m3byza2cf3sst32gzww6b6tm";
const PERP: &str = "undrly:instrument:01m3byza2df58a5vtr7qcdhfqc";

/// A synthetic snapshot: a crypto asset on two venues, an equity with its
/// issuer (CIK only) on a new venue, a 1,000-unit perp, and an S&P 500
/// membership that includes V1's NVDA by canonical id.
fn snapshot(sha: &str) -> Vec<u8> {
    let usd = v1("usd");
    let (kraken, coinbase, iex) = (v1("kraken"), v1("coinbase"), v1("iex"));
    let (usdc, hl, nvda) = (v1("usdc"), v1("hyperliquid"), v1("nvda"));
    serde_json::to_vec_pretty(&serde_json::json!({
        "dataset": "undrly-universe-snapshot", "version": 1, "description": "synthetic",
        "currencies": [],
        "entities": [{"key": "sec:111", "id": "undrly:entity:01m3byza2ef49tp8vytq4bg35w",
                      "kind": "company", "name": "Example One Inc", "cik": "0000000111"}],
        "venues": [{"key": "venue:XNYS", "id": "undrly:venue:01m3byza2gfvtree52ynzk1y25",
                    "name": "New York Stock Exchange", "mic": "XNYS"}],
        "instruments": [
            {"key": "coingecko:examplecoin", "id": EXC, "class": "crypto_asset", "name": "Example Coin"},
            {"key": "spy:037833100", "id": EXA, "class": "equity", "name": "EXAMPLE ONE INC"},
            {"key": "hyperliquid:kEXC", "id": PERP, "class": "perpetual_future",
             "name": "kEXC Perpetual", "contractMultiplier": "1000"}
        ],
        "listings": [{"key": "listing:exa", "id": "undrly:listing:01m3byza2fe769x9et1aq2e3pn",
                      "instrument": "spy:037833100", "venue": "venue:XNYS", "symbol": "EXA"}],
        "relationships": [
            ["spy:037833100", "ISSUED_BY", "sec:111"],
            ["spy:037833100", "DENOMINATED_IN", usd],
            ["hyperliquid:kEXC", "DERIVES_FROM", "coingecko:examplecoin"],
            ["hyperliquid:kEXC", "SETTLES_IN", usdc]
        ],
        "aliases": [{"node": "coingecko:examplecoin", "alias": "EXC", "kind": "symbol"},
                    {"node": "spy:037833100", "alias": "EXA", "kind": "symbol"}],
        "quoteFeeds": [
            {"source": "kraken", "symbol": "EXCUSD", "subject": "coingecko:examplecoin", "unit": usd,
             "basis": "venue", "venue": kraken, "priceType": "last", "staleAfterSeconds": 300},
            {"source": "coinbase", "symbol": "EXC-USD", "subject": "coingecko:examplecoin", "unit": usd,
             "basis": "venue", "venue": coinbase, "priceType": "mid", "staleAfterSeconds": 300},
            {"source": "alpaca", "symbol": "EXA", "subject": "spy:037833100", "unit": usd,
             "basis": "venue", "venue": iex, "priceType": "last"},
            {"source": "hyperliquid", "symbol": "kEXC", "subject": "hyperliquid:kEXC", "unit": usdc,
             "basis": "venue", "venue": hl, "priceType": "mark"}
        ],
        "quoteAggregations": [{"subject": "coingecko:examplecoin", "unit": usd, "method": "mean-venue-mid-v1"}],
        "universes": [{"key": "sp500", "source": "ssga", "recordKey": "https://example.test/spy.xlsx",
                       "sha256": sha, "asOf": "2026-09-23T00:00:00Z",
                       "members": [{"node": nvda, "sourceSymbol": "NVDA"},
                                   {"node": "spy:037833100", "sourceSymbol": "EXA"}]}]
    }))
    .unwrap()
}

fn provider() -> CuratedProvider {
    CuratedProvider::with_source(SourceId::parse("undrly-universe").unwrap())
}

async fn seeded() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in SOURCES.into_iter().chain(["hyperliquid", "gold-api"]) {
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
    ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            &repo("data/demo/universe.json"),
            "2026-09-25T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    Some((db, conn))
}

#[tokio::test]
async fn snapshot_seeds_beside_v1_with_memberships_from_the_upstream_record() {
    let Some((_db, mut conn)) = seeded().await else {
        return;
    };
    let payload = snapshot(&sha256(SPY));
    // Without the upstream file stored, nothing is written.
    let err = ingest_universe_as(
        &mut conn,
        provider(),
        &raw("snapshot.json", &payload, "2026-09-25T11:00:00Z"),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, IngestError::CuratedDisagrees(_)), "{err}");
    assert!(
        get_instrument(&mut conn, InstrumentId::parse(EXA).unwrap())
            .await
            .unwrap()
            .is_none()
    );

    let mut tx = sqlx::Acquire::begin(&mut *conn).await.unwrap();
    let (upstream, _) = store_raw_record(
        &mut tx,
        &SourceId::parse("ssga").unwrap(),
        &raw("https://example.test/spy.xlsx", SPY, "2026-09-25T10:00:00Z"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let first = ingest_universe_as(
        &mut conn,
        provider(),
        &raw("snapshot.json", &payload, "2026-09-25T11:00:00Z"),
    )
    .await
    .unwrap();
    assert!(first.inserted > 0);
    let replay = ingest_universe_as(
        &mut conn,
        provider(),
        &raw("snapshot.json", &payload, "2026-09-25T12:00:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(replay.inserted, 0, "replay is a no-op");

    let sp = latest_universe_snapshot(&mut conn, UniverseKey::Sp500)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sp.source_record, upstream.id,
        "membership names the SPY record"
    );
    assert_eq!(sp.snapshot.provenance.source_id.as_str(), "ssga");
    let members: Vec<String> = sp
        .snapshot
        .members
        .iter()
        .map(|m| m.node.to_string())
        .collect();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&v1("nvda")) && members.contains(&EXA.to_owned()));

    // The equity has no identifier at all (no constructed ISIN); its issuer
    // carries the CIK.
    let exa = CanonicalId::parse(EXA).unwrap();
    assert!(
        identifiers_for_node(&mut conn, exa)
            .await
            .unwrap()
            .is_empty()
    );
    let issuer = CanonicalId::parse("undrly:entity:01m3byza2ef49tp8vytq4bg35w").unwrap();
    let ids = identifiers_for_node(&mut conn, issuer).await.unwrap();
    assert!(
        matches!(ids[0].assignment.identifier(), ExternalIdentifier::Cik(c) if c.as_str() == "0000000111")
    );
    let perp = get_instrument(&mut conn, InstrumentId::parse(PERP).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(perp.contract_multiplier.unwrap().to_string(), "1000");
    // V1 NVDA is untouched: still its pinned ISIN.
    let nvda = identifiers_for_node(&mut conn, CanonicalId::parse(&v1("nvda")).unwrap())
        .await
        .unwrap();
    assert!(nvda.iter().any(|s| matches!(s.assignment.identifier(), ExternalIdentifier::Isin(i) if i.as_str() == "US67066G1040")));
}

#[tokio::test]
async fn records_cover_only_their_requested_symbols_and_a_second_pair_is_averaged() {
    let Some((_db, mut conn)) = seeded().await else {
        return;
    };
    let mut tx = sqlx::Acquire::begin(&mut *conn).await.unwrap();
    store_raw_record(
        &mut tx,
        &SourceId::parse("ssga").unwrap(),
        &raw("https://example.test/spy.xlsx", SPY, "2026-09-25T10:00:00Z"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    ingest_universe_as(
        &mut conn,
        provider(),
        &raw(
            "snapshot.json",
            &snapshot(&sha256(SPY)),
            "2026-09-25T11:00:00Z",
        ),
    )
    .await
    .unwrap();

    let at = "2026-09-25T12:00:00Z";
    // Coinbase: one product per record. With V1's BTC-USD also declared, an
    // unscoped record would be ambiguous; scoped, the other product is
    // neither matched nor reported missing.
    let exc = [VenueSymbol::new("EXC-USD").unwrap()];
    let book =
        br#"{"bids":[["1.2","1",1]],"asks":[["1.4","1",1]],"time":"2026-09-25T12:00:00.000000Z"}"#;
    let cb = ingest_quotes_for(
        &mut conn,
        &CoinbaseProvider::new(),
        &CoinbaseNormalizer,
        &raw("coinbase EXC-USD", book, at),
        Some(&exc),
    )
    .await
    .unwrap();
    assert_eq!(cb.observations.len(), 1);
    assert!(cb.missing.is_empty());
    assert!(
        ingest_quotes_for(
            &mut conn,
            &CoinbaseProvider::new(),
            &CoinbaseNormalizer,
            &raw("coinbase all", book, at),
            None
        )
        .await
        .is_err(),
        "unscoped: a book cannot be attributed to two products"
    );

    // Alpaca: a batch that covers NVDA only does not report EXA missing.
    let nvda = [VenueSymbol::new("NVDA").unwrap()];
    let alpaca = ingest_quotes_for(
        &mut conn,
        &AlpacaProvider::new(),
        &AlpacaNormalizer,
        &raw(
            "alpaca NVDA",
            &repo("tests/fixtures/sources/alpaca/snapshots-NVDA.json"),
            at,
        ),
        Some(&nvda),
    )
    .await
    .unwrap();
    assert_eq!(alpaca.observations.len(), 1);
    assert!(alpaca.missing.is_empty());

    // Kraken: last 1.5 with bid 1.0 / ask 2.0 → mid 1.50; Coinbase mid 1.30.
    let ticker = br#"{"error":[],"result":{"EXCUSD":{"a":["2.0","1","1.000"],"b":["1.0","1","1.000"],"c":["1.5","0.1"]}}}"#;
    let kr = ingest_quotes_for(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenNormalizer,
        &raw("kraken EXCUSD", ticker, at),
        Some(&[VenueSymbol::new("EXCUSD").unwrap()]),
    )
    .await
    .unwrap();
    assert!(
        kr.missing.is_empty(),
        "V1's XXBTZUSD is outside this record"
    );
    let subject = PriceSubject::Instrument(InstrumentId::parse(EXC).unwrap());
    let unit = PriceUnit::Currency(
        v1("usd")
            .parse::<CanonicalId>()
            .unwrap()
            .try_into()
            .unwrap(),
    );
    refresh_canonical_quotes(
        &mut conn,
        &[(subject, unit)],
        Timestamp::parse("2026-09-25T12:00:10Z").unwrap(),
    )
    .await
    .unwrap();
    let q = get_canonical_quote(&mut conn, subject, unit)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.method, AggregationMethod::MeanVenueMidV1);
    assert_eq!(q.basis, ObservationBasis::Aggregated);
    assert_eq!(q.inputs.len(), 2);
    assert_eq!(q.price.to_string(), "1.400");
}
