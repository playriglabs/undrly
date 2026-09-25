//! SEC EDGAR adapter, deterministic: a captured NVIDIA submissions document
//! and a local HTTP server stand in for data.sec.gov. No internet access.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI. The live
//! counterpart is `sec_live.rs` (ignored by default).

use std::path::Path;

use sqlx::PgConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use undrly_core::{
    Cik, DisplayName, Entity, EntityId, EntityKind, ExternalIdentifier, IdentifierAssignment, Isin,
    Lei, Redistribution, Source, SourceId, Timestamp, Validity,
};
use undrly_ingest::sec::{fetch_and_ingest_company, ingest_company};
use undrly_ingest::{IngestError, RawRecord, Resolution, ingest_reference};
use undrly_normalize::fixture::FixtureNormalizer;
use undrly_provider::fixture::FixtureProvider;
use undrly_provider::sec::SOURCE_ID;
use undrly_provider::sec::http::{FetchError, SecClient, SecUserAgent};
use undrly_store::identifiers::{
    AssignOutcome, assign_identifier, identifiers_for_node, resolve_identifier,
};
use undrly_store::sources::{
    SourceRecord, facts_from_source_record, get_source_record, insert_source, insert_source_record,
};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, reference};

const URL: &str = "https://data.sec.gov/submissions/CIK0001045810.json";
const RECEIVED_AT: &str = "2026-09-25T02:14:18.123456Z";
const USER_AGENT: &str = "Undrly tests tests@example.com";

fn captured() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/sources/sec-edgar/CIK0001045810.json");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// The captured document with one textual edit.
fn edited(from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(captured()).unwrap();
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1).into_bytes()
}

fn raw(payload: Vec<u8>, received_at: &str) -> RawRecord {
    RawRecord {
        record_key: URL.to_owned(),
        payload,
        received_at: Timestamp::parse(received_at).unwrap(),
    }
}

fn nvidia_cik() -> Cik {
    Cik::parse("0001045810").unwrap()
}

fn cik_id() -> ExternalIdentifier {
    ExternalIdentifier::Cik(nvidia_cik())
}

fn at() -> Timestamp {
    Timestamp::parse("2026-09-25T00:00:00Z").unwrap()
}

async fn register(conn: &mut PgConnection, id: &str) {
    insert_source(
        conn,
        &Source {
            id: SourceId::parse(id).unwrap(),
            name: DisplayName::new("Test source").unwrap(),
            redistribution: Redistribution::Unknown,
        },
    )
    .await
    .unwrap();
}

async fn setup() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    register(&mut conn, SOURCE_ID).await;
    Some((db, conn))
}

/// Row counts of `source_records` and every table ingestion can write.
async fn counts(conn: &mut PgConnection) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for table in [
        "source_records",
        "nodes",
        "entities",
        "instruments",
        "venues",
        "currencies",
        "listings",
        "identifiers",
        "listing_symbols",
        "identifier_conflicts",
        "graph_edges",
    ] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        out.push((table.to_owned(), n));
    }
    out
}

fn count(counts: &[(String, i64)], table: &str) -> i64 {
    counts.iter().find(|(t, _)| t == table).unwrap().1
}

