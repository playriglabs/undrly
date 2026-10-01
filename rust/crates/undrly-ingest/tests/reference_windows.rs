//! V1.9 gold-api OHLC windows: a captured `/ohlc/XAU` response → the raw
//! record and the window for the pair gold-api's `XAU` feed prices. No
//! network.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use undrly_core::{DisplayName, Redistribution, Source, SourceId, Timestamp, VenueSymbol};
use undrly_ingest::RawRecord;
use undrly_ingest::curated::ingest_universe;
use undrly_ingest::market_data::ingest_reference_window;
use undrly_store::Write;
use undrly_store::sources::insert_source;
use undrly_store::testing::fresh;

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

#[tokio::test]
async fn stores_the_source_window_for_the_feed_pair_once() {
    let Some(db) = fresh().await else {
        return;
    };
    let mut conn = db.pool.acquire().await.unwrap();
    for s in [
        "undrly-curated",
        "kraken",
        "coinbase",
        "hyperliquid",
        "gold-api",
        "alpaca",
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
    ingest_universe(
        &mut conn,
        &raw(
            "data/demo/universe.json",
            repo("data/demo/universe.json"),
            "2026-10-01T00:00:00Z",
        ),
    )
    .await
    .unwrap();

    let record = raw(
        "https://api.gold-api.com/ohlc/XAU?startTimestamp=1790760669&endTimestamp=1790847069",
        repo("tests/fixtures/sources/gold-api/ohlc-XAU-24h.json"),
        "2026-10-01T09:31:10Z",
    );
    let xau = VenueSymbol::new("XAU").unwrap();
    assert_eq!(
        ingest_reference_window(&mut conn, &record, &xau)
            .await
            .unwrap(),
        Write::Inserted
    );
    // The same response again writes nothing.
    assert_eq!(
        ingest_reference_window(&mut conn, &record, &xau)
            .await
            .unwrap(),
        Write::Unchanged
    );
    let (subject, span, high, close): (String, String, String, String) = sqlx::query_as(
        "SELECT i.name, (w.window_end - w.window_start)::text,
                w.high::text, w.close::text
         FROM reference_windows w JOIN instruments i ON i.id = w.subject_id",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(subject.starts_with("Gold"), "{subject}");
    assert_eq!(span, "1 day", "the source's own 24-hour window");
    assert_eq!((high.as_str(), close.as_str()), ("4219.0", "4160.7002"));

    // A symbol gold-api has no feed for is stored raw and nothing more.
    let unknown = VenueSymbol::new("XYZ").unwrap();
    assert_eq!(
        ingest_reference_window(&mut conn, &record, &unknown)
            .await
            .unwrap(),
        Write::Unchanged
    );
    drop(conn);
    db.teardown().await;
}
