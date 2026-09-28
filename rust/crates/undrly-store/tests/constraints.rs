//! Database-level invariants. Each test inserts rows that violate one rule and
//! asserts the intended constraint rejected them.

mod common;

use std::str::FromStr;

use common::{
    CHECK_VIOLATION, EXCLUSION_VIOLATION, FOREIGN_KEY_VIOLATION, NOT_NULL_VIOLATION, RECEIVED_AT,
    SOURCE, TestDb, UNIQUE_VIOLATION, assert_rejected, fresh, ts,
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
        sqlx::query(
            "INSERT INTO entities (id, entity_kind, name, source_record_id) VALUES ($1, 'company', 'X', $2)",
        )
        .bind(venue)
        .bind(db.record)
        .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        Some("entities_id_category_fkey"),
    );
    // A category row cannot claim a different category.
    assert_rejected(
        sqlx::query(
            "INSERT INTO venues (id, category, name, source_record_id) VALUES ($1, 'entity', 'X', $2)",
        )
        .bind(Uuid::now_v7())
        .bind(db.record)
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
        "INSERT INTO identifiers
           (scheme, value, node_id, node_category, valid_during, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(scheme)
    .bind(value)
    .bind(node)
    .bind(category.as_str())
    .bind(validity_to_range(valid_during))
    .bind(SOURCE)
    .bind(ts(RECEIVED_AT))
    .bind(db.record)
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
    // A CIK identifies an SEC filer (entity), never a security.
    assert_rejected(
        assign(
            &db,
            "cik",
            "0001045810",
            instrument,
            Category::Instrument,
            Validity::UNBOUNDED,
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("identifiers_scheme_node_category_fkey"),
    );
    let entity = db.node(Category::Entity).await;
    assign(
        &db,
        "cik",
        "0001045810",
        entity,
        Category::Entity,
        Validity::UNBOUNDED,
    )
    .await
    .unwrap();
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
    let entity = db.node(Category::Entity).await;
    for (scheme, value, node, category) in [
        ("isin", "us0378331005", instrument, Category::Instrument),
        ("isin", "US037833100", instrument, Category::Instrument),
        ("figi", "BBG000BLANH6", instrument, Category::Instrument),
        ("mic", "xnas", venue, Category::Venue),
        ("mic", "XNASD", venue, Category::Venue),
        ("cik", "1045810", entity, Category::Entity),
        ("cik", "0000000000", entity, Category::Entity),
        ("cik", "00001045810", entity, Category::Entity),
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
                "INSERT INTO identifiers
                   (scheme, value, node_id, node_category, valid_during, source_id, received_at, source_record_id)
                 VALUES ('iso4217', 'USD', $1, 'currency', $2::tstzrange, $3, $4, $5)",
            )
            .bind(usd)
            .bind(bad)
            .bind(SOURCE)
            .bind(ts(RECEIVED_AT))
            .bind(db.record)
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
        "INSERT INTO listing_symbols
           (listing_id, venue_id, symbol, valid_during, source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(listing)
    .bind(venue)
    .bind(symbol)
    .bind(validity_to_range(valid_during))
    .bind(SOURCE)
    .bind(ts(RECEIVED_AT))
    .bind(db.record)
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
                source_id, received_at, conflicting_identifier_id, source_record_id)
             VALUES ('isin', $1, $2, 'instrument', '(,)', $3, $4, $5, $6)",
        )
        .bind(value)
        .bind(claimed_node)
        .bind(SOURCE)
        .bind(ts(RECEIVED_AT))
        .bind(existing)
        .bind(db.record)
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
                conflicting_identifier_id, conflicting_listing_symbol_id, source_record_id)
             VALUES ($1, $2, $3, $4, $5, '(,)', $6, $7, $8, $9, $10)",
        )
        .bind(namespace)
        .bind(value)
        .bind(scope)
        .bind(node)
        .bind(category)
        .bind(SOURCE)
        .bind(ts(RECEIVED_AT))
        .bind(identifier)
        .bind(symbol)
        .bind(db.record)
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
           (subject_id, subject_category, relationship_type, object_id, object_category,
            source_id, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(subject.0)
    .bind(subject.1.as_str())
    .bind(relationship_type)
    .bind(object.0)
    .bind(object.1.as_str())
    .bind(SOURCE)
    .bind(ts(RECEIVED_AT))
    .bind(db.record)
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
        edge(&db, nvda, "HOLDS", usdc).await,
        FOREIGN_KEY_VIOLATION,
        rules,
    );
    // A derivative derives from an instrument, never a currency.
    assert_rejected(
        edge(&db, nvda, "DERIVES_FROM", usd).await,
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
    let second_record: i64 = sqlx::query_scalar(
        "INSERT INTO source_records (source_id, record_key, payload, received_at)
         VALUES ('second-source', 'k', 'p', $1) RETURNING id",
    )
    .bind(ts(RECEIVED_AT))
    .fetch_one(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO graph_edges
           (subject_id, subject_category, relationship_type, object_id, object_category,
            source_id, received_at, source_record_id)
         VALUES ($1, 'instrument', 'SETTLES_IN', $2, 'instrument', 'second-source', $3, $4)",
    )
    .bind(a.0)
    .bind(b.0)
    .bind(ts(RECEIVED_AT))
    .bind(second_record)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_rejected(
        sqlx::query(
            "INSERT INTO graph_edges
               (subject_id, subject_category, relationship_type, object_id, object_category,
                received_at, source_record_id)
             VALUES ($1, 'instrument', 'DENOMINATED_IN', $2, 'instrument', $3, $4)",
        )
        .bind(a.0)
        .bind(b.0)
        .bind(ts(RECEIVED_AT))
        .bind(db.record)
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
                valid_during, source_id, received_at, source_record_id)
             VALUES ($1, 'instrument', 'SETTLES_IN', $2, 'instrument', $3::tstzrange, $4, $5, $6)",
        )
        .bind(a)
        .bind(b)
        .bind(period)
        .bind(SOURCE)
        .bind(ts(RECEIVED_AT))
        .bind(db.record)
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

