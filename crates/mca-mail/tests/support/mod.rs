//! Helpers shared by the persistence and backup integration tests.
//!
//! The backup suite needs the same real PostgreSQL the persistence suite uses,
//! so the connection and migration bootstrap live here rather than being
//! duplicated in a second file.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use mca_mail::config::DatabaseSettings;

/// The URL the integration suites run against.
///
/// Without it the tests are skipped rather than silently passing, which is why
/// this returns `Option` instead of panicking.
pub fn database_url() -> Option<String> {
    std::env::var("MCA_TEST_DATABASE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

/// Connect and bring the schema up to date.
pub async fn pool() -> Option<PgPool> {
    let url = database_url()?;
    let settings = DatabaseSettings {
        url,
        max_connections: 4,
        min_connections: 0,
        auto_migrate: false,
        ..Default::default()
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&settings.url)
        .await
        .ok()?;
    // Migrations are embedded in the test binary, so a bare CI database comes up
    // to date without an external sqlx-cli install. A failure here fails the
    // suite: skipping instead would make CI green without testing anything.
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    Some(pool)
}

/// A unique per-test suffix, so suites can run against a shared database.
pub fn unique(tag: &str) -> String {
    format!("{tag}-{}", uuid::Uuid::new_v4())
}

/// A scratch directory under the system temp dir, named per test.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join("mca-mail-tests")
        .join(unique(tag))
}
