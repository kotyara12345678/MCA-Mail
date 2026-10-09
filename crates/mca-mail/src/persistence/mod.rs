//! PostgreSQL access. All SQL lives here; nothing above this module knows the
//! schema, and nothing below it performs business decisions.

pub mod api_key_repo;
pub mod audit_repo;
pub mod conversation_repo;
pub mod cursor_repo;
pub mod draft_repo;
pub mod email_repo;
pub mod event_repo;
pub mod handoff_repo;
pub mod lead_repo;
pub mod outbox_repo;
pub mod pool;
pub mod requirement_repo;
pub mod research_repo;
pub mod retention_repo;
pub mod rows;
pub mod run_repo;
pub mod settings_repo;
pub mod thread_repo;
pub mod voice_repo;

pub use pool::{connect, health, is_unique_violation, short_db_error, Health};

use std::str::FromStr;

use sqlx::{postgres::PgRow, Row};

use crate::domain::WireParseError;

/// Parse a `TEXT` column into a wire enum.
pub fn parse_enum<T>(value: &str, column: &str) -> Result<T, sqlx::Error>
where
    T: FromStr<Err = WireParseError>,
{
    value
        .parse::<T>()
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
        .map_err(|e: sqlx::Error| {
            tracing::warn!(error = %e, column, "stored value is not a known enum variant");
            e
        })
}

/// Read an optional `TEXT` column that may hold an enum.
pub fn parse_enum_opt<T>(value: Option<&str>, column: &str) -> Result<Option<T>, sqlx::Error>
where
    T: FromStr<Err = WireParseError>,
{
    value.map(|v| parse_enum::<T>(v, column)).transpose()
}

/// Convert a `Vec<String>` column into a `Vec<T>`.
pub fn parse_enum_vec<T>(values: &[String], column: &str) -> Result<Vec<T>, sqlx::Error>
where
    T: FromStr<Err = WireParseError>,
{
    values.iter().map(|v| parse_enum::<T>(v, column)).collect()
}

/// Read a column that may be absent, treating an unknown column as `None`.
pub fn opt_string(row: &PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten()
}

pub fn req_string(row: &PgRow, column: &str) -> Result<String, sqlx::Error> {
    row.try_get::<String, _>(column)
}
