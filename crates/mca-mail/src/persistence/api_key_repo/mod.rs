use sha2::Digest;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

mod query;
pub use query::{count_active, id_by_prefix, list, verify, KeySummary, VerifiedKey};

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

#[cfg(test)]
#[path = "api_key_repo_test.rs"]
mod tests;
