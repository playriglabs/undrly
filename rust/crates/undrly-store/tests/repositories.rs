//! Repository outcomes not covered by the NVDA slice in `undrly-ingest`.

mod common;

use common::{SOURCE, fresh};
use undrly_core::{
    CurrencyCode, DisplayName, Entity, EntityId, EntityKind, ExternalIdentifier,
    IdentifierAssignment, Instrument, InstrumentClass, InstrumentId, Listing, ListingId,
    ListingSymbol, Provenance, Redistribution, Source, SourceId, Timestamp, Validity, Venue,
    VenueId, VenueSymbol,
};
use undrly_store::identifiers::{AssignOutcome, assign_identifier};
use undrly_store::listing_symbols::{SymbolAssignOutcome, assign_listing_symbol};
use undrly_store::sources::{SourceRecord, get_source, insert_source, insert_source_record};
use undrly_store::{StoreError, Write, reference};

fn provenance() -> Provenance {
    Provenance {
        source_id: SourceId::parse(SOURCE).unwrap(),
        received_at: Timestamp::parse("2026-09-24T12:00:00Z").unwrap(),
    }
}

fn name(s: &str) -> DisplayName {
    DisplayName::new(s).unwrap()
}

fn t(s: &str) -> Option<Timestamp> {
    Some(Timestamp::parse(s).unwrap())
}

