use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};

use crate::config::DatabaseSettings;
use crate::error::AppError;

/// Connect with bounded timeouts and a bounded pool.
///
/// Every timeout is explicit: a hung database must not be able to stall a mail
/// worker forever, and the API must fail fast enough for a load balancer to
/// take the instance out of rotation.
pub async fn connect(settings: &DatabaseSettings) -> Result<PgPool, AppError> {
    let options: PgConnectOptions = settings
        .url
        .parse::<PgConnectOptions>()
        .map_err(|e| {
            AppError::Config(crate::error::ConfigError::Invalid {
                field: "DATABASE_URL".into(),
                reason: format!("{e}"),
            })
        })?
        .log_statements(tracing::log::LevelFilter::Debug);

    let pool = PgPoolOptions::new()
        .max_connections(settings.max_connections.max(1))
        .min_connections(settings.min_connections.min(settings.max_connections))
        .acquire_timeout(settings.acquire_timeout())
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(options)
        .await
        .map_err(|e| {
            // Bypass `From<sqlx::Error>` to emit the specific event instead of
            // the generic `repository_error`.
            crate::observability::system::database_connection_failed(
                crate::observability::errors::sqlx_error_type(&e),
                &e.to_string(),
            );
            AppError::Database(e)
        })?;

    Ok(pool)
}

/// Result of a liveness probe.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Health {
    pub reachable: bool,
    pub latency_ms: u64,
    pub detail: Option<String>,
}

/// Round-trip a trivial query. Used by `/ready`.
pub async fn health(pool: &PgPool) -> Health {
    let started = std::time::Instant::now();
    match sqlx::query("SELECT 1").execute(pool).await {
        Ok(_) => Health {
            reachable: true,
            latency_ms: started.elapsed().as_millis() as u64,
            detail: None,
        },
        Err(e) => Health {
            reachable: false,
            latency_ms: started.elapsed().as_millis() as u64,
            detail: Some(short_db_error(&e)),
        },
    }
}

/// True when the error is a unique-constraint violation, used by the
/// idempotent insert paths to distinguish "already processed" from real errors.
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")
    )
}

/// Reduce a database error to something safe to expose over HTTP.
pub fn short_db_error(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Io(e) => format!("io error: {e}"),
        sqlx::Error::PoolTimedOut => "connection pool timed out".into(),
        sqlx::Error::PoolClosed => "connection pool closed".into(),
        sqlx::Error::Protocol(msg) => format!("protocol error: {msg}"),
        sqlx::Error::RowNotFound => "row not found".into(),
        sqlx::Error::Migrate(_) => "migration failure".into(),
        sqlx::Error::Database(db) => match db.code().as_deref() {
            Some("23505") => "duplicate key".into(),
            Some("23503") => "referenced record missing".into(),
            Some("23502") => "required field missing".into(),
            Some("57014") => "statement timeout".into(),
            _ => format!("database error {}", db.code().unwrap_or_default()),
        },
        other => format!("database error: {other}"),
    }
}

/// Apply a per-statement timeout to a connection.
///
/// Done once at startup so an accidentally expensive query in the API cannot
/// hold a pool slot indefinitely.
pub async fn apply_statement_timeout(pool: &PgPool, seconds: u64) -> Result<(), AppError> {
    if seconds == 0 {
        return Ok(());
    }
    sqlx::query(&format!("SET statement_timeout = {}00", seconds * 1000))
        .execute(pool)
        .await?;
    Ok(())
}
