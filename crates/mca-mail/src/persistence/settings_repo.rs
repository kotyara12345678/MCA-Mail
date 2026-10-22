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

/// Pipeline switches the admin panel flips without a restart. Read on every
/// worker tick; `true` means the loop skips its work for that round.
pub const POLL_PAUSED: &str = "pipeline.poll_paused";
pub const QUEUE_PAUSED: &str = "pipeline.queue_paused";
pub const OUTBOX_PAUSED: &str = "pipeline.outbox_paused";

/// Agent autonomy mode (`dry_run` / `review` / `auto`). Absent means "use the
/// configured `EMAIL_MODE`"; present, it overrides the boot configuration, so
/// the panel can change how much the agent may do without a restart.
pub const AGENT_MODE: &str = "agent.mode";

/// Read a pause switch; any error (or an unset key) means "running", because
/// a broken settings table must not silently stop the mail flow.
pub async fn is_paused(pool: &PgPool, key: &str) -> bool {
    match get_bool(pool, key).await {
        Ok(flag) => flag.unwrap_or(false),
        Err(e) => {
            tracing::warn!(error = %e, key, "failed to read pause switch; treating as running");
            false
        }
    }
}

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

/// Read the runtime agent-mode override. `Ok(None)` means "not set, fall back
/// to the configured `EMAIL_MODE`". A stored value that no longer parses logs
/// a warning and also falls back, so a hand-edited row cannot brick the
/// pipeline.
pub async fn get_agent_mode(pool: &PgPool) -> Result<Option<crate::config::EmailMode>, AppError> {
    let value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM system_settings WHERE key = $1",
    )
    .bind(AGENT_MODE)
    .fetch_optional(pool)
    .await?;
    match value {
        None => Ok(None),
        Some(v) => match v.as_str().and_then(crate::config::EmailMode::parse) {
            Some(mode) => Ok(Some(mode)),
            None => {
                tracing::warn!(value = %v, "invalid stored agent.mode; falling back to config");
                Ok(None)
            }
        },
    }
}

pub async fn set_agent_mode(
    pool: &PgPool,
    mode: crate::config::EmailMode,
    updated_by: &str,
) -> Result<(), AppError> {
    set(
        pool,
        AGENT_MODE,
        serde_json::Value::String(mode.as_str().to_string()),
        updated_by,
    )
    .await
}

/// The mode the system is actually running under right now: the stored
/// override if present, otherwise the boot configuration. Every send-side
/// decision reads through here so the panel's switch takes effect on the next
/// tick rather than after a restart.
pub async fn effective_email_mode(
    pool: &PgPool,
    default: crate::config::EmailMode,
) -> crate::config::EmailMode {
    match get_agent_mode(pool).await {
        Ok(Some(mode)) => mode,
        Ok(None) => default,
        Err(e) => {
            tracing::warn!(error = %e, "failed to read agent.mode; using configured mode");
            default
        }
    }
}
