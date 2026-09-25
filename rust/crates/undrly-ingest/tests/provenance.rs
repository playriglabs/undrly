//! Source-record provenance: every fact ingestion writes is traceable to the
//! exact raw record that asserted it (fact → source record → source).
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::collections::BTreeSet;
use std::path::Path;

use sqlx::{Acquire, PgConnection};
use undrly_core::{
    CanonicalId, DisplayName, ExternalIdentifier, Figi, Isin, Provenance, Redistribution,
    RelationshipType, Source, SourceId, Timestamp, VenueSymbol,
};
use undrly_ingest::{IngestError, IngestReport, RawRecord, Resolution, ingest_reference};
use undrly_normalize::fixture::FixtureNormalizer;
use undrly_provider::fixture::FixtureProvider;
use undrly_store::conflicts::{
    ConflictClaim, conflicts_for_identifier, conflicts_for_listing_symbol,
};
use undrly_store::identifiers::{identifiers_for_node, resolve_identifier};
use undrly_store::listing_symbols::{resolve_listing_symbol, symbols_for_listing};
use undrly_store::sources::{
    DerivedFacts, SourceRecordId, facts_from_source_record, get_source_record, insert_source,
};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph, reference};

const FIXTURE_SOURCE: &str = "reference-fixture";
const SECOND_SOURCE: &str = "second-reference-fixture";
const RECEIVED_AT: &str = "2026-09-24T12:00:00.123456Z";

/// Tables holding source-derived facts, all of which carry `source_record_id`.
const FACT_TABLES: [&str; 9] = [
    "entities",
    "instruments",
    "venues",
    "currencies",
    "listings",
    "identifiers",
    "listing_symbols",
    "graph_edges",
    "identifier_conflicts",
];

fn payload(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/sources/reference-fixture")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// `name`'s payload with textual replacements, for record revisions.
fn revised(name: &str, replacements: &[(&str, &str)]) -> Vec<u8> {
    let mut text = String::from_utf8(payload(name)).unwrap();
    for (from, to) in replacements {
        assert!(text.contains(from), "{from} not in {name}");
        text = text.replace(from, to);
    }
    text.into_bytes()
}

fn at() -> Timestamp {
    Timestamp::parse("2026-09-24T00:00:00Z").unwrap()
}

fn isin() -> ExternalIdentifier {
    ExternalIdentifier::Isin(Isin::parse("US67066G1040").unwrap())
}

fn exchange_figi() -> ExternalIdentifier {
    ExternalIdentifier::Figi(Figi::parse("BBG000BBK0R0").unwrap())
}

async fn setup() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    for id in [FIXTURE_SOURCE, SECOND_SOURCE] {
        insert_source(
            &mut conn,
            &Source {
                id: SourceId::parse(id).unwrap(),
                name: DisplayName::new("Reference fixture").unwrap(),
                redistribution: Redistribution::Unknown,
            },
        )
        .await
        .unwrap();
    }
    Some((db, conn))
}

async fn try_ingest(
    conn: &mut PgConnection,
    source: &str,
    key: &str,
    payload: Vec<u8>,
    received_at: &str,
) -> Result<IngestReport, IngestError> {
    ingest_reference(
        conn,
        &FixtureProvider::new(SourceId::parse(source).unwrap()),
        &FixtureNormalizer,
        &RawRecord {
            record_key: key.to_owned(),
            payload,
            received_at: Timestamp::parse(received_at).unwrap(),
        },
    )
    .await
}

async fn ingest(
    conn: &mut PgConnection,
    source: &str,
    key: &str,
    payload: Vec<u8>,
    received_at: &str,
) -> IngestReport {
    try_ingest(conn, source, key, payload, received_at)
        .await
        .unwrap()
}

/// Row counts of `source_records` and every fact table.
async fn counts(conn: &mut PgConnection) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for table in std::iter::once("source_records").chain(FACT_TABLES) {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        out.push((table.to_owned(), n));
    }
    out
}

