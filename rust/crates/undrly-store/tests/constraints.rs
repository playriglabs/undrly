//! Database-level invariants. Each test inserts rows that violate one rule and
//! asserts the intended constraint rejected them.

mod common;

use std::str::FromStr;

use common::{
    CHECK_VIOLATION, EXCLUSION_VIOLATION, FOREIGN_KEY_VIOLATION, NOT_NULL_VIOLATION, SOURCE,
    TestDb, UNIQUE_VIOLATION, assert_rejected, fresh, ts,
};
use rust_decimal::Decimal;
use sqlx::postgres::types::PgRange;
use undrly_core::{Category, InstrumentId, Timestamp, Validity};
use undrly_store::mapping::{
    decimal_from_sql, decimal_to_sql, validity_from_range, validity_to_range,
};
use uuid::Uuid;

// --- nodes -------------------------------------------------------------------

#[tokio::test]
async fn nodes_accept_only_uuid_v7() {
    let Some(db) = fresh().await else { return };
    let generated = InstrumentId::generate().uuid();
    sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, 'instrument')")
        .bind(generated)
        .execute(&db.pool)
        .await
        .unwrap();
    let v7_bits_wrong_variant = Uuid::parse_str("0192a1b2-c3d4-7e5f-ca6b-7c8d9e0f1a2b").unwrap();
    for not_v7 in [
        Uuid::new_v4(),
        Uuid::nil(),
        Uuid::max(),
        v7_bits_wrong_variant,
    ] {
        assert_rejected(
            sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, 'instrument')")
                .bind(not_v7)
                .execute(&db.pool)
                .await,
            CHECK_VIOLATION,
            Some("nodes_id_check"),
        );
    }
    db.teardown().await;
}

