use sha2::Digest;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

crate::domain::wire_enum! {
    /// Role attached to an API credential. Order matters: `at_least` compares
    /// the numeric rank, so a new role must be inserted at the right position.
    Role {
        Viewer => "viewer",
        Operator => "operator",
        Manager => "manager",
        Admin => "admin",
    }
}

impl Role {
    pub const fn rank(&self) -> u8 {
        match self {
            Role::Viewer => 0,
            Role::Operator => 1,
            Role::Manager => 2,
            Role::Admin => 3,
        }
    }

    pub const fn at_least(&self, required: Role) -> bool {
        self.rank() >= required.rank()
    }
}

/// SHA-256 of the presented key. API keys are high-entropy random strings, so a
/// fast hash is appropriate; the point is that a database leak does not yield
/// usable credentials.
pub fn hash_key(raw: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(raw.trim().as_bytes());
    hex::encode(hasher.finalize())
}

/// First 8 characters of the hash, shown in the UI so an operator can identify
/// a key without storing it.
pub fn key_prefix(raw: &str) -> String {
    hash_key(raw).chars().take(8).collect()
}

/// Generate a new API key in the `mca_<32 hex>` format.
pub fn generate_key() -> String {
    use rand::Rng;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill(&mut buf);
    format!("mca_{}", hex::encode(buf))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    role: String,
    is_active: bool,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
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

pub async fn create(
    pool: &PgPool,
    name: &str,
    role: Role,
    created_by: &str,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(String, Uuid), AppError> {
    let raw = generate_key();
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO api_keys (name, role, key_hash, prefix, created_by, expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(name)
    .bind(role.as_str())
    .bind(hash_key(&raw))
    .bind(key_prefix(&raw))
    .bind(created_by)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok((raw, id))
}

pub async fn revoke(pool: &PgPool, id: Uuid) -> Result<(), AppError> {
    let done = sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("api key {id} not found")));
    }
    Ok(())
}

pub async fn count_active(pool: &PgPool) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM api_keys WHERE is_active = TRUE")
        .fetch_one(pool)
        .await?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ranking_is_ordered() {
        assert!(Role::Admin.at_least(Role::Viewer));
        assert!(Role::Manager.at_least(Role::Operator));
        assert!(!Role::Operator.at_least(Role::Manager));
        assert!(!Role::Viewer.at_least(Role::Operator));
    }

    #[test]
    fn keys_are_prefixed_and_hashed_deterministically() {
        let key = generate_key();
        assert!(key.starts_with("mca_"));
        assert_eq!(hash_key(&key), hash_key(&key));
        assert_ne!(hash_key(&key), hash_key(&generate_key()));
        assert_eq!(key_prefix(&key).len(), 8);
        assert!(!hash_key(&key).contains(&key));
    }
}
