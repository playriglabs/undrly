//! Migrations apply from empty, and database vocabularies equal the Rust
//! domain and the shared fixtures.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use undrly_core::{
    AggregationMethod, Category, Cik, CurrencyCode, EntityKind, Figi, InstrumentClass, Isin, Lei,
    Mic, Namespace, ObservationBasis, PriceType, RelationshipType,
};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[tokio::test]
async fn migrations_apply_to_empty_database_and_are_idempotent() {
    let Some(db) = common::fresh().await else {
        return;
    };

    let applied: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    let expected: Vec<(i64, bool)> = undrly_store::MIGRATOR
        .iter()
        .map(|m| (m.version, true))
        .collect();
    assert_eq!(applied, expected);
    assert_eq!(applied.len(), 12);

    // Re-running is a no-op.
    undrly_store::MIGRATOR.run(&db.pool).await.unwrap();

    let tables: BTreeSet<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap()
    .into_iter()
    .collect();
    let expected_tables: BTreeSet<String> = [
        "sources",
        "nodes",
        "entities",
        "instruments",
        "venues",
        "currencies",
        "listings",
        "identifier_schemes",
        "identifiers",
        "listing_symbols",
        "identifier_conflicts",
        "relationship_rules",
        "graph_edges",
        "market_observations",
        "source_records",
        "aliases",
        "quote_feeds",
        "canonical_quotes",
        "canonical_quote_inputs",
        "quote_aggregations",
        "universe_snapshots",
        "universe_members",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(tables, expected_tables);

    let no_json_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns
         WHERE table_schema = 'public' AND data_type IN ('json', 'jsonb')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        no_json_columns, 0,
        "the canonical domain is stored relationally"
    );

    db.teardown().await;
}