#[tokio::test]
async fn first_ingestion_mints_only_the_nvidia_entity_and_its_cik() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest_company(&mut conn, &nvidia_cik(), &raw(captured(), RECEIVED_AT))
        .await
        .unwrap();
    let Resolution::Created(entity_id) = report.entity else {
        panic!("{:?}", report.entity)
    };
    assert_eq!(report.source_record.1, Write::Inserted);
    assert!(matches!(
        report.identifiers.as_slice(),
        [(id, AssignOutcome::Assigned(_))] if *id == cik_id()
    ));

    let entity = reference::get_entity(&mut conn, entity_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (entity.kind, entity.name.as_str()),
        (EntityKind::Company, "NVIDIA CORP")
    );
    let resolved = resolve_identifier(&mut conn, &cik_id(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.assignment.node(), entity_id.into());
    assert_eq!(
        resolved.assignment.provenance().source_id.as_str(),
        SOURCE_ID
    );

    // SEC asserts identity only: no instrument, listing, venue, symbol,
    // currency, ISIN, FIGI, or edge is produced from its tickers/exchanges.
    let after = counts(&mut conn).await;
    for (table, expected) in [
        ("source_records", 2), // the harness's seed record + SEC's
        ("nodes", 1),
        ("entities", 1),
        ("identifiers", 1),
        ("instruments", 0),
        ("venues", 0),
        ("currencies", 0),
        ("listings", 0),
        ("listing_symbols", 0),
        ("identifier_conflicts", 0),
        ("graph_edges", 0),
    ] {
        assert_eq!(count(&after, table), expected, "{table}");
    }
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn raw_sec_bytes_survive_and_every_fact_traces_to_that_record() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest_company(&mut conn, &nvidia_cik(), &raw(captured(), RECEIVED_AT))
        .await
        .unwrap();
    let record_id = report.source_record.0;

    let record = get_source_record(&mut conn, record_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.payload, captured(), "byte for byte");
    assert_eq!(record.source_id.as_str(), SOURCE_ID);
    assert_eq!(record.record_key, URL);
    assert_eq!(record.received_at, Timestamp::parse(RECEIVED_AT).unwrap());
    let hash_matches: bool = sqlx::query_scalar(
        "SELECT payload_sha256 = sha256($2) AND length(payload) = $3
         FROM source_records WHERE id = $1",
    )
    .bind(record_id.0)
    .bind(captured())
    .bind(i32::try_from(captured().len()).unwrap())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(hash_matches);

    let entity = report.entity.id();
    let facts = facts_from_source_record(&mut conn, record_id)
        .await
        .unwrap();
    assert_eq!(facts.nodes, vec![entity.into()]);
    assert_eq!(facts.identifiers.len(), 1);
    assert!(
        facts.listing_symbols.is_empty()
            && facts.relationships.is_empty()
            && facts.conflicts.is_empty()
    );
    assert_eq!(
        reference::object_source_record(&mut conn, entity.into())
            .await
            .unwrap(),
        Some(record_id)
    );
    let identifiers = identifiers_for_node(&mut conn, entity.into())
        .await
        .unwrap();
    assert_eq!(identifiers.len(), 1);
    assert_eq!(identifiers[0].id, facts.identifiers[0]);
    assert_eq!(identifiers[0].source_record, record_id);
    assert_eq!(
        identifiers[0].assignment.provenance().received_at,
        record.received_at
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn replay_is_idempotent() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let first = ingest_company(&mut conn, &nvidia_cik(), &raw(captured(), RECEIVED_AT))
        .await
        .unwrap();
    let before = counts(&mut conn).await;

    // The same bytes fetched again later: nothing changes.
    let replay = ingest_company(
        &mut conn,
        &nvidia_cik(),
        &raw(captured(), "2026-09-26T00:00:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(
        replay.source_record,
        (first.source_record.0, Write::Unchanged)
    );
    assert_eq!(replay.entity, Resolution::Existing(first.entity.id()));
    assert!(matches!(
        replay.identifiers.as_slice(),
        [(_, AssignOutcome::Unchanged(_))]
    ));
    assert_eq!(counts(&mut conn).await, before);

    // SEC's document changes with every new filing. A changed document is a
    // new raw record; identity is unchanged, and the new record asserts
    // nothing new (its confirmation is not recorded: corroboration is
    // deferred).
    let updated = ingest_company(
        &mut conn,
        &nvidia_cik(),
        &raw(
            edited("\"phone\":\"408-486-2000\"", "\"phone\":\"408-486-2001\""),
            "2026-09-27T00:00:00Z",
        ),
    )
    .await
    .unwrap();
    assert_ne!(updated.source_record.0, first.source_record.0);
    assert_eq!(updated.source_record.1, Write::Inserted);
    assert_eq!(updated.entity, Resolution::Existing(first.entity.id()));
    assert!(
        facts_from_source_record(&mut conn, updated.source_record.0)
            .await
            .unwrap()
            .is_empty()
    );
    let after = counts(&mut conn).await;
    assert_eq!(
        count(&after, "source_records"),
        count(&before, "source_records") + 1
    );
    assert_eq!(count(&after, "entities"), 1);
    assert_eq!(count(&after, "identifiers"), 1);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn an_existing_entity_with_the_same_cik_is_reused() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    // An entity already identified by NVIDIA's CIK (from another record).
    register(&mut conn, "other-source").await;
    let (other, _) = insert_source_record(
        &mut conn,
        &SourceRecord {
            source_id: SourceId::parse("other-source").unwrap(),
            record_key: "entity".into(),
            payload: b"entity".to_vec(),
            received_at: Timestamp::parse("2026-09-01T00:00:00Z").unwrap(),
        },
    )
    .await
    .unwrap();
    let existing = EntityId::generate();
    reference::insert_entity(
        &mut conn,
        &Entity {
            id: existing,
            kind: EntityKind::Company,
            name: DisplayName::new("NVIDIA Corporation").unwrap(),
        },
        other.id,
    )
    .await
    .unwrap();
    let claim = IdentifierAssignment::new(
        cik_id(),
        existing.into(),
        Validity::UNBOUNDED,
        other.provenance.clone(),
    )
    .unwrap();
    assign_identifier(&mut conn, &claim, other.id)
        .await
        .unwrap();

    let report = ingest_company(&mut conn, &nvidia_cik(), &raw(captured(), RECEIVED_AT))
        .await
        .unwrap();
    assert_eq!(report.entity, Resolution::Existing(existing));
    assert!(matches!(
        report.identifiers.as_slice(),
        [(_, AssignOutcome::Unchanged(_))]
    ));
    // The existing entity is not modified by SEC's differing name.
    let entity = reference::get_entity(&mut conn, existing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entity.name.as_str(), "NVIDIA Corporation");
    assert_eq!(
        reference::object_source_record(&mut conn, existing.into())
            .await
            .unwrap(),
        Some(other.id)
    );
    assert_eq!(count(&counts(&mut conn).await, "entities"), 1);
    drop(conn);
    db.teardown().await;
}

/// Known mismatch with the fixture model: the NVIDIA entity from the
/// reference fixture is identified by LEI, and SEC reports no LEI for
/// NVIDIA. Without a shared identifier the two records resolve to two
/// entities; nothing links them by name. Merging is deferred.
#[tokio::test]
async fn sec_and_lei_only_records_do_not_link_without_a_shared_identifier() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    register(&mut conn, "reference-fixture").await;
    let fixture_payload = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sources/reference-fixture/nvda.json"),
    )
    .unwrap();
    let fixture = ingest_reference(
        &mut conn,
        &FixtureProvider::new(SourceId::parse("reference-fixture").unwrap()),
        &FixtureNormalizer,
        &RawRecord {
            record_key: "nvda.json".into(),
            payload: fixture_payload,
            received_at: Timestamp::parse(RECEIVED_AT).unwrap(),
        },
    )
    .await
    .unwrap();
    let sec = ingest_company(&mut conn, &nvidia_cik(), &raw(captured(), RECEIVED_AT))
        .await
        .unwrap();
    assert!(matches!(sec.entity, Resolution::Created(_)));
    assert_ne!(sec.entity.id(), fixture.entity.id());

    // If SEC did report NVIDIA's LEI, the record would resolve to the
    // LEI-identified entity and link the CIK to it.
    let with_lei = ingest_company(
        &mut conn,
        &nvidia_cik(),
        &raw(
            edited("\"lei\":null", "\"lei\":\"549300S4KLFTLO7GSQ80\""),
            RECEIVED_AT,
        ),
    )
    .await;
    // ...but the CIK already maps to the SEC-minted entity, so the record
    // names two entities and is rejected as ambiguous, writing nothing.
    assert!(
        matches!(with_lei, Err(IngestError::Ambiguous { .. })),
        "{with_lei:?}"
    );
    assert!(
        identifiers_for_node(&mut conn, fixture.entity.id().into())
            .await
            .unwrap()
            .iter()
            .all(|s| s.assignment.identifier().namespace() != undrly_core::Namespace::Cik)
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn an_sec_lei_links_the_cik_to_the_lei_identified_entity() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    register(&mut conn, "reference-fixture").await;
    let fixture = ingest_reference(
        &mut conn,
        &FixtureProvider::new(SourceId::parse("reference-fixture").unwrap()),
        &FixtureNormalizer,
        &RawRecord {
            record_key: "nvda.json".into(),
            payload: std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../tests/fixtures/sources/reference-fixture/nvda.json"),
            )
            .unwrap(),
            received_at: Timestamp::parse(RECEIVED_AT).unwrap(),
        },
    )
    .await
    .unwrap();
    let report = ingest_company(
        &mut conn,
        &nvidia_cik(),
        &raw(
            edited("\"lei\":null", "\"lei\":\"549300S4KLFTLO7GSQ80\""),
            RECEIVED_AT,
        ),
    )
    .await
    .unwrap();
    assert_eq!(report.entity, Resolution::Existing(fixture.entity.id()));
    let ids: Vec<ExternalIdentifier> = identifiers_for_node(&mut conn, fixture.entity.id().into())
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.assignment.identifier().clone())
        .collect();
    assert_eq!(
        ids,
        vec![
            ExternalIdentifier::Lei(Lei::parse("549300S4KLFTLO7GSQ80").unwrap()),
            cik_id()
        ]
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn malformed_or_unsupported_responses_write_nothing() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let empty = counts(&mut conn).await;
    let truncated = captured()[..4096].to_vec();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        (
            "html",
            b"<html><body>Your Request Originates from an Undeclared Automated Tool</body></html>"
                .to_vec(),
        ),
        ("truncated", truncated),
        (
            "unsupported entity type",
            edited("\"entityType\":\"operating\"", "\"entityType\":\"other\""),
        ),
        (
            "invalid cik",
            edited("\"cik\":\"0001045810\"", "\"cik\":\"0000000000\""),
        ),
        (
            "invalid lei",
            edited("\"lei\":null", "\"lei\":\"549300S4KLFTLO7GSQ81\""),
        ),
        ("missing name", edited("\"name\":\"NVIDIA CORP\",", "")),
    ];
    for (case, payload) in cases {
        let result = ingest_company(&mut conn, &nvidia_cik(), &raw(payload, RECEIVED_AT)).await;
        assert!(
            matches!(
                result,
                Err(IngestError::Decode(_) | IngestError::Normalize(_))
            ),
            "{case}: {result:?}"
        );
        assert_eq!(counts(&mut conn).await, empty, "{case}");
    }

    // A valid document for a different filer than requested.
    let apple = Cik::parse("0000320193").unwrap();
    let result = ingest_company(&mut conn, &apple, &raw(captured(), RECEIVED_AT)).await;
    assert!(
        matches!(result, Err(IngestError::UnexpectedRecord { .. })),
        "{result:?}"
    );
    assert_eq!(counts(&mut conn).await, empty);

    // No identifier that can resolve an entity: nothing is guessed.
    assert!(matches!(
        undrly_ingest::ingest_entity(
            &mut conn,
            &undrly_provider::sec::SecProvider::new(),
            &NoIdentifiers,
            &raw(captured(), RECEIVED_AT),
        )
        .await,
        Err(IngestError::NoEntityPrimaryIdentifier(_))
    ));
    assert_eq!(counts(&mut conn).await, empty);
    drop(conn);
    db.teardown().await;
}

