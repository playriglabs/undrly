//! Live SEC EDGAR integration: fetches NVIDIA's current submissions document
//! from data.sec.gov and runs it through the full ingestion/provenance path.
//!
//! Ignored by default; the normal suite never touches the internet. Run with:
//!
//! ```sh
//! UNDRLY_SEC_USER_AGENT="Your Name you@example.com" \
//! DATABASE_URL=postgres://... \
//! cargo test -p undrly-ingest --test sec_live -- --ignored --nocapture
//! ```
//!
//! Makes one request (plus none on replay: the replay reuses the fetched
//! bytes).

use undrly_core::{
    Cik, DisplayName, EntityKind, ExternalIdentifier, Redistribution, Source, SourceId,
};
use undrly_ingest::Resolution;
use undrly_ingest::sec::{fetch_and_ingest_company, ingest_company, raw_record};
use undrly_provider::ReferenceDataProvider;
use undrly_provider::sec::http::{SecClient, SecUserAgent};
use undrly_provider::sec::{SOURCE_ID, SecProvider};
use undrly_store::identifiers::identifiers_for_node;
use undrly_store::sources::{facts_from_source_record, get_source_record, insert_source};
use undrly_store::testing::fresh;
use undrly_store::{Write, reference};

#[tokio::test]
#[ignore = "live network: fetches from data.sec.gov (see module docs)"]
async fn live_nvidia_from_sec_edgar() {
    let user_agent = std::env::var("UNDRLY_SEC_USER_AGENT")
        .expect("set UNDRLY_SEC_USER_AGENT to \"Name contact@example.com\"");
    let client = SecClient::new(SecUserAgent::new(&user_agent).unwrap()).unwrap();
    let db = fresh()
        .await
        .expect("the live test needs DATABASE_URL (it writes to a fresh test database)");
    let mut conn = db.pool.acquire().await.unwrap();
    insert_source(
        &mut conn,
        &Source {
            id: SourceId::parse(SOURCE_ID).unwrap(),
            name: DisplayName::new("SEC EDGAR").unwrap(),
            redistribution: Redistribution::Unknown,
        },
    )
    .await
    .unwrap();

    let cik = Cik::normalize("1045810").unwrap();
    let (fetched, report) = fetch_and_ingest_company(&mut conn, &client, &cik)
        .await
        .unwrap();
    let decoded = SecProvider::new().decode_reference(&fetched.body).unwrap();
    println!("url:          {}", fetched.url);
    println!("request id:   {:?}", fetched.request_id);
    println!("date:         {:?}", fetched.date);
    println!("received_at:  {}", fetched.received_at);
    println!("bytes:        {}", fetched.body.len());
    println!(
        "sec says:     cik={} entityType={} name={:?} lei={:?} tickers={:?} exchanges={:?}",
        decoded.cik,
        decoded.entity_type,
        decoded.name,
        decoded.lei,
        decoded.tickers,
        decoded.exchanges
    );

    // Raw bytes stored unchanged, as the record every fact derives from.
    let record_id = report.source_record.0;
    let record = get_source_record(&mut conn, record_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.payload, fetched.body);
    assert_eq!(record.record_key, fetched.url);
    assert_eq!(record.received_at, fetched.received_at);

    let Resolution::Created(entity_id) = report.entity else {
        panic!(
            "fresh database: expected a new entity, got {:?}",
            report.entity
        )
    };
    let entity = reference::get_entity(&mut conn, entity_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entity.kind, EntityKind::Company);
    let identifiers = identifiers_for_node(&mut conn, entity_id.into())
        .await
        .unwrap();
    assert!(
        identifiers
            .iter()
            .any(|s| *s.assignment.identifier() == ExternalIdentifier::Cik(cik.clone()))
    );
    assert!(identifiers.iter().all(|s| s.source_record == record_id));
    let facts = facts_from_source_record(&mut conn, record_id)
        .await
        .unwrap();
    assert_eq!(facts.nodes, vec![entity_id.into()]);
    assert!(facts.relationships.is_empty() && facts.listing_symbols.is_empty());
    println!(
        "entity:       {} {:?} {:?}",
        entity_id,
        entity.kind,
        entity.name.as_str()
    );
    for stored in &identifiers {
        println!(
            "identifier:   {} {} (source record {})",
            stored.assignment.identifier().namespace(),
            stored.assignment.identifier().value(),
            stored.source_record.0
        );
    }
    println!("derived:      {facts:?}");

    // Replaying the fetched bytes is idempotent.
    let replay = ingest_company(&mut conn, &cik, &raw_record(&fetched))
        .await
        .unwrap();
    assert_eq!(replay.source_record, (record_id, Write::Unchanged));
    assert_eq!(replay.entity, Resolution::Existing(entity_id));

    drop(conn);
    db.teardown().await;
}
