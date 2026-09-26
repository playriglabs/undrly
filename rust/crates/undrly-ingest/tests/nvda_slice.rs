//! NVIDIA / NVDA vertical slice:
//! raw record → decode → normalize → resolve identities → persist → read back.
//!
//! Database tests: skipped without `DATABASE_URL`, required in CI.

use std::path::Path;

use sqlx::PgConnection;
use undrly_core::{
    CanonicalId, CurrencyCode, DisplayName, EntityId, ExternalIdentifier, Figi,
    IdentifierAssignment, Instrument, InstrumentClass, InstrumentId, Isin, Lei, Listing, ListingId,
    Mic, Provenance, Redistribution, RelationshipType, Source, SourceId, Timestamp, Validity,
    Venue, VenueId, VenueSymbol,
};
use undrly_ingest::{IngestError, IngestReport, RawRecord, Resolution, ingest_reference};
use undrly_normalize::fixture::FixtureNormalizer;
use undrly_provider::fixture::FixtureProvider;
use undrly_store::conflicts::{
    ConflictClaim, conflicts_for_identifier, conflicts_for_listing_symbol,
};
use undrly_store::identifiers::{
    AssignOutcome, assign_identifier, identifiers_for_node, resolve_identifier,
};
use undrly_store::listing_symbols::{
    SymbolAssignOutcome, assign_listing_symbol, listings_with_symbol, resolve_listing_symbol,
    symbols_for_listing,
};
use undrly_store::sources::{
    RecordProvenance, SourceRecord, get_source_record, insert_source, insert_source_record,
};
use undrly_store::testing::{TestDb, fresh};
use undrly_store::{Write, graph, reference};

const FIXTURE_SOURCE: &str = "reference-fixture";
const SECOND_SOURCE: &str = "second-reference-fixture";
const RECEIVED_AT: &str = "2026-09-24T12:00:00.123456Z";

fn payload(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/sources/reference-fixture")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn raw(name: &str, received_at: &str) -> RawRecord {
    RawRecord {
        record_key: name.to_owned(),
        payload: payload(name),
        received_at: Timestamp::parse(received_at).unwrap(),
    }
}

fn provider(source: &str) -> FixtureProvider {
    FixtureProvider::new(SourceId::parse(source).unwrap())
}

fn at() -> Timestamp {
    Timestamp::parse("2026-09-24T00:00:00Z").unwrap()
}

fn lei() -> ExternalIdentifier {
    ExternalIdentifier::Lei(Lei::parse("549300S4KLFTLO7GSQ80").unwrap())
}
fn isin() -> ExternalIdentifier {
    ExternalIdentifier::Isin(Isin::parse("US67066G1040").unwrap())
}
fn share_class_figi() -> ExternalIdentifier {
    ExternalIdentifier::Figi(Figi::parse("BBG001S5TZJ6").unwrap())
}
fn exchange_figi() -> ExternalIdentifier {
    ExternalIdentifier::Figi(Figi::parse("BBG000BBK0R0").unwrap())
}
fn mic() -> ExternalIdentifier {
    ExternalIdentifier::Mic(Mic::parse("XNAS").unwrap())
}
fn usd() -> ExternalIdentifier {
    ExternalIdentifier::Iso4217(CurrencyCode::parse("USD").unwrap())
}
fn nvda() -> VenueSymbol {
    VenueSymbol::new("NVDA").unwrap()
}

async fn register(conn: &mut PgConnection, id: &str) {
    insert_source(
        conn,
        &Source {
            id: SourceId::parse(id).unwrap(),
            name: DisplayName::new("Reference fixture").unwrap(),
            redistribution: Redistribution::Unknown,
        },
    )
    .await
    .unwrap();
}

/// Stores a raw record for facts a test writes through repositories directly.
async fn manual_record(
    conn: &mut PgConnection,
    source: &str,
    key: &str,
    received_at: &str,
) -> RecordProvenance {
    insert_source_record(
        conn,
        &SourceRecord {
            source_id: SourceId::parse(source).unwrap(),
            record_key: key.to_owned(),
            payload: key.as_bytes().to_vec(),
            received_at: Timestamp::parse(received_at).unwrap(),
        },
    )
    .await
    .unwrap()
    .0
}