// --- source-record provenance ------------------------------------------------

/// Every source-derived fact names the raw record that asserted it, and its
/// `source_id` / `received_at` are that record's.
#[tokio::test]
async fn facts_name_a_matching_source_record() {
    let Some(db) = fresh().await else { return };
    let usd = db.node(Category::Currency).await;
    sqlx::query("INSERT INTO sources (id, name) VALUES ('second-source', 'Second')")
        .execute(&db.pool)
        .await
        .unwrap();
    let other_source_record: i64 = sqlx::query_scalar(
        "INSERT INTO source_records (source_id, record_key, payload, received_at)
         VALUES ('second-source', 'k', 'p', $1) RETURNING id",
    )
    .bind(ts(RECEIVED_AT))
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let insert = |source: &'static str, received_at: &'static str, record: Option<i64>| {
        sqlx::query(
            "INSERT INTO identifiers
               (scheme, value, node_id, node_category, source_id, received_at, source_record_id)
             VALUES ('iso4217', 'USD', $1, 'currency', $2, $3, $4)",
        )
        .bind(usd)
        .bind(source)
        .bind(ts(received_at))
        .bind(record)
    };
    let fk = Some("identifiers_source_record_fkey");
    // No record.
    assert_rejected(
        insert(SOURCE, RECEIVED_AT, None).execute(&db.pool).await,
        NOT_NULL_VIOLATION,
        None,
    );
    // A record that does not exist.
    assert_rejected(
        insert(SOURCE, RECEIVED_AT, Some(i64::MAX))
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        fk,
    );
    // Another source's record.
    assert_rejected(
        insert(SOURCE, RECEIVED_AT, Some(other_source_record))
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        fk,
    );
    // The record's source, but a receipt time the record does not have.
    assert_rejected(
        insert(SOURCE, "2026-09-25T00:00:00Z", Some(db.record))
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        fk,
    );
    insert(SOURCE, RECEIVED_AT, Some(db.record))
        .execute(&db.pool)
        .await
        .unwrap();

    // Node objects name the record that minted them.
    let entity = Uuid::now_v7();
    sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, 'entity')")
        .bind(entity)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_rejected(
        sqlx::query(
            "INSERT INTO entities (id, entity_kind, name, source_record_id)
             VALUES ($1, 'company', 'X', $2)",
        )
        .bind(entity)
        .bind(i64::MAX)
        .execute(&db.pool)
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("entities_source_record_fkey"),
    );

    // Every fact table carries a mandatory source_record_id.
    let nullable: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, is_nullable::text FROM information_schema.columns
         WHERE table_schema = 'public' AND column_name = 'source_record_id' ORDER BY table_name",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    let expected: Vec<(String, String)> = [
        "aliases",
        "chains",
        "corporate_actions",
        "currencies",
        "deployments",
        "earnings_events",
        "economic_calendar_windows",
        "economic_release_dates",
        "entities",
        "graph_edges",
        "identifier_conflicts",
        "identifiers",
        "instruments",
        "listing_symbols",
        "listings",
        "market_bars",
        "market_observations",
        "perp_contexts",
        "quote_aggregations",
        "quote_feeds",
        "trading_calendar_ranges",
        "trading_sessions",
        "universe_snapshots",
        "venues",
    ]
    .into_iter()
    .map(|t| (t.to_owned(), "NO".to_owned()))
    .collect();
    assert_eq!(nullable, expected);

    // Raw evidence cannot be deleted while facts derive from it.
    assert_rejected(
        sqlx::query("DELETE FROM source_records WHERE id = $1")
            .bind(db.record)
            .execute(&db.pool)
            .await,
        FOREIGN_KEY_VIOLATION,
        None,
    );
    db.teardown().await;
}

