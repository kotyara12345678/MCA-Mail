//! Read-side queries for API key management.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::{hash_key, Role};
use crate::error::AppError;

/// Row projection for the `api-key list` operator view.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KeySummary {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub prefix: String,
    pub is_active: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// All keys, newest first — revoked ones included so an operator can audit.
pub async fn list(pool: &PgPool) -> Result<Vec<KeySummary>, AppError> {
    let rows = sqlx::query_as::<_, KeySummary>(
        "SELECT id, name, role, prefix, is_active, created_by, created_at, \
         last_used_at, expires_at FROM api_keys ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Resolve the stored `prefix` (first 8 hex chars of the key hash) to a key id,
/// so `api-key revoke` can target a key without pasting the full UUID.
pub async fn id_by_prefix(pool: &PgPool, prefix: &str) -> Result<Option<Uuid>, AppError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM api_keys WHERE prefix = $1 AND is_active = TRUE",
    )
    .bind(prefix.trim())
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    role: String,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedKey {
    pub id: Uuid,
    pub role: Role,
}

/// Resolve a presented credential.
///
/// A missing, malformed, inactive or expired key all return `None`, so the API
/// layer cannot accidentally distinguish them for an attacker.
pub async fn verify(pool: &PgPool, raw: &str) -> Result<Option<VerifiedKey>, AppError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1 AND is_active = TRUE \
         AND (expires_at IS NULL OR expires_at > now()) RETURNING id, role, is_active, expires_at",
    )
    .bind(hash_key(raw))
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let role = match row.role.parse::<Role>() {
        Ok(role) => role,
        Err(_) => return Ok(None),
    };
    Ok(Some(VerifiedKey { id: row.id, role }))
}

pub async fn count_active(pool: &PgPool) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM api_keys WHERE is_active = TRUE")
        .fetch_one(pool)
        .await?;
    Ok(total)
}