/// A migrated database with both fixture sources registered.
async fn setup() -> Option<(TestDb, sqlx::pool::PoolConnection<sqlx::Postgres>)> {
    let db = fresh().await?;
    let mut conn = db.pool.acquire().await.unwrap();
    register(&mut conn, FIXTURE_SOURCE).await;
    register(&mut conn, SECOND_SOURCE).await;
    Some((db, conn))
}

async fn ingest(
    conn: &mut PgConnection,
    source: &str,
    name: &str,
    received_at: &str,
) -> IngestReport {
    ingest_reference(
        conn,
        &provider(source),
        &FixtureNormalizer,
        &raw(name, received_at),
    )
    .await
    .unwrap()
}

/// Row counts of every table ingestion writes, for before/after comparisons.
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

// 1 -------------------------------------------------------------------------------

#[tokio::test]
async fn same_external_identifier_resolves_to_same_canonical_node() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let first = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    assert!(matches!(first.entity, Resolution::Created(_)));
    assert!(matches!(first.instrument, Resolution::Created(_)));

    // Every identifier resolves to the node ingestion created.
    let instrument: CanonicalId = first.instrument.id().into();
    for (identifier, node) in [
        (lei(), first.entity.id().into()),
        (isin(), instrument),
        (share_class_figi(), instrument),
        (exchange_figi(), first.listing.id().into()),
        (mic(), first.venue.id().into()),
        (usd(), first.currency.id().into()),
    ] {
        let resolved = resolve_identifier(&mut conn, &identifier, at())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.assignment.node(), node, "{identifier:?}");
    }

    // A different source describing the same security resolves to the same
    // nodes; nothing new is minted.
    let second = ingest(
        &mut conn,
        SECOND_SOURCE,
        "nvda.json",
        "2026-09-25T08:00:00Z",
    )
    .await;
    assert_eq!(second.entity, Resolution::Existing(first.entity.id()));
    assert_eq!(
        second.instrument,
        Resolution::Existing(first.instrument.id())
    );
    assert_eq!(second.currency, Resolution::Existing(first.currency.id()));
    assert_eq!(second.venue, Resolution::Existing(first.venue.id()));
    assert_eq!(second.listing, Resolution::Existing(first.listing.id()));
    let nodes: i64 = sqlx::query_scalar("SELECT count(*) FROM nodes")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(nodes, 5, "entity, instrument, currency, venue, listing");
    drop(conn);
    db.teardown().await;
}

// 2 -------------------------------------------------------------------------------

#[tokio::test]
async fn replaying_the_same_source_record_is_idempotent() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let first = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    let before = counts(&mut conn).await;

    let replay = ingest(
        &mut conn,
        FIXTURE_SOURCE,
        "nvda.json",
        "2026-09-24T13:00:00Z",
    )
    .await;
    assert_eq!(counts(&mut conn).await, before, "no new rows");
    assert_eq!(
        replay.source_record,
        (first.source_record.0, Write::Unchanged)
    );
    assert_eq!(replay.entity, Resolution::Existing(first.entity.id()));
    assert_eq!(replay.listing, Resolution::Existing(first.listing.id()));
    for (identifier, outcome) in &replay.identifiers {
        assert!(
            matches!(outcome, AssignOutcome::Unchanged(_)),
            "{identifier:?}: {outcome:?}"
        );
    }
    assert!(matches!(
        replay.listing_symbol,
        SymbolAssignOutcome::Unchanged(_)
    ));
    for (kind, write) in &replay.relationships {
        assert_eq!(*write, Write::Unchanged, "{kind}");
    }
    assert!(replay.quarantined().is_empty());

    // The replay's later receipt time did not overwrite the original provenance.
    let stored = resolve_identifier(&mut conn, &isin(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.assignment.provenance().received_at,
        Timestamp::parse(RECEIVED_AT).unwrap()
    );
    drop(conn);
    db.teardown().await;
}

// 3 -------------------------------------------------------------------------------