// --- market observations -----------------------------------------------------

#[derive(Clone, Copy)]
struct Obs {
    subject: (Uuid, &'static str),
    basis: &'static str,
    venue: Option<Uuid>,
    price_type: &'static str,
    price: &'static str,
    bid_ask: Option<(&'static str, &'static str)>,
    unit: (Uuid, &'static str),
    observed_at: Option<&'static str>,
    record: i64,
}

/// A fresh raw record from `SOURCE` received at `RECEIVED_AT`.
async fn record(db: &TestDb) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO source_records (source_id, record_key, payload, received_at)
         VALUES ($1, $2, 'p', $3) RETURNING id",
    )
    .bind(SOURCE)
    .bind(Uuid::now_v7().to_string())
    .bind(ts(RECEIVED_AT))
    .fetch_one(&db.pool)
    .await
    .unwrap()
}

async fn observe(db: &TestDb, o: &Obs) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO market_observations
           (subject_id, subject_category, basis, venue_id, price_type, price, bid, ask,
            unit_id, unit_category, source_id, observed_at, received_at, source_record_id)
         VALUES ($1, $2, $3, $4, $5, $6::numeric, $7::numeric, $8::numeric,
                 $9, $10, $11, $12, $13, $14) RETURNING id",
    )
    .bind(o.subject.0)
    .bind(o.subject.1)
    .bind(o.basis)
    .bind(o.venue)
    .bind(o.price_type)
    .bind(o.price)
    .bind(o.bid_ask.map(|(b, _)| b))
    .bind(o.bid_ask.map(|(_, a)| a))
    .bind(o.unit.0)
    .bind(o.unit.1)
    .bind(SOURCE)
    .bind(o.observed_at.map(ts))
    .bind(ts(RECEIVED_AT))
    .bind(o.record)
    .fetch_one(&db.pool)
    .await
}

async fn base_obs(db: &TestDb) -> Obs {
    Obs {
        subject: (db.node(Category::Instrument).await, "instrument"),
        basis: "aggregated",
        venue: None,
        price_type: "last",
        price: "183.4200",
        bid_ask: None,
        unit: (db.node(Category::Currency).await, "currency"),
        observed_at: Some("2026-09-24T12:00:00Z"),
        record: record(db).await,
    }
}

