//! V1.3 market data (docs/v1.3-market-data.md): captured bars, perpetual
//! contexts, the equity calendar and reference history
//! → stored rows with provenance. No network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use chrono::NaiveDate;
use undrly_core::{
    BarInterval, CanonicalId, DisplayName, PriceSubject, Redistribution, Source, SourceId,
    Timestamp, VenueId, VenueSymbol,
};
use undrly_ingest::RawRecord;
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::market_data::{
    ingest_bars, ingest_calendar, ingest_corporate_actions, ingest_earnings,
    ingest_economic_release_dates, ingest_perp_contexts,
};
use undrly_ingest::quotes::{ingest_history_for, ingest_quotes};
use undrly_normalize::fx::FedH10Normalizer;
use undrly_normalize::market_data::{
    AlpacaBarNormalizer, HyperliquidBarNormalizer, KrakenBarNormalizer,
};
use undrly_provider::alpaca::AlpacaProvider;
use undrly_provider::fed_h10::FedH10Provider;
use undrly_provider::hyperliquid::HyperliquidProvider;
use undrly_provider::kraken::KrakenProvider;
use undrly_store::market_data::bar_count;
use undrly_store::sources::insert_source;
use undrly_store::testing::{TestDb, fresh};

const SOURCES: [&str; 15] = [
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
    "fred",
    "finnhub",
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

fn raw(key: &str, payload: Vec<u8>, at: &str) -> RawRecord {
    RawRecord {
        record_key: key.to_owned(),
        payload,
        received_at: Timestamp::parse(at).unwrap(),
    }
}

fn v1(key: &str) -> CanonicalId {
    let u: serde_json::Value = serde_json::from_slice(&repo("data/demo/universe.json")).unwrap();
    for group in ["venues", "instruments", "currencies"] {
        for o in u[group].as_array().unwrap() {
            if o["key"] == key {
                return CanonicalId::parse(o["id"].as_str().unwrap()).unwrap();
            }
        }
    }
    panic!("no key {key}")
}

fn subject(key: &str) -> PriceSubject {
    PriceSubject::Instrument(v1(key).try_into().unwrap())
}

fn sym(s: &str) -> Vec<VenueSymbol> {
    vec![VenueSymbol::new(s).unwrap()]
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

#[tokio::test]
async fn venue_bars_are_stored_once_and_replaced_only_by_newer_records() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let key = "https://api.kraken.com/0/public/OHLC?pair=XXBTZUSD&interval=60&since=1790377200";
    let payload = fixture("kraken/ohlc-XXBTZUSD-60.json");
    let r = ingest_bars(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenBarNormalizer(BarInterval::OneHour),
        &raw(key, payload.clone(), "2026-09-26T04:30:00Z"),
        &sym("XXBTZUSD"),
    )
    .await
    .unwrap();
    assert_eq!((r.inserted, r.replaced, r.unchanged), (6, 0, 0));
    // Replay: the same record, nothing changes.
    let r = ingest_bars(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenBarNormalizer(BarInterval::OneHour),
        &raw(key, payload.clone(), "2026-09-26T04:30:00Z"),
        &sym("XXBTZUSD"),
    )
    .await
    .unwrap();
    assert_eq!((r.inserted, r.replaced, r.unchanged), (0, 0, 6));
    // A later fetch where the in-progress bar moved: only it is replaced.
    let mut later: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    let bars = later["result"]["XXBTZUSD"].as_array_mut().unwrap();
    let last = bars.last_mut().unwrap().as_array_mut().unwrap();
    last[6] = "999.00000000".into();
    let r = ingest_bars(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenBarNormalizer(BarInterval::OneHour),
        &raw(
            key,
            serde_json::to_vec(&later).unwrap(),
            "2026-09-26T04:40:00Z",
        ),
        &sym("XXBTZUSD"),
    )
    .await
    .unwrap();
    assert_eq!((r.inserted, r.replaced, r.unchanged), (0, 1, 5));
    // An older record never overwrites a newer one.
    let r = ingest_bars(
        &mut conn,
        &KrakenProvider::new(),
        &KrakenBarNormalizer(BarInterval::OneHour),
        &raw(
            &format!("{key}#old"),
            payload.clone(),
            "2026-09-26T04:35:00Z",
        ),
        &sym("XXBTZUSD"),
    )
    .await
    .unwrap();
    assert_eq!((r.inserted, r.replaced), (0, 0));
    assert_eq!(bar_count(&mut conn, subject("btc"), "1h").await.unwrap(), 6);
    // Every bar names a raw record of the source; the in-progress bar is
    // marked incomplete.
    let rows: Vec<(String, bool)> = sqlx::query_as(
        "SELECT r.source_id::text, b.close_time <= b.received_at FROM market_bars b
         JOIN source_records r ON r.id = b.source_record_id ORDER BY b.open_time",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(rows.iter().all(|(s, _)| s == "kraken"));
    assert_eq!(rows.iter().filter(|(_, complete)| !complete).count(), 1);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn perp_candles_and_contexts_trace_to_their_records() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let r = ingest_bars(
        &mut conn,
        &HyperliquidProvider::new(),
        &HyperliquidBarNormalizer,
        &raw(
            "POST https://api.hyperliquid.xyz/info candles",
            fixture("hyperliquid/candles-BTC-1h.json"),
            "2026-09-26T04:30:00Z",
        ),
        &sym("BTC"),
    )
    .await
    .unwrap();
    assert_eq!(r.inserted, 5);
    let ctx = raw(
        "POST https://api.hyperliquid.xyz/info {\"type\":\"metaAndAssetCtxs\"}",
        fixture("hyperliquid/metaAndAssetCtxs.json"),
        "2026-09-26T04:30:00Z",
    );
    // The same record gives the mark-price quote and the context.
    ingest_quotes(
        &mut conn,
        &HyperliquidProvider::new(),
        &undrly_normalize::hyperliquid::HyperliquidNormalizer,
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(
        ingest_perp_contexts(&mut conn, &ctx, &sym("BTC"))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ingest_perp_contexts(&mut conn, &ctx, &sym("BTC"))
            .await
            .unwrap(),
        0
    );
    let (oi, funding, hours, same_record): (String, String, i32, bool) = sqlx::query_as(
        "SELECT c.open_interest::text, c.funding_rate::text, c.funding_interval_hours,
                c.source_record_id = o.source_record_id
         FROM perp_contexts c JOIN market_observations o ON o.subject_id = c.subject_id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        (oi.as_str(), funding.as_str(), hours),
        ("39202.60446", "0.0000125", 1)
    );
    assert!(same_record);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn equity_bars_and_calendar() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let r = ingest_bars(
        &mut conn,
        &AlpacaProvider::new(),
        &AlpacaBarNormalizer(BarInterval::OneDay),
        &raw(
            "https://data.alpaca.markets/v2/stocks/bars?symbols=AAPL,NVDA&timeframe=1Day",
            fixture("alpaca/bars-1Day.json"),
            "2026-09-26T04:30:00Z",
        ),
        &sym("NVDA"),
    )
    .await
    .unwrap();
    // Only NVDA has a feed in the V1 universe; AAPL is not requested.
    assert_eq!(r.inserted, 5);
    let iex = VenueId::try_from(v1("iex")).unwrap();
    let n = ingest_calendar(
        &mut conn,
        &SourceId::parse("alpaca").unwrap(),
        &raw(
            "https://paper-api.alpaca.markets/v2/calendar?start=2026-09-21&end=2026-12-31",
            fixture("alpaca/calendar.json"),
            "2026-09-26T04:30:00Z",
        ),
        iex,
        NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(n, 72);
    let thanksgiving: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM trading_sessions WHERE session_date = '2026-11-26'",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(thanksgiving, 0, "no session on a holiday");
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn reference_history_stores_every_dated_value_idempotently() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    // The FX universe declares H.10's USD/JPY feed; its spec is stored first.
    undrly_ingest::store_raw_record(
        &mut conn,
        &SourceId::parse("undrly-curated").unwrap(),
        &raw(
            "data/reference/fx-spec.json",
            repo("data/reference/fx-spec.json"),
            "2026-09-26T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    ingest_universe(
        &mut conn,
        &raw(
            "data/reference/fx.json",
            repo("data/reference/fx.json"),
            "2026-09-26T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    let key = undrly_provider::fed_h10::PACKAGE_URL;
    let first = ingest_history_for(
        &mut conn,
        &FedH10Provider::new(),
        &FedH10Normalizer,
        &raw(
            key,
            fixture("fed-h10/h10-lastobs10.csv"),
            "2026-09-26T04:30:00Z",
        ),
        Some(&sym("RXI_N.B.JA")),
    )
    .await
    .unwrap();
    // Ten business days, one of them `ND` (no rate): nine observations.
    assert_eq!(first.observations.len(), 9);
    assert!(
        first
            .observations
            .iter()
            .all(|(_, _, w)| *w == undrly_store::Write::Inserted)
    );
    let again = ingest_history_for(
        &mut conn,
        &FedH10Provider::new(),
        &FedH10Normalizer,
        &raw(
            key,
            fixture("fed-h10/h10-lastobs10.csv"),
            "2026-09-26T05:30:00Z",
        ),
        Some(&sym("RXI_N.B.JA")),
    )
    .await
    .unwrap();
    assert!(
        again
            .observations
            .iter()
            .all(|(_, _, w)| *w == undrly_store::Write::Unchanged),
        "an overlapping later payload restates the same observations"
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn corporate_actions_attach_to_equities_and_accept_revisions() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let key = "https://data.alpaca.markets/v1/corporate-actions?symbols=NVDA";
    let payload = fixture("alpaca/corporate-actions.json");
    // Only NVDA has an Alpaca feed in the V1 universe.
    let r = ingest_corporate_actions(
        &mut conn,
        &raw(key, payload.clone(), "2026-09-26T04:30:00Z"),
        &sym("NVDA"),
    )
    .await
    .unwrap();
    assert!(r.inserted > 0);
    assert_eq!((r.replaced, r.unchanged), (0, 0));
    let rows: Vec<(String, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT action_type, role, cash_amount::text, r.source_id::text = 'alpaca'
         FROM corporate_actions a JOIN source_records r ON r.id = a.source_record_id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(rows.len(), r.inserted);
    assert!(
        rows.iter()
            .all(|(t, role, cash, traced)| t == "cash_dividend"
                && role == "subject"
                && cash.is_some()
                && *traced)
    );
    // Replay: unchanged.
    let again = ingest_corporate_actions(
        &mut conn,
        &raw(key, payload.clone(), "2026-09-26T04:30:00Z"),
        &sym("NVDA"),
    )
    .await
    .unwrap();
    assert_eq!((again.inserted, again.replaced), (0, 0));
    // A later response revising one dividend's payable date replaces it.
    let mut revised: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    let divs = revised["corporate_actions"]["cash_dividends"]
        .as_array_mut()
        .unwrap();
    let nvda = divs.iter_mut().find(|d| d["symbol"] == "NVDA").unwrap();
    nvda["payable_date"] = "2026-12-31".into();
    let later = ingest_corporate_actions(
        &mut conn,
        &raw(
            key,
            serde_json::to_vec(&revised).unwrap(),
            "2026-09-27T04:30:00Z",
        ),
        &sym("NVDA"),
    )
    .await
    .unwrap();
    assert_eq!((later.inserted, later.replaced), (0, 1));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn economic_release_dates_and_earnings_trace_to_their_records() {
    let Some((db, mut conn)) = seeded().await else {
        return;
    };
    let window = (
        NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    );
    let key = "https://api.stlouisfed.org/fred/release/dates?release_id=10&file_type=json";
    let n = ingest_economic_release_dates(
        &mut conn,
        &raw(
            key,
            fixture("fred/release-dates-10.json"),
            "2026-09-26T04:30:00Z",
        ),
        "10",
        "Consumer Price Index",
        "inflation",
        window,
    )
    .await
    .unwrap();
    assert_eq!(n, 5);
    // A replay is the same record: nothing new.
    let again = ingest_economic_release_dates(
        &mut conn,
        &raw(
            key,
            fixture("fred/release-dates-10.json"),
            "2026-09-26T04:30:00Z",
        ),
        "10",
        "Consumer Price Index",
        "inflation",
        window,
    )
    .await
    .unwrap();
    assert_eq!(again, 0);
    // A response for another release is rejected, not relabelled.
    assert!(
        ingest_economic_release_dates(
            &mut conn,
            &raw(
                &format!("{key}#50"),
                fixture("fred/release-dates-10.json"),
                "2026-09-26T04:31:00Z"
            ),
            "50",
            "Employment Situation",
            "labor",
            window,
        )
        .await
        .is_err()
    );
    // Earnings: only NVDA has an equity feed in the V1 universe.
    let r = ingest_earnings(
        &mut conn,
        &raw(
            "https://finnhub.io/api/v1/calendar/earnings?from=2026-05-01&to=2026-12-31",
            fixture("finnhub/earnings-calendar.json"),
            "2026-09-26T04:30:00Z",
        ),
        &sym("NVDA"),
    )
    .await
    .unwrap();
    assert_eq!(r.inserted, 3);
    let (time, eps): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT report_time, eps_estimate::text FROM earnings_events
         WHERE fiscal_year = 2027 AND fiscal_quarter = 3",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        (time.as_deref(), eps.as_deref()),
        (Some("after_close"), Some("1.2501"))
    );
    drop(conn);
    db.teardown().await;
}