#[tokio::test]
async fn category_rows_must_match_node_category() {
    let Some(db) = fresh().await else { return };
    let venue = db.node(Category::Venue).await;
    // A venue node cannot get an entity row.
    assert_rejected(
        sqlx::query("INSERT INTO entities (id, entity_kind, name) VALUES ($1, 'company', 'X')")
            .bind(venue)
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        Some("entities_id_category_fkey"),
    );
    // A category row cannot claim a different category.
    assert_rejected(
        sqlx::query("INSERT INTO venues (id, category, name) VALUES ($1, 'entity', 'X')")
            .bind(Uuid::now_v7())
            .execute(&db.pool)
            .await,
        CHECK_VIOLATION,
        Some("venues_category_check"),
    );
    // A node's category cannot change once referenced.
    assert_rejected(
        sqlx::query("UPDATE nodes SET category = 'currency' WHERE id = $1")
            .bind(venue)
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        Some("venues_id_category_fkey"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn referenced_nodes_cannot_be_deleted() {
    let Some(db) = fresh().await else { return };
    let entity = db.node(Category::Entity).await;
    assert_rejected(
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(entity)
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        None,
    );
    db.teardown().await;
}

#[tokio::test]
async fn display_names_are_validated() {
    let Some(db) = fresh().await else { return };
    for bad in ["", " NVIDIA", "NVIDIA ", "NV\u{7}DIA", &"a".repeat(257)] {
        assert_rejected(
            sqlx::query("INSERT INTO sources (id, name) VALUES ('other-source', $1)")
                .bind(bad)
                .execute(&db.pool)
                .await,
            CHECK_VIOLATION,
            Some("display_name_check"),
        );
    }
    db.teardown().await;
}

// --- sources -----------------------------------------------------------------

#[tokio::test]
async fn sources_default_to_unknown_redistribution() {
    let Some(db) = fresh().await else { return };
    let redistribution: String =
        sqlx::query_scalar("SELECT redistribution FROM sources WHERE id = $1")
            .bind(SOURCE)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(redistribution, "unknown");
    for bad_id in ["Example", "a--b", "a_b", "-a"] {
        assert_rejected(
            sqlx::query("INSERT INTO sources (id, name) VALUES ($1, 'X')")
                .bind(bad_id)
                .execute(&db.pool)
                .await,
            CHECK_VIOLATION,
            Some("source_id_check"),
        );
    }
    assert_rejected(
        sqlx::query("INSERT INTO sources (id, name, redistribution) VALUES ('x', 'X', 'public')")
            .execute(&db.pool)
            .await,
        CHECK_VIOLATION,
        Some("sources_redistribution_check"),
    );
    db.teardown().await;
}

// --- identifiers -------------------------------------------------------------

async fn assign(
    db: &TestDb,
    scheme: &str,
    value: &str,
    node: Uuid,
    category: Category,
    valid_during: Validity,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO identifiers (scheme, value, node_id, node_category, valid_during, source_id, received_at)
         VALUES ($1, $2, $3, $4, $5, $6, now()) RETURNING id",
    )
    .bind(scheme)
    .bind(value)
    .bind(node)
    .bind(category.as_str())
    .bind(validity_to_range(valid_during))
    .bind(SOURCE)
    .fetch_one(&db.pool)
    .await
}

fn period(from: Option<&str>, until: Option<&str>) -> Validity {
    Validity::new(
        from.map(|s| Timestamp::parse(s).unwrap()),
        until.map(|s| Timestamp::parse(s).unwrap()),
    )
    .unwrap()
}

#[tokio::test]
async fn identifier_schemes_restrict_categories() {
    let Some(db) = fresh().await else { return };
    let usd = db.node(Category::Currency).await;
    let instrument = db.node(Category::Instrument).await;
    assign(
        &db,
        "iso4217",
        "USD",
        usd,
        Category::Currency,
        Validity::UNBOUNDED,
    )
    .await
    .unwrap();
    assert_rejected(
        assign(
            &db,
            "iso4217",
            "EUR",
            instrument,
            Category::Instrument,
            Validity::UNBOUNDED,
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifiers_scheme_node_category_fkey"),
    );
    // Declaring a false category is caught by the node FK.
    assert_rejected(
        assign(
            &db,
            "isin",
            "US0378331005",
            usd,
            Category::Instrument,
            Validity::UNBOUNDED,
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifiers_node_id_node_category_fkey"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn identifier_values_are_shape_checked_per_namespace() {
    let Some(db) = fresh().await else { return };
    let instrument = db.node(Category::Instrument).await;
    let venue = db.node(Category::Venue).await;
    for (scheme, value, node, category) in [
        ("isin", "us0378331005", instrument, Category::Instrument),
        ("isin", "US037833100", instrument, Category::Instrument),
        ("figi", "BBG000BLANH6", instrument, Category::Instrument),
        ("mic", "xnas", venue, Category::Venue),
        ("mic", "XNASD", venue, Category::Venue),
    ] {
        assert_rejected(
            assign(&db, scheme, value, node, category, Validity::UNBOUNDED).await,
            CHECK_VIOLATION,
            Some("identifiers_value_shape"),
        );
    }
    db.teardown().await;
}

#[tokio::test]
async fn an_identifier_maps_to_one_node_at_a_time() {
    let Some(db) = fresh().await else { return };
    let old = db.node(Category::Instrument).await;
    let new = db.node(Category::Instrument).await;
    let isin = "US0378331005";
    assign(
        &db,
        "isin",
        isin,
        old,
        Category::Instrument,
        period(None, Some("2020-01-01T00:00:00Z")),
    )
    .await
    .unwrap();
    // Overlapping claim for a different node is rejected.
    assert_rejected(
        assign(
            &db,
            "isin",
            isin,
            new,
            Category::Instrument,
            period(Some("2019-06-01T00:00:00Z"), None),
        )
        .await,
        EXCLUSION_VIOLATION,
        Some("identifiers_one_node_at_a_time"),
    );
    // The same identifier may be reassigned after the earlier period ends.
    assign(
        &db,
        "isin",
        isin,
        new,
        Category::Instrument,
        period(Some("2020-01-01T00:00:00Z"), None),
    )
    .await
    .unwrap();
    // History is preserved: both periods are queryable.
    let owner_at = |t: &'static str| {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT node_id FROM identifiers WHERE scheme = 'isin' AND value = $1 AND valid_during @> $2",
        )
        .bind(isin)
        .bind(ts(t))
    };
    assert_eq!(
        owner_at("2015-01-01T00:00:00Z")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        old
    );
    assert_eq!(
        owner_at("2024-01-01T00:00:00Z")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        new
    );
    db.teardown().await;
}

#[tokio::test]
async fn validity_periods_must_be_half_open_and_non_empty() {
    let Some(db) = fresh().await else { return };
    let usd = db.node(Category::Currency).await;
    for bad in [
        "empty",
        "[2020-01-01,2020-01-01)",
        "[2020-01-01,2021-01-01]",
        "(2020-01-01,2021-01-01)",
    ] {
        assert_rejected(
            sqlx::query(
                "INSERT INTO identifiers (scheme, value, node_id, node_category, valid_during, source_id, received_at)
                 VALUES ('iso4217', 'USD', $1, 'currency', $2::tstzrange, $3, now())",
            )
            .bind(usd)
            .bind(bad)
            .bind(SOURCE)
            .execute(&db.pool)
            .await,
            CHECK_VIOLATION,
            Some("validity_check"),
        );
    }
    db.teardown().await;
}

// --- listing symbols ---------------------------------------------------------

async fn symbol(
    db: &TestDb,
    listing: Uuid,
    venue: Uuid,
    symbol: &str,
    valid_during: Validity,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO listing_symbols (listing_id, venue_id, symbol, valid_during, source_id, received_at)
         VALUES ($1, $2, $3, $4, $5, now()) RETURNING id",
    )
    .bind(listing)
    .bind(venue)
    .bind(symbol)
    .bind(validity_to_range(valid_during))
    .bind(SOURCE)
    .fetch_one(&db.pool)
    .await
}

#[tokio::test]
async fn listing_symbols_are_venue_scoped_with_history() {
    let Some(db) = fresh().await else { return };
    let nasdaq = db.node(Category::Venue).await;
    let other_venue = db.node(Category::Venue).await;
    let meta = db.node(Category::Instrument).await;
    let later_company = db.node(Category::Instrument).await;
    let meta_listing = db.listing(meta, nasdaq).await;
    let later_listing = db.listing(later_company, nasdaq).await;
    let elsewhere = db.listing(meta, other_venue).await;

    let until_2022 = period(None, Some("2022-06-09T00:00:00Z"));
    let from_2022 = period(Some("2022-06-09T00:00:00Z"), None);
    // Ticker change on the same listing: FB until 2022, then META.
    symbol(&db, meta_listing, nasdaq, "FB", until_2022)
        .await
        .unwrap();
    symbol(&db, meta_listing, nasdaq, "META", from_2022)
        .await
        .unwrap();
    // A listing has one symbol at a time.
    assert_rejected(
        symbol(&db, meta_listing, nasdaq, "MTA", from_2022).await,
        EXCLUSION_VIOLATION,
        Some("listing_symbols_one_symbol_at_a_time"),
    );
    // A symbol maps to one listing per venue at a time...
    assert_rejected(
        symbol(
            &db,
            later_listing,
            nasdaq,
            "META",
            period(Some("2023-01-01T00:00:00Z"), None),
        )
        .await,
        EXCLUSION_VIOLATION,
        Some("listing_symbols_one_listing_at_a_time"),
    );
    // ...but can be reused after its period ends.
    symbol(&db, later_listing, nasdaq, "FB", from_2022)
        .await
        .unwrap();
    // The same symbol on another venue is unrelated.
    symbol(&db, elsewhere, other_venue, "META", Validity::UNBOUNDED)
        .await
        .unwrap();
    // A symbol's venue must be its listing's venue.
    let fresh_listing = db.listing(later_company, other_venue).await;
    assert_rejected(
        symbol(&db, fresh_listing, nasdaq, "ZZZ", Validity::UNBOUNDED).await,
        FOREIGN_KEY_VIOLATION,
        Some("listing_symbols_listing_id_venue_id_fkey"),
    );
    // Case is preserved, not folded.
    symbol(&db, fresh_listing, other_venue, "meta", Validity::UNBOUNDED)
        .await
        .unwrap();
    for bad in ["", "NV DA", "NVDA\n"] {
        assert_rejected(
            symbol(
                &db,
                fresh_listing,
                other_venue,
                bad,
                period(Some("2030-01-01T00:00:00Z"), None),
            )
            .await,
            CHECK_VIOLATION,
            Some("listing_symbols_symbol_check"),
        );
    }
    db.teardown().await;
}

// --- identifier conflicts ----------------------------------------------------

#[tokio::test]
async fn conflicting_identifier_claims_are_quarantined_with_context() {
    let Some(db) = fresh().await else { return };
    let existing_node = db.node(Category::Instrument).await;
    let claimed_node = db.node(Category::Instrument).await;
    let existing = assign(
        &db,
        "isin",
        "US0378331005",
        existing_node,
        Category::Instrument,
        Validity::UNBOUNDED,
    )
    .await
    .unwrap();

    // The conflicting claim is rejected...
    assert_rejected(
        assign(
            &db,
            "isin",
            "US0378331005",
            claimed_node,
            Category::Instrument,
            Validity::UNBOUNDED,
        )
        .await,
        EXCLUSION_VIOLATION,
        Some("identifiers_one_node_at_a_time"),
    );
    // ...and quarantined with the claim and the mapping it collided with.
    let insert_conflict = |value: &'static str, existing: i64| {
        sqlx::query(
            "INSERT INTO identifier_conflicts
               (namespace, value, claimed_node_id, claimed_node_category, claimed_valid_during,
                source_id, received_at, conflicting_identifier_id)
             VALUES ('isin', $1, $2, 'instrument', '(,)', $3, now(), $4)",
        )
        .bind(value)
        .bind(claimed_node)
        .bind(SOURCE)
        .bind(existing)
    };
    insert_conflict("US0378331005", existing)
        .execute(&db.pool)
        .await
        .unwrap();
    // The referenced mapping must be for the same identifier.
    assert_rejected(
        insert_conflict("US67066G1040", existing)
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifier_conflicts_conflicting_identifier_id_global_sche_fkey"),
    );
    // The canonical mapping is unchanged: no merge, no source chosen as truth.
    let owner: Uuid =
        sqlx::query_scalar("SELECT node_id FROM identifiers WHERE value = 'US0378331005'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(owner, existing_node);
    db.teardown().await;
}

#[tokio::test]
async fn quarantine_rows_must_describe_a_coherent_claim() {
    let Some(db) = fresh().await else { return };
    let venue = db.node(Category::Venue).await;
    let instrument = db.node(Category::Instrument).await;
    let listing = db.listing(instrument, venue).await;
    let existing_symbol = symbol(&db, listing, venue, "NVDA", Validity::UNBOUNDED)
        .await
        .unwrap();
    let other_listing = db.listing(db.node(Category::Instrument).await, venue).await;

    let conflict = |namespace: &'static str,
                    value: &'static str,
                    scope: Option<Uuid>,
                    node: Uuid,
                    category: &'static str,
                    identifier: Option<i64>,
                    symbol: Option<i64>| {
        sqlx::query(
            "INSERT INTO identifier_conflicts
               (namespace, value, scope_venue_id, claimed_node_id, claimed_node_category,
                claimed_valid_during, source_id, received_at,
                conflicting_identifier_id, conflicting_listing_symbol_id)
             VALUES ($1, $2, $3, $4, $5, '(,)', $6, now(), $7, $8)",
        )
        .bind(namespace)
        .bind(value)
        .bind(scope)
        .bind(node)
        .bind(category)
        .bind(SOURCE)
        .bind(identifier)
        .bind(symbol)
    };

    // A venue-symbol conflict: another listing claimed NVDA on the same venue.
    conflict(
        "venue_symbol",
        "NVDA",
        Some(venue),
        other_listing,
        "listing",
        None,
        Some(existing_symbol),
    )
    .execute(&db.pool)
    .await
    .unwrap();
    // Missing venue scope.
    assert_rejected(
        conflict(
            "venue_symbol",
            "NVDA",
            None,
            other_listing,
            "listing",
            None,
            Some(existing_symbol),
        )
        .execute(&db.pool)
        .await,
        CHECK_VIOLATION,
        Some("identifier_conflicts_target"),
    );
    // Venue symbols can only be claimed for listings.
    assert_rejected(
        conflict(
            "venue_symbol",
            "NVDA",
            Some(venue),
            instrument,
            "instrument",
            None,
            Some(existing_symbol),
        )
        .execute(&db.pool)
        .await,
        CHECK_VIOLATION,
        Some("identifier_conflicts_target"),
    );
    // Referenced symbol row must match value and venue.
    assert_rejected(
        conflict(
            "venue_symbol",
            "AMD",
            Some(venue),
            other_listing,
            "listing",
            None,
            Some(existing_symbol),
        )
        .execute(&db.pool)
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifier_conflicts_conflicting_listing_symbol_id_scope_v_fkey"),
    );
    // A global-namespace claim must name an allowed category.
    let usd_node = db.node(Category::Currency).await;
    let usd = assign(
        &db,
        "iso4217",
        "USD",
        usd_node,
        Category::Currency,
        Validity::UNBOUNDED,
    )
    .await
    .unwrap();
    assert_rejected(
        conflict(
            "iso4217",
            "USD",
            None,
            instrument,
            "instrument",
            Some(usd),
            None,
        )
        .execute(&db.pool)
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifier_conflicts_global_scheme_claimed_node_category_fkey"),
    );
    db.teardown().await;
}

// --- graph edges -------------------------------------------------------------

async fn edge(
    db: &TestDb,
    subject: (Uuid, Category),
    relationship_type: &str,
    object: (Uuid, Category),
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO graph_edges
           (subject_id, subject_category, relationship_type, object_id, object_category, source_id, received_at)
         VALUES ($1, $2, $3, $4, $5, $6, now()) RETURNING id",
    )
    .bind(subject.0)
    .bind(subject.1.as_str())
    .bind(relationship_type)
    .bind(object.0)
    .bind(object.1.as_str())
    .bind(SOURCE)
    .fetch_one(&db.pool)
    .await
}

#[tokio::test]
async fn only_canonical_directions_are_stored() {
    let Some(db) = fresh().await else { return };
    let nvda = (db.node(Category::Instrument).await, Category::Instrument);
    let nvidia = (db.node(Category::Entity).await, Category::Entity);
    let nasdaq = (db.node(Category::Venue).await, Category::Venue);
    let usd = (db.node(Category::Currency).await, Category::Currency);
    let usdc = (db.crypto_asset().await, Category::Instrument);

    edge(&db, nvda, "ISSUED_BY", nvidia).await.unwrap();
    edge(&db, nvda, "TRADES_ON", nasdaq).await.unwrap();
    edge(&db, nvda, "DENOMINATED_IN", usd).await.unwrap();
    edge(&db, usdc, "DENOMINATED_IN", usd).await.unwrap();

    let rules = Some("graph_edges_relationship_type_subject_category_object_cate_fkey");
    // Reversed direction.
    assert_rejected(
        edge(&db, nvidia, "ISSUED_BY", nvda).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    // Inverse labels and projected types are never stored.
    assert_rejected(
        edge(&db, usdc, "UNDERLYING_OF", nvda).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    assert_rejected(
        edge(&db, nvda, "LISTED_ON", nasdaq).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    // Types whose node categories do not exist yet.
    assert_rejected(
        edge(&db, nvda, "DERIVES_FROM", usdc).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    assert_rejected(
        edge(&db, nvda, "RELATED_TO", usdc).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    // Currency is a node, but not a venue.
    assert_rejected(
        edge(&db, nvda, "TRADES_ON", usd).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );

    // Inverse traversal is derived at query time from the canonical edge.
    let issued: Vec<Uuid> = sqlx::query_scalar(
        "SELECT subject_id FROM graph_edges WHERE object_id = $1 AND relationship_type = 'ISSUED_BY'",
    )
    .bind(nvidia.0)
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(issued, vec![nvda.0]);
    db.teardown().await;
}

#[tokio::test]
async fn edges_cannot_lie_about_endpoint_categories() {
    let Some(db) = fresh().await else { return };
    let venue = db.node(Category::Venue).await;
    let entity = db.node(Category::Entity).await;
    assert_rejected(
        edge(
            &db,
            (venue, Category::Instrument),
            "ISSUED_BY",
            (entity, Category::Entity),
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("graph_edges_subject_id_subject_category_fkey"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn edges_have_provenance_and_no_self_or_duplicate_assertions() {
    let Some(db) = fresh().await else { return };
    let a = (db.crypto_asset().await, Category::Instrument);
    let b = (db.crypto_asset().await, Category::Instrument);
    assert_rejected(
        edge(&db, a, "DENOMINATED_IN", a).await,
        CHECK_VIOLATION,
        Some("graph_edges_no_self_edge"),
    );
    edge(&db, a, "SETTLES_IN", b).await.unwrap();
    assert_rejected(
        edge(&db, a, "SETTLES_IN", b).await,
        EXCLUSION_VIOLATION,
        Some("graph_edges_one_assertion_per_period"),
    );
    // A second source may assert the same edge.
    sqlx::query("INSERT INTO sources (id, name) VALUES ('second-source', 'Second')")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO graph_edges
           (subject_id, subject_category, relationship_type, object_id, object_category, source_id, received_at)
         VALUES ($1, 'instrument', 'SETTLES_IN', $2, 'instrument', 'second-source', now())",
    )
    .bind(a.0)
    .bind(b.0)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_rejected(
        sqlx::query(
            "INSERT INTO graph_edges
               (subject_id, subject_category, relationship_type, object_id, object_category, received_at)
             VALUES ($1, 'instrument', 'DENOMINATED_IN', $2, 'instrument', now())",
        )
        .bind(a.0)
        .bind(b.0)
        .execute(&db.pool)
        .await,
        NOT_NULL_VIOLATION,
        None,
    );
    db.teardown().await;
}

/// Edge validity is deferred: rows are current assertions. The schema must not
/// block a future where one source asserts an edge for several periods, so
/// uniqueness is "no overlapping periods", and the deferral is one CHECK.
#[tokio::test]
async fn edge_validity_is_deferred_without_blocking_multiple_periods() {
    let Some(db) = fresh().await else { return };
    let a = db.crypto_asset().await;
    let b = db.crypto_asset().await;
    let insert = |period: &'static str| {
        sqlx::query(
            "INSERT INTO graph_edges
               (subject_id, subject_category, relationship_type, object_id, object_category,
                valid_during, source_id, received_at)
             VALUES ($1, 'instrument', 'SETTLES_IN', $2, 'instrument', $3::tstzrange, $4, now())",
        )
        .bind(a)
        .bind(b)
        .bind(period)
        .bind(SOURCE)
    };
    // Phase 2: only unbounded (current) assertions.
    assert_rejected(
        insert("[2020-01-01,2021-01-01)").execute(&db.pool).await,
        CHECK_VIOLATION,
        Some("graph_edges_validity_deferred"),
    );
    // No constraint other than the deferral CHECK prevents disjoint periods:
    // with it dropped, two disjoint periods for the same tuple are accepted and
    // overlapping ones rejected.
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE graph_edges DROP CONSTRAINT graph_edges_validity_deferred")
        .execute(&mut *tx)
        .await
        .unwrap();
    insert("[2019-01-01,2021-01-01)")
        .execute(&mut *tx)
        .await
        .unwrap();
    insert("[2023-01-01,)").execute(&mut *tx).await.unwrap();
    let overlap = insert("[2020-06-01,2022-01-01)").execute(&mut *tx).await;
    assert_rejected(
        overlap,
        EXCLUSION_VIOLATION,
        Some("graph_edges_one_assertion_per_period"),
    );
    tx.rollback().await.unwrap();
    db.teardown().await;
}

// --- market observations -----------------------------------------------------

struct Obs {
    instrument: Uuid,
    basis: &'static str,
    venue: Option<Uuid>,
    price: &'static str,
    unit: (Uuid, &'static str),
    observed_at: &'static str,
}

async fn observe(db: &TestDb, o: &Obs) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO market_observations
           (instrument_id, basis, venue_id, price, unit_id, unit_category, source_id, observed_at, received_at)
         VALUES ($1, $2, $3, $4::numeric, $5, $6, $7, $8, now()) RETURNING id",
    )
    .bind(o.instrument)
    .bind(o.basis)
    .bind(o.venue)
    .bind(o.price)
    .bind(o.unit.0)
    .bind(o.unit.1)
    .bind(SOURCE)
    .bind(ts(o.observed_at))
    .fetch_one(&db.pool)
    .await
}

#[tokio::test]
async fn observation_basis_and_venue_agree() {
    let Some(db) = fresh().await else { return };
    let nvda = db.node(Category::Instrument).await;
    let nasdaq = db.node(Category::Venue).await;
    let usd = (db.node(Category::Currency).await, "currency");
    let base = Obs {
        instrument: nvda,
        basis: "venue",
        venue: Some(nasdaq),
        price: "183.4200",
        unit: usd,
        observed_at: "2026-09-24T12:00:00Z",
    };
    observe(&db, &base).await.unwrap();
    for (basis, venue) in [
        ("venue", None),
        ("aggregated", Some(nasdaq)),
        ("derived", Some(nasdaq)),
    ] {
        assert_rejected(
            observe(
                &db,
                &Obs {
                    basis,
                    venue,
                    observed_at: "2026-09-24T12:00:01Z",
                    ..base
                },
            )
            .await,
            CHECK_VIOLATION,
            Some("market_observations_basis_venue"),
        );
    }
    assert_rejected(
        observe(
            &db,
            &Obs {
                basis: "indicative",
                venue: None,
                ..base
            },
        )
        .await,
        CHECK_VIOLATION,
        Some("market_observations_basis_check"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn observation_units_are_canonical_currency_or_asset_nodes() {
    let Some(db) = fresh().await else { return };
    let nvda = db.node(Category::Instrument).await;
    let usd = db.node(Category::Currency).await;
    let usdc = db.crypto_asset().await;
    let venue = db.node(Category::Venue).await;
    let base = Obs {
        instrument: nvda,
        basis: "aggregated",
        venue: None,
        price: "183.4200",
        unit: (usd, "currency"),
        observed_at: "2026-09-24T12:00:00Z",
    };
    observe(&db, &base).await.unwrap();
    observe(
        &db,
        &Obs {
            unit: (usdc, "instrument"),
            ..base
        },
    )
    .await
    .unwrap();
    assert_rejected(
        observe(
            &db,
            &Obs {
                unit: (venue, "venue"),
                ..base
            },
        )
        .await,
        CHECK_VIOLATION,
        Some("market_observations_unit_category_check"),
    );
    // Declaring a false category is caught by the node FK.
    assert_rejected(
        observe(
            &db,
            &Obs {
                unit: (usdc, "currency"),
                observed_at: "2026-09-24T12:00:01Z",
                ..base
            },
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("market_observations_unit_id_unit_category_fkey"),
    );
    assert_rejected(
        observe(
            &db,
            &Obs {
                instrument: usdc,
                unit: (usdc, "instrument"),
                ..base
            },
        )
        .await,
        CHECK_VIOLATION,
        Some("market_observations_not_self_denominated"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn observation_replays_are_idempotent_including_null_venue() {
    let Some(db) = fresh().await else { return };
    let nvda = db.node(Category::Instrument).await;
    let nasdaq = db.node(Category::Venue).await;
    let usd = (db.node(Category::Currency).await, "currency");
    for (basis, venue) in [("venue", Some(nasdaq)), ("aggregated", None)] {
        let o = Obs {
            instrument: nvda,
            basis,
            venue,
            price: "183.4200",
            unit: usd,
            observed_at: "2026-09-24T12:00:00Z",
        };
        observe(&db, &o).await.unwrap();
        assert_rejected(
            observe(
                &db,
                &Obs {
                    price: "183.4300",
                    ..o
                },
            )
            .await,
            UNIQUE_VIOLATION,
            Some("market_observations_replay_key"),
        );
    }
    db.teardown().await;
}

// --- value round-trips ---------------------------------------------------------

#[tokio::test]
async fn decimals_keep_scale_and_stay_within_decimal_range() {
    let Some(db) = fresh().await else { return };
    let nvda = db.node(Category::Instrument).await;
    let usd = (db.node(Category::Currency).await, "currency");
    let cases = [
        ("183.4200", "2026-01-01T00:00:00Z"),
        ("0.00", "2026-01-02T00:00:00Z"),
        ("-37.63", "2026-01-03T00:00:00Z"),
        ("79228162514264337593543950335", "2026-01-04T00:00:00Z"),
        ("0.0000000000000000000000000001", "2026-01-05T00:00:00Z"),
    ];
    for (price, observed_at) in cases {
        let written = decimal_to_sql(Decimal::from_str(price).unwrap());
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO market_observations
               (instrument_id, basis, price, unit_id, unit_category, source_id, observed_at, received_at)
             VALUES ($1, 'derived', $2::numeric, $3, $4, $5, $6, now()) RETURNING id",
        )
        .bind(nvda)
        .bind(&written)
        .bind(usd.0)
        .bind(usd.1)
        .bind(SOURCE)
        .bind(ts(observed_at))
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let text: String =
            sqlx::query_scalar("SELECT price::text FROM market_observations WHERE id = $1")
                .bind(id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(text, price, "stored with scale");
        assert_eq!(
            decimal_from_sql(&text).unwrap().to_string(),
            price,
            "read with scale"
        );
    }
    for bad in [
        "NaN",
        "Infinity",
        "-Infinity",
        "79228162514264337593543950336",
        "0.00000000000000000000000000001",
    ] {
        assert_rejected(
            observe(
                &db,
                &Obs {
                    instrument: nvda,
                    basis: "derived",
                    venue: None,
                    price: bad,
                    unit: usd,
                    observed_at: "2026-02-01T00:00:00Z",
                },
            )
            .await,
            CHECK_VIOLATION,
            Some("financial_decimal_check"),
        );
    }
    db.teardown().await;
}

/// Why `undrly_store::mapping` transfers decimals as text: sqlx's native
/// `rust_decimal` codec loses the scale of zero in both directions. If this
/// test starts failing, sqlx fixed it and the text rule can be revisited.
#[tokio::test]
async fn sqlx_native_decimal_codec_drops_zero_scale() {
    let Some(db) = fresh().await else { return };
    let zero = Decimal::from_str("0.00").unwrap();
    let written: String = sqlx::query_scalar("SELECT $1::numeric::text")
        .bind(zero)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let read: Decimal = sqlx::query_scalar("SELECT '0.00'::numeric")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!((written.as_str(), read.to_string().as_str()), ("0", "0"));
    // Non-zero values keep their scale natively; the loss is specific to zero.
    let nonzero: Decimal = sqlx::query_scalar("SELECT '183.4200'::numeric")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(nonzero.to_string(), "183.4200");
    db.teardown().await;
}

#[tokio::test]
async fn timestamps_and_validity_round_trip_exactly() {
    let Some(db) = fresh().await else { return };
    let t = Timestamp::parse("2026-09-24T12:00:00.012345Z").unwrap();
    let back: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT $1::timestamptz")
        .bind(t.as_datetime())
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(Timestamp::from_datetime(back).unwrap(), t);

    let v = period(
        Some("2020-01-01T00:00:00Z"),
        Some("2021-01-01T00:00:00.500Z"),
    );
    let back: PgRange<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("SELECT $1::validity")
        .bind(validity_to_range(v))
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(validity_from_range(back).unwrap(), v);
    db.teardown().await;
}