/// A normalizer that claims an ISIN for an entity.
struct NoIdentifiers;

impl undrly_normalize::EntityNormalizer for NoIdentifiers {
    type Record = undrly_provider::sec::CompanySubmissions;

    fn normalize_entity(
        &self,
        _: &Self::Record,
    ) -> Result<undrly_normalize::NormalizedEntityRecord, undrly_normalize::NormalizeError> {
        Ok(undrly_normalize::NormalizedEntityRecord {
            kind: EntityKind::Company,
            name: DisplayName::new("X").unwrap(),
            identifiers: vec![ExternalIdentifier::Isin(
                Isin::parse("US67066G1040").unwrap(),
            )],
        })
    }
}

// --- transport -------------------------------------------------------------------

enum Reply {
    Respond(Vec<u8>),
    /// Accept, read the request, and close without responding.
    Hangup,
}

/// Serves one connection on a local port and returns what was requested.
async fn serve_once(reply: Reply) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
        }
        if let Reply::Respond(bytes) = reply {
            socket.write_all(&bytes).await.unwrap();
        }
        socket.shutdown().await.ok();
        String::from_utf8(request).unwrap()
    });
    (base, handle)
}

fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         x-amzn-requestid: test-request-id\r\ndate: Fri, 25 Sep 2026 02:14:18 GMT\r\n\
         connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn client(base: &str) -> SecClient {
    SecClient::with_base_url(SecUserAgent::new(USER_AGENT).unwrap(), base).unwrap()
}

