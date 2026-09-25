//! Database test harness (feature `testing`), shared by the database tests of
//! `undrly-store` and `undrly-ingest`.
//!
//! Every test gets a freshly created database migrated from empty, so tests
//! exercise the migrations themselves and cannot see each other's rows.
//!
//! - `DATABASE_URL` unset: the test is skipped (opt-in locally).
//! - `UNDRLY_REQUIRE_DATABASE=1` (CI): a missing `DATABASE_URL` fails the test.
//!
//! The role in `DATABASE_URL` needs `CREATEDB`.

use std::fmt::Debug;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, ConnectOptions, Connection, PgPool};
use undrly_core::{Category, Timestamp};

pub const SOURCE: &str = "example-source";

pub struct TestDb {
    pub pool: PgPool,
    admin: PgConnectOptions,
    name: String,
}

/// Returns a migrated, empty database, or `None` when database tests are not
/// enabled.
pub async fn fresh() -> Option<TestDb> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        if std::env::var("UNDRLY_REQUIRE_DATABASE").is_ok_and(|v| v == "1") {
            panic!("UNDRLY_REQUIRE_DATABASE=1 but DATABASE_URL is not set");
        }
        eprintln!("skipping database test: DATABASE_URL is not set");
        return None;
    };
    let admin = PgConnectOptions::from_str(&url).expect("valid DATABASE_URL");
    let name = format!("undrly_test_{}", Uuid::now_v7().simple());
    let mut conn = admin.connect().await.expect("connect to DATABASE_URL");
    // `name` is generated above, never external input.
    sqlx::raw_sql(AssertSqlSafe(format!(r#"CREATE DATABASE "{name}""#)))
        .execute(&mut conn)
        .await
        .expect("create test database (role needs CREATEDB)");
    conn.close().await.ok();

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(admin.clone().database(&name))
        .await
        .expect("connect to test database");
    crate::MIGRATOR
        .run(&pool)
        .await
        .expect("migrations apply to an empty database");
    // Seed the source used by most tests.
    sqlx::query("INSERT INTO sources (id, name) VALUES ($1, 'Example Source')")
        .bind(SOURCE)
        .execute(&pool)
        .await
        .unwrap();
    Some(TestDb { pool, admin, name })
}

impl TestDb {
    /// Drops the test database. Tests call this at the end; a panicking test
    /// leaves its `undrly_test_*` database behind for inspection.
    pub async fn teardown(self) {
        self.pool.close().await;
        let mut conn = self.admin.connect().await.unwrap();
        sqlx::raw_sql(AssertSqlSafe(format!(
            r#"DROP DATABASE "{}" WITH (FORCE)"#,
            self.name
        )))
        .execute(&mut conn)
        .await
        .unwrap();
    }

    /// Registers a node of `category` and inserts its category row.
    pub async fn node(&self, category: Category) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, $2)")
            .bind(id)
            .bind(category.as_str())
            .execute(&self.pool)
            .await
            .unwrap();
        let sql = match category {
            Category::Entity => {
                "INSERT INTO entities (id, entity_kind, name) VALUES ($1, 'company', 'Test Entity')"
            }
            Category::Instrument => {
                "INSERT INTO instruments (id, instrument_class, name) VALUES ($1, 'equity', 'Test Instrument')"
            }
            Category::Venue => "INSERT INTO venues (id, name) VALUES ($1, 'Test Venue')",
            Category::Currency => "INSERT INTO currencies (id, name) VALUES ($1, 'Test Currency')",
            Category::Listing => panic!("use TestDb::listing"),
        };
        sqlx::query(sql).bind(id).execute(&self.pool).await.unwrap();
        id
    }

    pub async fn crypto_asset(&self) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, 'instrument')")
            .bind(id)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO instruments (id, instrument_class, name) VALUES ($1, 'crypto_asset', 'Test Asset')",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    pub async fn listing(&self, instrument_id: Uuid, venue_id: Uuid) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO nodes (id, category) VALUES ($1, 'listing')")
            .bind(id)
            .execute(&self.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO listings (id, instrument_id, venue_id, source_id, received_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(id)
        .bind(instrument_id)
        .bind(venue_id)
        .bind(SOURCE)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }
}

pub fn ts(s: &str) -> DateTime<Utc> {
    Timestamp::parse(s).unwrap().as_datetime()
}

/// Asserts the database rejected the statement with `sqlstate`, and, when
/// given, that the named constraint is the one that fired. This proves the
/// intended invariant rejected the row, not some unrelated failure.
#[track_caller]
pub fn assert_rejected<T: Debug>(
    result: Result<T, sqlx::Error>,
    sqlstate: &str,
    constraint: Option<&str>,
) {
    let err = match result {
        Ok(value) => panic!("expected rejection with {sqlstate}, got Ok({value:?})"),
        Err(err) => err,
    };
    let Some(db) = err.as_database_error() else {
        panic!("expected database error {sqlstate}, got {err}");
    };
    assert_eq!(db.code().as_deref(), Some(sqlstate), "{db}");
    if let Some(expected) = constraint {
        assert_eq!(db.constraint(), Some(expected), "{db}");
    }
}

pub const CHECK_VIOLATION: &str = "23514";
pub const FOREIGN_KEY_VIOLATION: &str = "23503";
pub const UNIQUE_VIOLATION: &str = "23505";
pub const NOT_NULL_VIOLATION: &str = "23502";
pub const EXCLUSION_VIOLATION: &str = "23P01";