#[tokio::test]
async fn nvda_is_scoped_to_nasdaq_not_global_identity() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    let nasdaq = report.venue.id();

    // On Nasdaq, NVDA resolves to the NVDA listing.
    let on_nasdaq = resolve_listing_symbol(&mut conn, nasdaq, &nvda(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(on_nasdaq.symbol.listing_id, report.listing.id());

    // A symbol is not an identifier or a canonical id.
    assert!(CanonicalId::parse("NVDA").is_err());
    let global: i64 = sqlx::query_scalar("SELECT count(*) FROM identifiers WHERE value = 'NVDA'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(global, 0);

    // Another venue may use NVDA for an unrelated listing without conflict.
    let record = manual_record(&mut conn, FIXTURE_SOURCE, "other-venue", RECEIVED_AT).await;
    let other_venue = VenueId::generate();
    reference::insert_venue(
        &mut conn,
        &Venue {
            id: other_venue,
            name: DisplayName::new("Other Venue").unwrap(),
        },
        record.id,
    )
    .await
    .unwrap();
    assert!(
        resolve_listing_symbol(&mut conn, other_venue, &nvda(), at())
            .await
            .unwrap()
            .is_none()
    );
    let unrelated = InstrumentId::generate();
    reference::insert_instrument(
        &mut conn,
        &Instrument {
            id: unrelated,
            class: InstrumentClass::Equity,
            name: DisplayName::new("Unrelated Test Instrument").unwrap(),
            contract_multiplier: None,
            unit_of_measure: None,
            fx_pair: None,
        },
        record.id,
    )
    .await
    .unwrap();
    let provenance = record.provenance.clone();
    let other_listing = ListingId::generate();
    reference::insert_listing(
        &mut conn,
        &Listing {
            id: other_listing,
            instrument_id: unrelated,
            venue_id: other_venue,
            provenance: provenance.clone(),
        },
        record.id,
    )
    .await
    .unwrap();
    let outcome = assign_listing_symbol(
        &mut conn,
        &undrly_core::ListingSymbol {
            listing_id: other_listing,
            venue_id: other_venue,
            symbol: nvda(),
            valid_during: Validity::UNBOUNDED,
            provenance,
        },
        record.id,
    )
    .await
    .unwrap();
    assert!(matches!(outcome, SymbolAssignOutcome::Assigned(_)));

    // A bare symbol is ambiguous across venues; callers must choose a venue.
    let everywhere = listings_with_symbol(&mut conn, &nvda(), at())
        .await
        .unwrap();
    let venues: Vec<VenueId> = everywhere.iter().map(|s| s.symbol.venue_id).collect();
    assert_eq!(venues, vec![nasdaq, other_venue]);
    drop(conn);
    db.teardown().await;
}

// 4 -------------------------------------------------------------------------------

#[tokio::test]
async fn conflicting_claims_are_quarantined_without_mutating_identity() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let nvda_report = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;

    // The synthetic record claims NVDA's Nasdaq symbol and exchange FIGI.
    let synthetic = ingest(
        &mut conn,
        SECOND_SOURCE,
        "synthetic-conflict.json",
        "2026-09-25T08:00:00Z",
    )
    .await;
    assert!(
        matches!(synthetic.instrument, Resolution::Created(_)),
        "resolved by its own ISIN"
    );
    assert_ne!(synthetic.listing.id(), nvda_report.listing.id());
    let SymbolAssignOutcome::Conflict {
        quarantined,
        existing,
    } = &synthetic.listing_symbol
    else {
        panic!(
            "expected symbol conflict, got {:?}",
            synthetic.listing_symbol
        );
    };
    assert_eq!(quarantined.len(), 1);
    assert_eq!(existing[0].symbol.listing_id, nvda_report.listing.id());
    let figi_outcome = &synthetic
        .identifiers
        .iter()
        .find(|(id, _)| *id == exchange_figi())
        .unwrap()
        .1;
    assert!(
        matches!(figi_outcome, AssignOutcome::Conflict { .. }),
        "{figi_outcome:?}"
    );
    assert_eq!(synthetic.quarantined().len(), 2);

    // Canonical identity is unchanged.
    let symbol = resolve_listing_symbol(&mut conn, nvda_report.venue.id(), &nvda(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(symbol.symbol.listing_id, nvda_report.listing.id());
    let figi = resolve_identifier(&mut conn, &exchange_figi(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(figi.assignment.node(), nvda_report.listing.id().into());
    assert!(
        symbols_for_listing(&mut conn, synthetic.listing.id())
            .await
            .unwrap()
            .is_empty()
    );

    // The quarantine keeps the full claim and what it collided with.
    let conflicts = conflicts_for_listing_symbol(&mut conn, nvda_report.venue.id(), &nvda())
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);
    let ConflictClaim::ListingSymbol { claim, existing } = &conflicts[0].claim else {
        panic!()
    };
    assert_eq!(claim.listing_id, synthetic.listing.id());
    assert_eq!(claim.provenance.source_id.as_str(), SECOND_SOURCE);
    assert_eq!(*existing, symbol.id);
    assert_eq!(
        conflicts_for_identifier(&mut conn, &exchange_figi())
            .await
            .unwrap()
            .len(),
        1
    );

    // Replaying the conflicting record does not duplicate quarantine rows.
    let before = counts(&mut conn).await;
    let replay = ingest(
        &mut conn,
        SECOND_SOURCE,
        "synthetic-conflict.json",
        "2026-09-26T08:00:00Z",
    )
    .await;
    assert_eq!(replay.quarantined(), synthetic.quarantined());
    assert_eq!(counts(&mut conn).await, before);
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn repository_quarantines_a_conflicting_isin_claim() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    let record = manual_record(&mut conn, SECOND_SOURCE, "other", "2026-09-25T08:00:00Z").await;
    let other = InstrumentId::generate();
    reference::insert_instrument(
        &mut conn,
        &Instrument {
            id: other,
            class: InstrumentClass::Equity,
            name: DisplayName::new("Other Test Instrument").unwrap(),
            contract_multiplier: None,
            unit_of_measure: None,
            fx_pair: None,
        },
        record.id,
    )
    .await
    .unwrap();
    let claim = IdentifierAssignment::new(
        isin(),
        other.into(),
        Validity::UNBOUNDED,
        record.provenance.clone(),
    )
    .unwrap();
    let outcome = assign_identifier(&mut conn, &claim, record.id)
        .await
        .unwrap();
    let AssignOutcome::Conflict {
        quarantined,
        existing,
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert_eq!(quarantined.len(), 1);
    assert_eq!(existing[0].assignment.node(), report.instrument.id().into());
    let resolved = resolve_identifier(&mut conn, &isin(), at())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.assignment.node(), report.instrument.id().into());
    assert!(
        identifiers_for_node(&mut conn, other.into())
            .await
            .unwrap()
            .is_empty()
    );
    let stored = conflicts_for_identifier(&mut conn, &isin()).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].source_record, record.id);
    assert_eq!(
        stored[0].claim,
        ConflictClaim::Identifier {
            claim,
            existing: existing[0].id
        }
    );
    drop(conn);
    db.teardown().await;
}

// 5 -------------------------------------------------------------------------------

#[tokio::test]
async fn graph_is_reconstructed_from_storage() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;

    // Start from nothing but NVIDIA's LEI.
    let entity_id = EntityId::try_from(
        resolve_identifier(&mut conn, &lei(), at())
            .await
            .unwrap()
            .unwrap()
            .assignment
            .node(),
    )
    .unwrap();
    let entity = reference::get_entity(&mut conn, entity_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entity.name.as_str(), "NVIDIA CORPORATION");

    // entity ← ISSUED_BY ← instrument (inverse traversal at query time)
    let issued = graph::relationships_to(
        &mut conn,
        entity_id.into(),
        Some(RelationshipType::IssuedBy),
    )
    .await
    .unwrap();
    assert_eq!(issued.len(), 1);
    let instrument_id = InstrumentId::try_from(issued[0].relationship.subject()).unwrap();
    let instrument = reference::get_instrument(&mut conn, instrument_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (instrument.name.as_str(), instrument.class),
        ("NVIDIA CORP", InstrumentClass::Equity)
    );
    let instrument_ids: Vec<ExternalIdentifier> =
        identifiers_for_node(&mut conn, instrument_id.into())
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.assignment.identifier().clone())
            .collect();
    assert_eq!(instrument_ids, vec![isin(), share_class_figi()]);

    // instrument → DENOMINATED_IN → currency
    let denominated = graph::relationships_from(
        &mut conn,
        instrument_id.into(),
        Some(RelationshipType::DenominatedIn),
    )
    .await
    .unwrap();
    assert_eq!(denominated.len(), 1);
    let currency_id = denominated[0].relationship.object().try_into().unwrap();
    let currency = reference::get_currency(&mut conn, currency_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(currency.name.as_str(), "US Dollar");
    let codes = identifiers_for_node(&mut conn, currency_id.into())
        .await
        .unwrap();
    assert_eq!(codes[0].assignment.identifier(), &usd());

    // instrument → listing → venue, with its venue-scoped symbol
    let listings = reference::listings_for_instrument(&mut conn, instrument_id)
        .await
        .unwrap();
    assert_eq!(listings.len(), 1);
    let venue = reference::get_venue(&mut conn, listings[0].venue_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(venue.name.as_str(), "Nasdaq");
    assert_eq!(
        identifiers_for_node(&mut conn, venue.id.into())
            .await
            .unwrap()[0]
            .assignment
            .identifier(),
        &mic()
    );
    let symbols = symbols_for_listing(&mut conn, listings[0].id)
        .await
        .unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(
        (
            symbols[0].symbol.venue_id,
            symbols[0].symbol.symbol.as_str()
        ),
        (venue.id, "NVDA")
    );
    let listing_ids = identifiers_for_node(&mut conn, listings[0].id.into())
        .await
        .unwrap();
    assert_eq!(listing_ids[0].assignment.identifier(), &exchange_figi());

    // Exactly the asserted edges exist: nothing inferred or duplicated.
    let all_from_instrument = graph::relationships_from(&mut conn, instrument_id.into(), None)
        .await
        .unwrap();
    let kinds: Vec<RelationshipType> = all_from_instrument
        .iter()
        .map(|r| r.relationship.relationship_type())
        .collect();
    assert_eq!(
        kinds,
        vec![RelationshipType::IssuedBy, RelationshipType::DenominatedIn]
    );
    drop(conn);
    db.teardown().await;
}

// 6 -------------------------------------------------------------------------------

#[tokio::test]
async fn provenance_survives_the_round_trip() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let report = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    let expected = Provenance {
        source_id: SourceId::parse(FIXTURE_SOURCE).unwrap(),
        received_at: Timestamp::parse(RECEIVED_AT).unwrap(),
    };

    // The raw record is stored byte-for-byte with its receipt time.
    let record = get_source_record(&mut conn, report.source_record.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.payload, payload("nvda.json"));
    assert_eq!(record.received_at, expected.received_at);
    assert_eq!(record.source_id, expected.source_id);

    // Every fact carries the source and exact (microsecond) receipt time.
    let nodes: [CanonicalId; 5] = [
        report.entity.id().into(),
        report.instrument.id().into(),
        report.currency.id().into(),
        report.venue.id().into(),
        report.listing.id().into(),
    ];
    for node in nodes {
        for stored in identifiers_for_node(&mut conn, node).await.unwrap() {
            assert_eq!(stored.assignment.provenance(), &expected, "{node}");
        }
    }
    for edge in graph::relationships_from(&mut conn, report.instrument.id().into(), None)
        .await
        .unwrap()
    {
        assert_eq!(edge.relationship.provenance(), &expected);
    }
    let listing = reference::get_listing(&mut conn, report.listing.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(listing.provenance, expected);
    for symbol in symbols_for_listing(&mut conn, report.listing.id())
        .await
        .unwrap()
    {
        assert_eq!(symbol.symbol.provenance, expected);
    }

    // A second source's assertion of the same edge is kept as its own row.
    ingest(
        &mut conn,
        SECOND_SOURCE,
        "nvda.json",
        "2026-09-25T08:00:00Z",
    )
    .await;
    let sources: Vec<String> = graph::relationships_from(
        &mut conn,
        report.instrument.id().into(),
        Some(RelationshipType::IssuedBy),
    )
    .await
    .unwrap()
    .iter()
    .map(|r| r.relationship.provenance().source_id.to_string())
    .collect();
    assert_eq!(sources, vec![FIXTURE_SOURCE, SECOND_SOURCE]);
    drop(conn);
    db.teardown().await;
}

// 7 -------------------------------------------------------------------------------

#[tokio::test]
async fn failed_ingestion_leaves_no_partial_canonical_state() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let empty = counts(&mut conn).await;

    // Make the last canonical write of the pipeline fail, after the entity,
    // instrument, currency, venue, listing, and identifiers were written.
    sqlx::raw_sql(
        "CREATE FUNCTION fail_symbol() RETURNS trigger LANGUAGE plpgsql AS
           $$ BEGIN RAISE EXCEPTION 'injected failure'; END $$;
         CREATE TRIGGER fail_symbol BEFORE INSERT ON listing_symbols
           FOR EACH ROW EXECUTE FUNCTION fail_symbol();",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let result = ingest_reference(
        &mut conn,
        &provider(FIXTURE_SOURCE),
        &FixtureNormalizer,
        &raw("nvda.json", RECEIVED_AT),
    )
    .await;
    assert!(matches!(result, Err(IngestError::Store(_))), "{result:?}");
    assert_eq!(
        counts(&mut conn).await,
        empty,
        "every write rolled back, including the raw record"
    );

    // With the failure removed, the same record ingests cleanly.
    sqlx::raw_sql("DROP TRIGGER fail_symbol ON listing_symbols")
        .execute(&mut *conn)
        .await
        .unwrap();
    let report = ingest(&mut conn, FIXTURE_SOURCE, "nvda.json", RECEIVED_AT).await;
    assert!(matches!(report.entity, Resolution::Created(_)));
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn repository_writes_are_atomic() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    sqlx::raw_sql(
        "CREATE FUNCTION fail_entity() RETURNS trigger LANGUAGE plpgsql AS
           $$ BEGIN RAISE EXCEPTION 'injected failure'; END $$;
         CREATE TRIGGER fail_entity BEFORE INSERT ON entities
           FOR EACH ROW EXECUTE FUNCTION fail_entity();",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let record = manual_record(&mut conn, FIXTURE_SOURCE, "entity", RECEIVED_AT).await;
    let entity = undrly_core::Entity {
        id: EntityId::generate(),
        kind: undrly_core::EntityKind::Company,
        name: DisplayName::new("Test Entity").unwrap(),
    };
    assert!(
        reference::insert_entity(&mut conn, &entity, record.id)
            .await
            .is_err()
    );
    // The node row written before the failing entity row was rolled back.
    assert!(
        reference::get_node(&mut conn, entity.id.uuid())
            .await
            .unwrap()
            .is_none()
    );
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn invalid_records_write_nothing() {
    let Some((db, mut conn)) = setup().await else {
        return;
    };
    let empty = counts(&mut conn).await;
    let mut bad = raw("nvda.json", RECEIVED_AT);
    bad.payload = String::from_utf8(bad.payload)
        .unwrap()
        .replace("US67066G1040", "US67066G1041")
        .into_bytes();
    let result = ingest_reference(
        &mut conn,
        &provider(FIXTURE_SOURCE),
        &FixtureNormalizer,
        &bad,
    )
    .await;
    assert!(
        matches!(result, Err(IngestError::Normalize(_))),
        "{result:?}"
    );
    let unknown = ingest_reference(
        &mut conn,
        &provider("unregistered-source"),
        &FixtureNormalizer,
        &raw("nvda.json", RECEIVED_AT),
    )
    .await;
    assert!(
        matches!(unknown, Err(IngestError::UnknownSource(_))),
        "{unknown:?}"
    );
    assert_eq!(counts(&mut conn).await, empty);
    drop(conn);
    db.teardown().await;
}