#[tokio::test]
async fn fetched_bytes_are_stored_unchanged_with_request_metadata() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let (base, server) = serve_once(Reply::Respond(http_response("200 OK", &captured()))).await;
    let (fetched, report) = fetch_and_ingest_company(&mut conn, &client(&base), &nvidia_cik())
        .await
        .unwrap();
    let request = server.await.unwrap();

    // Conservative, declared request: one GET with the configured
    // User-Agent, no compression negotiated.
    assert!(
        request.starts_with("GET /submissions/CIK0001045810.json HTTP/1.1\r\n"),
        "{request}"
    );
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains(&format!(
        "user-agent: {}\r\n",
        USER_AGENT.to_ascii_lowercase()
    )));
    assert!(!lower.contains("accept-encoding"), "{request}");

    assert_eq!(fetched.body, captured());
    assert_eq!(fetched.request_id.as_deref(), Some("test-request-id"));
    assert_eq!(
        fetched.date.as_deref(),
        Some("Fri, 25 Sep 2026 02:14:18 GMT")
    );
    let record = get_source_record(&mut conn, report.source_record.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.payload, captured());
    assert_eq!(
        record.record_key,
        format!("{base}/submissions/CIK0001045810.json")
    );
    assert_eq!(record.received_at, fetched.received_at);
    assert!(matches!(report.entity, Resolution::Created(_)));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn network_and_provider_failures_write_nothing() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let empty = counts(&mut conn).await;

    for (case, reply) in [
        (
            "rate limited",
            Reply::Respond(http_response(
                "403 Forbidden",
                b"Request Rate Threshold Exceeded",
            )),
        ),
        (
            "server error",
            Reply::Respond(http_response("500 Internal Server Error", b"")),
        ),
        (
            "not found",
            Reply::Respond(http_response("404 Not Found", b"")),
        ),
        (
            "redirect",
            Reply::Respond(
                b"HTTP/1.1 301 Moved Permanently\r\nlocation: http://127.0.0.1:1/\r\n\
                  content-length: 0\r\nconnection: close\r\n\r\n"
                    .to_vec(),
            ),
        ),
        ("hangup", Reply::Hangup),
        (
            "body shorter than declared",
            Reply::Respond({
                let mut r = http_response("200 OK", &captured());
                r.truncate(r.len() - 1000);
                r
            }),
        ),
        (
            "200 with an error page",
            Reply::Respond(http_response("200 OK", b"<html>maintenance</html>")),
        ),
    ] {
        let (base, server) = serve_once(reply).await;
        let result = fetch_and_ingest_company(&mut conn, &client(&base), &nvidia_cik()).await;
        server.await.unwrap();
        assert!(result.is_err(), "{case}");
        assert_eq!(counts(&mut conn).await, empty, "{case}");
    }

    // Nothing listening.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let result = fetch_and_ingest_company(&mut conn, &client(&base), &nvidia_cik()).await;
    assert!(
        matches!(result, Err(IngestError::Fetch(FetchError::Request { .. }))),
        "{result:?}"
    );
    assert_eq!(counts(&mut conn).await, empty);
    drop(conn);
    db.teardown().await;
}
