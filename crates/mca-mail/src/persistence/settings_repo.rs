use sqlx::PgPool;

use crate::error::AppError;

/// Runtime settings that operators can change without a restart.
///
/// The typed configuration in `config` is the source of truth for anything the
/// process needs *in order to boot*. This table holds switches that are safe to
/// flip while running, which is what the admin panel edits.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct SettingRow {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub updated_by: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub const AUTOMATION_ENABLED: &str = "automation.global_enabled";
pub const RETENTION_ENABLED: &str = "retention.enabled";

pub async fn all(pool: &PgPool) -> Result<Vec<SettingRow>, AppError> {
    let rows = sqlx::query_as::<_, SettingRow>(
        "SELECT key, value, description, updated_by, updated_at FROM system_settings \
         ORDER BY key",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_bool(pool: &PgPool, key: &str) -> Result<Option<bool>, AppError> {
    let value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM system_settings WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(value.and_then(|v| v.as_bool()))
}

pub async fn get_i64(pool: &PgPool, key: &str) -> Result<Option<i64>, AppError> {
    let value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM system_settings WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(value.and_then(|v| v.as_i64()))
}

pub async fn set(
    pool: &PgPool,
    key: &str,
    value: serde_json::Value,
    updated_by: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO system_settings (key, value, updated_by) VALUES ($1,$2,$3) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, \
         updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .bind(updated_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Global kill switch for autonomous outbound mail, independent of the
/// configured mode. Read on every send decision.
pub async fn automation_enabled(pool: &PgPool) -> Result<bool, AppError> {
    Ok(get_bool(pool, AUTOMATION_ENABLED).await?.unwrap_or(true))
}

pub async fn set_automation_enabled(
    pool: &PgPool,
    enabled: bool,
    updated_by: &str,
) -> Result<(), AppError> {
    set(
        pool,
        AUTOMATION_ENABLED,
        serde_json::Value::Bool(enabled),
        updated_by,
    )
    .await
}