fn nodes(report: &IngestReport) -> BTreeSet<CanonicalId> {
    [
        report.entity.id().into(),
        report.instrument.id().into(),
        report.currency.id().into(),
        report.venue.id().into(),
        report.listing.id().into(),
    ]
    .into_iter()
    .collect()
}

async fn facts(conn: &mut PgConnection, record: SourceRecordId) -> DerivedFacts {
    facts_from_source_record(conn, record).await.unwrap()
}

fn fact_count(facts: &DerivedFacts) -> usize {
    facts.nodes.len()
        + facts.identifiers.len()
        + facts.listing_symbols.len()
        + facts.relationships.len()
        + facts.conflicts.len()
}

#[tokio::test]
async fn every_ingested_fact_traces_to_its_exact_raw_record() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    let record = report.source_record.0;
    let expected = Provenance {
        source_id: SourceId::parse(FIXTURE_SOURCE).unwrap(),
        received_at: Timestamp::parse(RECEIVED_AT).unwrap(),
    };

    // The record is the raw payload, byte for byte, from the fixture source.
    let raw = get_source_record(&mut conn, record).await.unwrap().unwrap();
    assert_eq!(raw.payload, payload("nvda.json"));
    assert_eq!(raw.record_key, "nvda.json");
    assert_eq!(
        (raw.source_id.clone(), raw.received_at),
        (expected.source_id.clone(), expected.received_at)
    );

    // What the record asserted, found from the record.
    let derived = facts(&mut conn, record).await;
    assert_eq!(
        derived.nodes.iter().copied().collect::<BTreeSet<_>>(),
        nodes(&report)
    );
    assert_eq!(derived.identifiers.len(), 6, "LEI, ISIN, 2 FIGIs, MIC, USD");
    assert_eq!(derived.listing_symbols.len(), 1);
    assert_eq!(derived.relationships.len(), 2, "ISSUED_BY, DENOMINATED_IN");
    assert!(derived.conflicts.is_empty());

    // The record, found from each fact through the read APIs.
    for node in nodes(&report) {
        assert_eq!(
            reference::object_source_record(&mut conn, node)
                .await
                .unwrap(),
            Some(record),
            "{node}"
        );
        for stored in identifiers_for_node(&mut conn, node).await.unwrap() {
            assert_eq!(stored.source_record, record);
            assert_eq!(stored.assignment.provenance(), &expected);
        }
    }
    for symbol in symbols_for_listing(&mut conn, report.listing.id())
        .await
        .unwrap()
    {
        assert_eq!(symbol.source_record, record);
        assert_eq!(symbol.symbol.provenance, expected);
    }
    for edge in graph::relationships_from(&mut conn, report.instrument.id().into(), None)
        .await
        .unwrap()
    {
        assert_eq!(edge.source_record, record);
        assert_eq!(edge.relationship.provenance(), &expected);
    }

    // No fact in any table lacks the lineage fact → record → source.
    let mut traced = 0;
    for table in FACT_TABLES {
        let (total, via_record): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT count(*),
                    count(*) FILTER (WHERE r.id = $1 AND s.id = $2 AND r.payload = $3)
             FROM {table} f
             LEFT JOIN source_records r ON r.id = f.source_record_id
             LEFT JOIN sources s ON s.id = r.source_id"
        )))
        .bind(record.0)
        .bind(FIXTURE_SOURCE)
        .bind(payload("nvda.json"))
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(total, via_record, "{table}");
        traced += total;
    }
    assert_eq!(traced as usize, fact_count(&derived));
    drop(conn);
    db.teardown().await;
}