#[tokio::test]
async fn inserts_are_idempotent_and_never_overwrite() {
    let Some(db) = fresh().await else { return };
    let mut conn = db.pool.acquire().await.unwrap();

    let source = Source {
        id: SourceId::parse("repo-source").unwrap(),
        name: name("Repo Source"),
        redistribution: Redistribution::Restricted,
    };
    assert_eq!(
        insert_source(&mut conn, &source).await.unwrap(),
        Write::Inserted
    );
    assert_eq!(
        insert_source(&mut conn, &source).await.unwrap(),
        Write::Unchanged
    );
    let changed = Source {
        redistribution: Redistribution::Permitted,
        ..source.clone()
    };
    assert!(matches!(
        insert_source(&mut conn, &changed).await,
        Err(StoreError::ExistingRecordDiffers { what: "source", .. })
    ));
    assert_eq!(
        get_source(&mut conn, &source.id).await.unwrap(),
        Some(source)
    );

    let entity = Entity {
        id: EntityId::generate(),
        kind: EntityKind::Company,
        name: name("Test Co"),
    };
    assert_eq!(
        reference::insert_entity(&mut conn, &entity).await.unwrap(),
        Write::Inserted
    );
    assert_eq!(
        reference::insert_entity(&mut conn, &entity).await.unwrap(),
        Write::Unchanged
    );
    let renamed = Entity {
        name: name("Renamed Co"),
        ..entity.clone()
    };
    assert!(matches!(
        reference::insert_entity(&mut conn, &renamed).await,
        Err(StoreError::ExistingRecordDiffers { what: "entity", .. })
    ));
    assert_eq!(
        reference::get_entity(&mut conn, entity.id).await.unwrap(),
        Some(entity.clone())
    );
    assert_eq!(
        reference::get_node(&mut conn, entity.id.uuid())
            .await
            .unwrap(),
        Some(entity.id.canonical())
    );

    let record = SourceRecord {
        source_id: SourceId::parse(SOURCE).unwrap(),
        record_key: "k".into(),
        payload: b"payload".to_vec(),
        received_at: Timestamp::parse("2026-09-24T12:00:00Z").unwrap(),
    };
    let (id, write) = insert_source_record(&mut conn, &record).await.unwrap();
    assert_eq!(write, Write::Inserted);
    let later = SourceRecord {
        received_at: Timestamp::parse("2026-09-25T12:00:00Z").unwrap(),
        ..record.clone()
    };
    assert_eq!(
        insert_source_record(&mut conn, &later).await.unwrap(),
        (id, Write::Unchanged)
    );
    let changed_payload = SourceRecord {
        payload: b"payload v2".to_vec(),
        ..record
    };
    assert_eq!(
        insert_source_record(&mut conn, &changed_payload)
            .await
            .unwrap()
            .1,
        Write::Inserted
    );

    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn identifier_period_overlap_on_same_node_writes_nothing() {
    let Some(db) = fresh().await else { return };
    let mut conn = db.pool.acquire().await.unwrap();
    let currency = undrly_core::Currency {
        id: undrly_core::CurrencyId::generate(),
        name: name("Test Currency"),
    };
    reference::insert_currency(&mut conn, &currency)
        .await
        .unwrap();
    let code = ExternalIdentifier::Iso4217(CurrencyCode::parse("TST").unwrap());
    let claim = |validity| {
        IdentifierAssignment::new(code.clone(), currency.id.into(), validity, provenance()).unwrap()
    };

    let first = assign_identifier(
        &mut conn,
        &claim(Validity::new(t("2020-01-01T00:00:00Z"), None).unwrap()),
    )
    .await
    .unwrap();
    assert!(matches!(first, AssignOutcome::Assigned(_)));
    let overlapping = assign_identifier(&mut conn, &claim(Validity::UNBOUNDED))
        .await
        .unwrap();
    assert!(
        matches!(overlapping, AssignOutcome::OverlapsExistingPeriod { .. }),
        "{overlapping:?}"
    );
    // Adjacent, non-overlapping period on the same node is a separate row.
    let earlier = assign_identifier(
        &mut conn,
        &claim(Validity::new(None, t("2020-01-01T00:00:00Z")).unwrap()),
    )
    .await
    .unwrap();
    assert!(matches!(earlier, AssignOutcome::Assigned(_)));
    let conflicts: i64 = sqlx::query_scalar("SELECT count(*) FROM identifier_conflicts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(conflicts, 0, "same-node overlaps are not conflicts");
    drop(conn);
    db.teardown().await;
}

#[tokio::test]
async fn listing_symbol_rules() {
    let Some(db) = fresh().await else { return };
    let mut conn = db.pool.acquire().await.unwrap();
    let venue = Venue {
        id: VenueId::generate(),
        name: name("Test Venue"),
    };
    let other_venue = Venue {
        id: VenueId::generate(),
        name: name("Other Venue"),
    };
    reference::insert_venue(&mut conn, &venue).await.unwrap();
    reference::insert_venue(&mut conn, &other_venue)
        .await
        .unwrap();
    let instrument = Instrument {
        id: InstrumentId::generate(),
        class: InstrumentClass::Equity,
        name: name("Test Instrument"),
    };
    reference::insert_instrument(&mut conn, &instrument)
        .await
        .unwrap();
    let listing = Listing {
        id: ListingId::generate(),
        instrument_id: instrument.id,
        venue_id: venue.id,
        provenance: provenance(),
    };
    assert_eq!(
        reference::insert_listing(&mut conn, &listing)
            .await
            .unwrap(),
        Write::Inserted
    );
    assert_eq!(
        reference::insert_listing(&mut conn, &listing)
            .await
            .unwrap(),
        Write::Unchanged
    );
    let moved = Listing {
        venue_id: other_venue.id,
        ..listing.clone()
    };
    assert!(matches!(
        reference::insert_listing(&mut conn, &moved).await,
        Err(StoreError::ExistingRecordDiffers {
            what: "listing",
            ..
        })
    ));

    let symbol = |venue_id, s: &str| ListingSymbol {
        listing_id: listing.id,
        venue_id,
        symbol: VenueSymbol::new(s).unwrap(),
        valid_during: Validity::UNBOUNDED,
        provenance: provenance(),
    };
    assert!(matches!(
        assign_listing_symbol(&mut conn, &symbol(other_venue.id, "TST")).await,
        Err(StoreError::ListingVenueMismatch { .. })
    ));
    assert!(matches!(
        assign_listing_symbol(&mut conn, &symbol(venue.id, "TST"))
            .await
            .unwrap(),
        SymbolAssignOutcome::Assigned(_)
    ));
    assert!(matches!(
        assign_listing_symbol(&mut conn, &symbol(venue.id, "TST"))
            .await
            .unwrap(),
        SymbolAssignOutcome::Unchanged(_)
    ));
    // A different symbol for the same listing and period is reported, not written.
    let other = assign_listing_symbol(&mut conn, &symbol(venue.id, "TST2"))
        .await
        .unwrap();
    assert!(
        matches!(other, SymbolAssignOutcome::ListingHasOtherSymbol { .. }),
        "{other:?}"
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM listing_symbols")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    drop(conn);
    db.teardown().await;
}