#[tokio::test]
async fn relationship_rules_equal_core_and_fixture() {
    let Some(db) = common::fresh().await else {
        return;
    };

    let db_rules: BTreeSet<(String, String, String)> = sqlx::query_as(
        "SELECT relationship_type, subject_category, object_category FROM relationship_rules",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap()
    .into_iter()
    .collect();
    let core_rules: BTreeSet<(String, String, String)> = RelationshipType::ALL
        .iter()
        .flat_map(|t| {
            t.allowed_endpoints()
                .iter()
                .map(|(s, o)| (t.as_str().to_owned(), s.to_string(), o.to_string()))
        })
        .collect();
    assert_eq!(db_rules, core_rules);

    let fixture_rules: BTreeSet<(String, String, String)> =
        serde_json::from_value::<Vec<(String, String, String)>>(
            fixture("shared/vocabulary.json")["relationshipRules"].clone(),
        )
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(db_rules, fixture_rules);

    db.teardown().await;
}

#[tokio::test]
async fn identifier_schemes_equal_core() {
    let Some(db) = common::fresh().await else {
        return;
    };

    let db_schemes: BTreeSet<(String, String)> =
        sqlx::query_as("SELECT scheme, node_category FROM identifier_schemes")
            .fetch_all(&db.pool)
            .await
            .unwrap()
            .into_iter()
            .collect();
    let core_schemes: BTreeSet<(String, String)> = Namespace::ALL
        .iter()
        .flat_map(|ns| {
            ns.categories()
                .iter()
                .map(|c| (ns.as_str().to_owned(), c.to_string()))
        })
        .collect();
    assert_eq!(db_schemes, core_schemes);

    db.teardown().await;
}

/// Every value the database accepts in a text-enum CHECK must be a core
/// name, and every core name must be accepted.
#[tokio::test]
async fn check_constraint_vocabularies_equal_core() {
    let Some(db) = common::fresh().await else {
        return;
    };

    async fn accepted(db: &common::TestDb, domain_sql: &'static str, value: &str) -> bool {
        // Evaluate the CHECK in a rolled-back transaction.
        let mut tx = db.pool.begin().await.unwrap();
        let ok = sqlx::query(domain_sql)
            .bind(value)
            .execute(&mut *tx)
            .await
            .is_ok();
        tx.rollback().await.unwrap();
        ok
    }

    let node_sql = "INSERT INTO nodes (id, category) VALUES (uuidv7_test(), $1)";
    sqlx::query(
        "CREATE FUNCTION uuidv7_test() RETURNS uuid LANGUAGE sql AS
         $$ SELECT (lpad(to_hex((extract(epoch FROM clock_timestamp()) * 1000)::bigint), 12, '0')
                    || '7' || substr(md5(random()::text), 1, 3)
                    || '8' || substr(md5(random()::text), 1, 15))::uuid $$",
    )
    .execute(&db.pool)
    .await
    .unwrap();

    for c in Category::ALL {
        assert!(accepted(&db, node_sql, c.as_str()).await, "{c}");
    }
    for bogus in ["chain", "company", "Entity", ""] {
        assert!(!accepted(&db, node_sql, bogus).await, "{bogus}");
    }

    let entity_sql =
        "WITH n AS (INSERT INTO nodes (id, category) VALUES (uuidv7_test(), 'entity') RETURNING id)
                      INSERT INTO entities (id, entity_kind, name, source_record_id)
                      SELECT id, $1, 'x', (SELECT min(id) FROM source_records) FROM n";
    for k in EntityKind::ALL {
        assert!(accepted(&db, entity_sql, k.as_str()).await);
    }
    assert!(!accepted(&db, entity_sql, "person").await);

    let instrument_sql = "WITH n AS (INSERT INTO nodes (id, category) VALUES (uuidv7_test(), 'instrument') RETURNING id)
                          INSERT INTO instruments (id, instrument_class, name, source_record_id)
                          SELECT id, $1, 'x', (SELECT min(id) FROM source_records) FROM n";
    for c in InstrumentClass::ALL {
        assert!(accepted(&db, instrument_sql, c.as_str()).await);
    }
    for bogus in ["fx", "currency", "Equity"] {
        assert!(!accepted(&db, instrument_sql, bogus).await, "{bogus}");
    }

    let constraint: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint
         WHERE conrelid = 'market_observations'::regclass AND conname = 'market_observations_basis_check'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    for name in ObservationBasis::NAMES {
        assert!(constraint.contains(&format!("'{name}'")), "{constraint}");
    }
    for (table, constraint, names) in [
        (
            "market_observations",
            "market_observations_price_type_check",
            PriceType::ALL.map(PriceType::as_str).to_vec(),
        ),
        (
            "quote_feeds",
            "quote_feeds_price_type_check",
            PriceType::ALL.map(PriceType::as_str).to_vec(),
        ),
        (
            "canonical_quotes",
            "canonical_quotes_method_check",
            AggregationMethod::ALL
                .map(AggregationMethod::as_str)
                .to_vec(),
        ),
        (
            "quote_aggregations",
            "quote_aggregations_method_check",
            AggregationMethod::ALL
                .map(AggregationMethod::as_str)
                .to_vec(),
        ),
    ] {
        let def: String = sqlx::query_scalar(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint
             WHERE conrelid = $1::regclass AND conname = $2",
        )
        .bind(table)
        .bind(constraint)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let quoted = def.matches('\'').count() / 2;
        assert_eq!(quoted, names.len(), "{def}");
        for name in names {
            assert!(def.contains(&format!("'{name}'")), "{def}");
        }
    }

    db.teardown().await;
}

/// The database checks identifier shape only (check digits are validated in
/// Rust). Every value Rust accepts must also pass the database checks.
#[tokio::test]
async fn rust_valid_identifiers_pass_database_shape_checks() {
    let Some(db) = common::fresh().await else {
        return;
    };
    let fixture = fixture("identifiers.json");
    let strings = |key: &str| -> Vec<String> {
        fixture[key]["valid"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };

    let mut checked = 0;
    for (scheme, values) in [
        ("isin", strings("isin")),
        ("figi", strings("figi")),
        ("lei", strings("lei")),
        ("mic", strings("mic")),
        ("iso4217", strings("iso4217")),
        ("cik", strings("cik")),
    ] {
        for value in values {
            let rust_ok = match scheme {
                "isin" => Isin::parse(&value).is_ok(),
                "figi" => Figi::parse(&value).is_ok(),
                "lei" => Lei::parse(&value).is_ok(),
                "mic" => Mic::parse(&value).is_ok(),
                "cik" => Cik::parse(&value).is_ok(),
                _ => CurrencyCode::parse(&value).is_ok(),
            };
            assert!(rust_ok, "{scheme} {value}");
            let db_ok: bool = sqlx::query_scalar(
                "SELECT CASE $1
                   WHEN 'isin'    THEN $2 ~ '^[A-Z]{2}[A-Z0-9]{9}[0-9]$'
                   WHEN 'figi'    THEN $2 ~ '^[B-DF-HJ-NP-TV-Z0-9]{2}G[B-DF-HJ-NP-TV-Z0-9]{8}[0-9]$'
                   WHEN 'lei'     THEN $2 ~ '^[A-Z0-9]{18}[0-9]{2}$'
                   WHEN 'mic'     THEN $2 ~ '^[A-Z0-9]{4}$'
                   WHEN 'iso4217' THEN $2 ~ '^[A-Z]{3}$'
                   WHEN 'cik'     THEN $2 ~ '^[0-9]{10}$' AND $2 <> '0000000000'
                 END",
            )
            .bind(scheme)
            .bind(&value)
            .fetch_one(&db.pool)
            .await
            .unwrap();
            assert!(db_ok, "{scheme} {value}");
            checked += 1;
        }
    }
    assert!(checked > 0);

    // The expression above must be the one installed on the table.
    let installed: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'identifiers_value_shape'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    for pattern in [
        "^[A-Z]{2}[A-Z0-9]{9}[0-9]$",
        "^[B-DF-HJ-NP-TV-Z0-9]{2}G[B-DF-HJ-NP-TV-Z0-9]{8}[0-9]$",
        "^[A-Z0-9]{18}[0-9]{2}$",
        "^[A-Z0-9]{4}$",
        "^[A-Z]{3}$",
        "^[0-9]{10}$",
    ] {
        assert!(installed.contains(pattern), "{pattern} not in {installed}");
    }

    db.teardown().await;
}

/// 0007 adds a mandatory `source_record_id` to fact tables. Facts stored
/// before it cannot be traced to a record, so the migration refuses to run
/// over them instead of inventing a link.
#[tokio::test]
async fn source_record_migration_refuses_untraceable_facts() {
    let Some(db) = common::fresh().await else {
        return;
    };
    let mut conn = db.pool.acquire().await.unwrap();
    // Replay 0001–0006 into a separate schema, add a pre-0007 fact, then 0007.
    sqlx::raw_sql("CREATE SCHEMA pre_0007; SET search_path = pre_0007, public")
        .execute(&mut *conn)
        .await
        .unwrap();
    let (before, rest) = undrly_store::MIGRATOR.migrations.split_at(6);
    for migration in before {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO sources (id, name) VALUES ('example-source', 'Example');
         INSERT INTO nodes (id, category) VALUES ('01920000-0000-7000-8000-000000000000', 'venue');
         INSERT INTO venues (id, name) VALUES ('01920000-0000-7000-8000-000000000000', 'Venue');",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    assert_eq!(rest[0].version, 7);
    let err = sqlx::raw_sql(rest[0].sql.clone())
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("requires empty fact tables"),
        "{err}"
    );
    drop(conn);
    db.teardown().await;
}