/// Two records from one source (here: two revisions under the same record
/// key) stay distinct, and each fact names the one that asserted it. The
/// revision names a different issuer: both `ISSUED_BY` assertions are kept,
/// each with its own record, and neither is chosen as canonical.
#[tokio::test]
async fn records_from_the_same_source_remain_distinguishable() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let first = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    let first_facts = facts(&mut conn, first.source_record.0).await;

    let revision = revised(
        "nvda.json",
        &[
            ("549300S4KLFTLO7GSQ80", "SYNTHETIC00000000174"),
            ("NVIDIA CORPORATION", "Synthetic Conflict Test Company"),
        ],
    );
    let second = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        revision.clone(),
        "2026-09-25T08:00:00Z",
    )
    .await;
    let (a, b) = (first.source_record.0, second.source_record.0);
    assert_ne!(a, b);
    assert_eq!(second.source_record.1, Write::Inserted);
    assert_eq!(
        get_source_record(&mut conn, b)
            .await
            .unwrap()
            .unwrap()
            .payload,
        revision
    );
    assert!(matches!(second.entity, Resolution::Created(_)));
    assert_eq!(
        second.instrument,
        Resolution::Existing(first.instrument.id())
    );

    // The revision is credited only with what it newly asserted.
    let second_facts = facts(&mut conn, b).await;
    assert_eq!(second_facts.nodes, vec![second.entity.id().into()]);
    assert_eq!(second_facts.identifiers.len(), 1, "the new LEI");
    assert!(second_facts.listing_symbols.is_empty());
    assert_eq!(second_facts.relationships.len(), 1, "the new ISSUED_BY");
    assert_eq!(facts(&mut conn, a).await, first_facts, "unchanged");

    // Known limitation: facts the revision merely repeated keep the first
    // record; the confirmation is not recorded as supporting evidence.
    assert_eq!(
        resolve_identifier(&mut conn, &isin(), at())
            .await
            .unwrap()
            .unwrap()
            .source_record,
        a
    );

    // Contradictory issuers coexist as assertions, each naming its record.
    let issued_by: Vec<(CanonicalId, SourceRecordId, String)> = graph::relationships_from(
        &mut conn,
        first.instrument.id().into(),
        Some(RelationshipType::IssuedBy),
    )
    .await
    .unwrap()
    .into_iter()
    .map(|e| {
        (
            e.relationship.object(),
            e.source_record,
            e.relationship.provenance().source_id.to_string(),
        )
    })
    .collect();
    assert_eq!(
        issued_by,
        vec![
            (first.entity.id().into(), a, FIXTURE_SOURCE.to_owned()),
            (second.entity.id().into(), b, FIXTURE_SOURCE.to_owned()),
        ]
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn rollback_removes_the_record_and_every_derived_fact() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let empty = counts(&mut conn).await;

    // Rolled back by the caller: ingestion composes into the caller's
    // transaction, so the record and its facts go together.
    let mut tx = conn.begin().await.unwrap();
    let report = ingest(
        &mut tx,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    let record = report.source_record.0;
    assert_eq!(fact_count(&facts(&mut tx, record).await), 14);
    tx.rollback().await.unwrap();
    assert_eq!(counts(&mut conn).await, empty);
    assert!(
        get_source_record(&mut conn, record)
            .await
            .unwrap()
            .is_none()
    );
    assert!(facts(&mut conn, record).await.is_empty());

    // Failed at the very last write (the second edge): nothing survives,
    // including the raw record.
    sqlx::raw_sql(
        "CREATE FUNCTION fail_edge() RETURNS trigger LANGUAGE plpgsql AS
           $$ BEGIN
                IF NEW.relationship_type = 'DENOMINATED_IN' THEN
                  RAISE EXCEPTION 'injected failure';
                END IF;
                RETURN NEW;
              END $$;
         CREATE TRIGGER fail_edge BEFORE INSERT ON graph_edges
           FOR EACH ROW EXECUTE FUNCTION fail_edge();",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let failed = try_ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    assert!(matches!(failed, Err(IngestError::Store(_))), "{failed:?}");
    assert_eq!(counts(&mut conn).await, empty);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn replay_is_idempotent_and_keeps_the_originating_record() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let first = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    let record = first.source_record.0;
    let before_counts = counts(&mut conn).await;
    let before_facts = facts(&mut conn, record).await;

    for received_at in ["2026-09-24T13:00:00Z", "2026-09-30T00:00:00Z"] {
        let replay = ingest(
            &mut conn,
            FIXTURE_SOURCE,
            "nvda.json",
            payload("nvda.json"),
            received_at,
        )
        .await;
        assert_eq!(replay.source_record, (record, Write::Unchanged));
        assert_eq!(nodes(&replay), nodes(&first));
        assert!(replay.quarantined().is_empty());
        assert_eq!(counts(&mut conn).await, before_counts, "no new rows");
        assert_eq!(facts(&mut conn, record).await, before_facts);
    }

    // The stored receipt time is the record's original one, not the replay's.
    let stored = resolve_identifier(&mut conn, &isin(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.source_record, record);
    assert_eq!(
        stored.assignment.provenance().received_at,
        Timestamp::parse(RECEIVED_AT).unwrap()
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn quarantined_claims_keep_the_record_that_produced_them() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let nvda = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        payload("nvda.json"),
        RECEIVED_AT,
    )
    .await;
    let synthetic = ingest(
        &mut conn,
        SECOND_SOURCE,
        "synthetic-conflict.json",
        payload("synthetic-conflict.json"),
        "2026-09-25T08:00:00Z",
    )
    .await;
    let (a, b) = (nvda.source_record.0, synthetic.source_record.0);
    let quarantined = synthetic.quarantined();
    assert_eq!(quarantined.len(), 2, "exchange FIGI and NVDA symbol");
    assert_eq!(facts(&mut conn, b).await.conflicts, quarantined);
    let b_record = get_source_record(&mut conn, b).await.unwrap().unwrap();
    let b_provenance = Provenance {
        source_id: b_record.source_id,
        received_at: b_record.received_at,
    };

    // Each quarantined claim names the record that made it; the mapping it
    // collided with names the record that established it.
    let nvda_symbol = VenueSymbol::new("NVDA").unwrap();
    let figi_conflicts = conflicts_for_identifier(&mut conn, &exchange_figi())
        .await
        .unwrap();
    let symbol_conflicts = conflicts_for_listing_symbol(&mut conn, nvda.venue.id(), &nvda_symbol)
        .await
        .unwrap();
    for conflict in figi_conflicts.iter().chain(&symbol_conflicts) {
        assert_eq!(conflict.source_record, b);
        let provenance = match &conflict.claim {
            ConflictClaim::Identifier { claim, .. } => claim.provenance(),
            ConflictClaim::ListingSymbol { claim, .. } => &claim.provenance,
        };
        assert_eq!(provenance, &b_provenance);
    }
    assert_eq!(
        resolve_identifier(&mut conn, &exchange_figi(), at())
            .await
            .unwrap()
            .unwrap()
            .source_record,
        a
    );
    assert_eq!(
        resolve_listing_symbol(&mut conn, nvda.venue.id(), &nvda_symbol, at())
            .await
            .unwrap()
            .unwrap()
            .source_record,
        a
    );

    // Replaying the same record adds no quarantine rows.
    let before = counts(&mut conn).await;
    let replay = ingest(
        &mut conn,
        SECOND_SOURCE,
        "synthetic-conflict.json",
        payload("synthetic-conflict.json"),
        "2026-09-26T08:00:00Z",
    )
    .await;
    assert_eq!(replay.quarantined(), quarantined);
    assert_eq!(counts(&mut conn).await, before);

    // A different record making the same claims is separate evidence.
    let revision = revised(
        "synthetic-conflict.json",
        &[("synthetic-conflict-v1", "synthetic-conflict-v2")],
    );
    let again = ingest(
        &mut conn,
        SECOND_SOURCE,
        "synthetic-conflict.json",
        revision,
        "2026-09-27T08:00:00Z",
    )
    .await;
    let c = again.source_record.0;
    assert_ne!(c, b);
    assert_eq!(again.quarantined().len(), 2);
    assert_eq!(facts(&mut conn, c).await.conflicts, again.quarantined());
    let records: Vec<SourceRecordId> =
        conflicts_for_listing_symbol(&mut conn, nvda.venue.id(), &nvda_symbol)
            .await
            .unwrap()
            .iter()
            .map(|c| c.source_record)
            .collect();
    assert_eq!(records, vec![b, c]);
    assert_eq!(
        facts(&mut conn, b).await.conflicts,
        quarantined,
        "unchanged"
    );
    drop(conn);
    db.teardown().await;
}