#[tokio::test]
async fn observation_basis_and_venue_agree() {
    let Some(db) = fresh().await else { return };
    let nasdaq = db.node(Category::Venue).await;
    let base = Obs {
        basis: "venue",
        venue: Some(nasdaq),
        ..base_obs(&db).await
    };
    observe(&db, &base).await.unwrap();
    for (basis, venue) in [
        ("venue", None),
        ("aggregated", Some(nasdaq)),
        ("derived", Some(nasdaq)),
    ] {
        let o = Obs {
            basis,
            venue,
            record: record(&db).await,
            ..base
        };
        assert_rejected(
            observe(&db, &o).await,
            CHECK_VIOLATION,
            Some("market_observations_basis_venue"),
        );
    }
    let o = Obs {
        basis: "indicative",
        venue: None,
        record: record(&db).await,
        ..base
    };
    assert_rejected(
        observe(&db, &o).await,
        CHECK_VIOLATION,
        Some("market_observations_basis_check"),
    );
    let o = Obs {
        price_type: "close",
        record: record(&db).await,
        ..base
    };
    assert_rejected(
        observe(&db, &o).await,
        CHECK_VIOLATION,
        Some("market_observations_price_type_check"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn observation_subjects_and_units_are_canonical_nodes() {
    let Some(db) = fresh().await else { return };
    let base = base_obs(&db).await;
    let usdc = db.crypto_asset().await;
    let eur = db.node(Category::Currency).await;
    let venue = db.node(Category::Venue).await;
    observe(&db, &base).await.unwrap();
    // FX: a currency priced in a currency.
    observe(
        &db,
        &Obs {
            subject: (eur, "currency"),
            record: record(&db).await,
            ..base
        },
    )
    .await
    .unwrap();
    // A crypto asset as the unit (e.g. a perpetual in USDC).
    observe(
        &db,
        &Obs {
            unit: (usdc, "instrument"),
            record: record(&db).await,
            ..base
        },
    )
    .await
    .unwrap();
    let rejected = [
        (
            Obs {
                subject: (venue, "venue"),
                ..base
            },
            CHECK_VIOLATION,
            "market_observations_subject_category_check",
        ),
        (
            Obs {
                unit: (venue, "venue"),
                ..base
            },
            CHECK_VIOLATION,
            "market_observations_unit_category_check",
        ),
        (
            Obs {
                unit: (usdc, "currency"),
                ..base
            },
            FOREIGN_KEY_VIOLATION,
            "market_observations_unit_id_unit_category_fkey",
        ),
        (
            Obs {
                subject: (usdc, "instrument"),
                unit: (usdc, "instrument"),
                ..base
            },
            CHECK_VIOLATION,
            "market_observations_not_self_denominated",
        ),
        (
            Obs {
                bid_ask: Some(("2.0", "1.0")),
                ..base
            },
            CHECK_VIOLATION,
            "market_observations_bid_ask",
        ),
    ];
    for (o, sqlstate, constraint) in rejected {
        // No source time, so no replay key can fire before the constraint
        // under test.
        let o = Obs {
            observed_at: None,
            record: record(&db).await,
            ..o
        };
        assert_rejected(observe(&db, &o).await, sqlstate, Some(constraint));
    }
    // Half a spread is not a spread.
    let o = Obs {
        record: record(&db).await,
        ..base
    };
    assert_rejected(
        sqlx::query(
            "INSERT INTO market_observations
               (subject_id, subject_category, basis, price_type, price, bid, unit_id,
                unit_category, source_id, received_at, source_record_id)
             VALUES ($1, 'instrument', 'aggregated', 'last', 1, 1, $2, 'currency', $3, $4, $5)",
        )
        .bind(o.subject.0)
        .bind(o.unit.0)
        .bind(SOURCE)
        .bind(ts(RECEIVED_AT))
        .bind(o.record)
        .execute(&db.pool)
        .await,
        CHECK_VIOLATION,
        Some("market_observations_bid_ask"),
    );
    db.teardown().await;
}

#[tokio::test]
async fn observation_replays_are_idempotent() {
    let Some(db) = fresh().await else { return };
    let nasdaq = db.node(Category::Venue).await;
    let base = base_obs(&db).await;
    for (basis, venue) in [("venue", Some(nasdaq)), ("aggregated", None)] {
        let o = Obs {
            basis,
            venue,
            record: record(&db).await,
            ..base
        };
        observe(&db, &o).await.unwrap();
        // The same record cannot yield the pair twice for one source time...
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
        // ...but a series payload yields it once per source time (V1.3 history).
        observe(
            &db,
            &Obs {
                price: "183.4300",
                observed_at: Some("2026-09-23T12:00:00Z"),
                ..o
            },
        )
        .await
        .unwrap();
        // Another response restating the same source time is the same
        // observation.
        assert_rejected(
            observe(
                &db,
                &Obs {
                    record: record(&db).await,
                    ..o
                },
            )
            .await,
            UNIQUE_VIOLATION,
            Some("market_observations_replay_key"),
        );
    }
    // Without a source time, each response is its own observation.
    for _ in 0..2 {
        let o = Obs {
            observed_at: None,
            record: record(&db).await,
            ..base
        };
        observe(&db, &o).await.unwrap();
    }
    db.teardown().await;
}

#[tokio::test]
async fn observations_feeds_aliases_and_canonical_quotes_have_provenance() {
    let Some(db) = fresh().await else { return };
    let base = base_obs(&db).await;
    sqlx::query("INSERT INTO sources (id, name) VALUES ('second-source', 'Second')")
        .execute(&db.pool)
        .await
        .unwrap();
    let foreign: i64 = sqlx::query_scalar(
        "INSERT INTO source_records (source_id, record_key, payload, received_at)
         VALUES ('second-source', 'k', 'p', $1) RETURNING id",
    )
    .bind(ts(RECEIVED_AT))
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_rejected(
        observe(
            &db,
            &Obs {
                record: foreign,
                ..base
            },
        )
        .await,
        FOREIGN_KEY_VIOLATION,
        Some("market_observations_source_record_fkey"),
    );

    let feed = |basis: &'static str, venue: Option<Uuid>, record: i64| {
        sqlx::query(
            "INSERT INTO quote_feeds
               (feed_source_id, symbol, subject_id, subject_category, unit_id, unit_category,
                basis, venue_id, price_type, source_id, received_at, source_record_id)
             VALUES ($1, 'XBTUSD', $2, 'instrument', $3, 'currency', $4, $5, 'last', $1, $6, $7)",
        )
        .bind(SOURCE)
        .bind(base.subject.0)
        .bind(base.unit.0)
        .bind(basis)
        .bind(venue)
        .bind(ts(RECEIVED_AT))
        .bind(record)
    };
    assert_rejected(
        feed("venue", None, db.record).execute(&db.pool).await,
        CHECK_VIOLATION,
        Some("quote_feeds_basis_venue"),
    );
    assert_rejected(
        feed("aggregated", None, foreign).execute(&db.pool).await,
        FOREIGN_KEY_VIOLATION,
        Some("quote_feeds_source_record_fkey"),
    );
    feed("aggregated", None, db.record)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_rejected(
        feed("aggregated", None, db.record).execute(&db.pool).await,
        UNIQUE_VIOLATION,
        Some("quote_feeds_one_per_symbol"),
    );

    let alias = |text: &'static str| {
        sqlx::query(
            "INSERT INTO aliases
               (node_id, node_category, alias, kind, source_id, received_at, source_record_id)
             VALUES ($1, 'instrument', $2, 'symbol', $3, $4, $5)",
        )
        .bind(base.subject.0)
        .bind(text)
        .bind(SOURCE)
        .bind(ts(RECEIVED_AT))
        .bind(db.record)
    };
    alias("BTC").execute(&db.pool).await.unwrap();
    // Aliases are case-insensitive per node, kind and source.
    assert_rejected(
        alias("btc").execute(&db.pool).await,
        UNIQUE_VIOLATION,
        Some("aliases_one_per_source"),
    );
    assert_rejected(
        alias(" BTC").execute(&db.pool).await,
        CHECK_VIOLATION,
        Some("display_name_check"),
    );

    // A canonical quote's inputs must be observations of that very pair.
    let observation = observe(&db, &base).await.unwrap();
    let other_unit = db.node(Category::Currency).await;
    let quote = |unit: Uuid,
                 method: &'static str,
                 basis: &'static str,
                 bid: Option<&'static str>,
                 ask: Option<&'static str>| {
        sqlx::query(
            "INSERT INTO canonical_quotes
               (subject_id, subject_category, unit_id, unit_category, method, price, price_type,
                basis, as_of, eligible_count, computed_at, bid, ask)
             VALUES ($1, 'instrument', $2, 'currency', $3, 1, 'mid', $4, now(), 1, now(),
                     $5::numeric, $6::numeric)",
        )
        .bind(base.subject.0)
        .bind(unit)
        .bind(method)
        .bind(basis)
        .bind(bid)
        .bind(ask)
    };
    let canonical = |unit: Uuid, method: &'static str, basis: &'static str| {
        quote(unit, method, basis, Some("0.9"), Some("1.1"))
    };
    // A mean of venue mids states its mean bid and ask, around its price.
    for (bid, ask) in [
        (None, None),
        (Some("1.1"), Some("1.2")),
        (Some("0.8"), Some("0.9")),
    ] {
        assert_rejected(
            quote(base.unit.0, "mean-venue-mid-v1", "aggregated", bid, ask)
                .execute(&db.pool)
                .await,
            CHECK_VIOLATION,
            Some("canonical_quotes_mean_has_bid_ask"),
        );
    }
    // Bid and ask come together, uncrossed.
    for (bid, ask) in [(Some("0.9"), None), (Some("1.1"), Some("0.9"))] {
        assert_rejected(
            quote(base.unit.0, "latest-observation-v1", "aggregated", bid, ask)
                .execute(&db.pool)
                .await,
            CHECK_VIOLATION,
            Some("canonical_quotes_bid_ask"),
        );
    }
    // A mean of venue mids is never attributed to a venue.
    assert_rejected(
        canonical(base.unit.0, "mean-venue-mid-v1", "derived")
            .execute(&db.pool)
            .await,
        CHECK_VIOLATION,
        Some("canonical_quotes_mean_is_aggregated"),
    );
    canonical(other_unit, "mean-venue-mid-v1", "aggregated")
        .execute(&db.pool)
        .await
        .unwrap();
    canonical(base.unit.0, "mean-venue-mid-v1", "aggregated")
        .execute(&db.pool)
        .await
        .unwrap();
    let input = |unit: Uuid| {
        sqlx::query(
            "INSERT INTO canonical_quote_inputs (subject_id, unit_id, observation_id, input_price)
             VALUES ($1, $2, $3, 1)",
        )
        .bind(base.subject.0)
        .bind(unit)
        .bind(observation)
    };
    assert_rejected(
        input(other_unit).execute(&db.pool).await,
        FOREIGN_KEY_VIOLATION,
        Some("canonical_quote_inputs_observation_id_subject_id_unit_id_fkey"),
    );
    input(base.unit.0).execute(&db.pool).await.unwrap();
    // Removing a canonical quote removes its inputs, never the observation.
    sqlx::query("DELETE FROM canonical_quotes WHERE subject_id = $1")
        .bind(base.subject.0)
        .execute(&db.pool)
        .await
        .unwrap();
    let left: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM canonical_quote_inputs),
                (SELECT count(*) FROM market_observations WHERE id = $1)",
    )
    .bind(observation)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(left, (0, 1));
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
        let id = observe(
            &db,
            &Obs {
                subject: (nvda, "instrument"),
                basis: "derived",
                venue: None,
                price_type: "last",
                price: &*written.clone().leak(),
                bid_ask: None,
                unit: usd,
                observed_at: Some(observed_at),
                record: record(&db).await,
            },
        )
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
                    subject: (nvda, "instrument"),
                    basis: "derived",
                    venue: None,
                    price_type: "last",
                    price: bad,
                    bid_ask: None,
                    unit: usd,
                    observed_at: Some("2026-02-01T00:00:00Z"),
                    record: record(&db).await,
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
