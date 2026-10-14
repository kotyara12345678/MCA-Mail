//! Cookie sessions for the admin panel: one login/password, a random bearer
//! token stored only as a SHA-256 hash, so a database leak yields nothing
//! usable. The cookie itself is `HttpOnly` and `SameSite=Lax`, set by the
//! login route.

use chrono::{Duration, Utc};
use sqlx::PgPool;

use crate::error::AppError;

/// Cookie the panel authenticates with.
pub const COOKIE_NAME: &str = "mca_session";

/// How long a login stays valid without activity.
const SESSION_TTL_DAYS: i64 = 30;

/// Login identity recovered from a valid session token.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub login: String,
}

/// High-entropy random token for the `Set-Cookie` header.
pub fn generate_token() -> String {
    use rand::Rng;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill(&mut buf);
    hex::encode(buf)
}

/// SHA-256 of the presented token. Fast hash on purpose: tokens are
/// high-entropy, so a leak of this table cannot be replayed.
pub fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw.trim().as_bytes());
    hex::encode(hasher.finalize())
}

/// Cookie lifetime in seconds, mirroring [`SESSION_TTL_DAYS`].
pub fn cookie_max_age_seconds() -> i64 {
    SESSION_TTL_DAYS * 24 * 60 * 60
}

/// Create a session for `login` and return the raw token for the cookie.
pub async fn create(pool: &PgPool, login: &str) -> Result<String, AppError> {
    let raw = generate_token();
    let expires = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query("INSERT INTO admin_sessions (token_hash, login, expires_at) VALUES ($1, $2, $3)")
        .bind(hash_token(&raw))
        .bind(login)
        .bind(expires)
        .execute(pool)
        .await?;
    // Opportunistic sweep: one cheap DELETE per new login keeps the table
    // from accumulating dead rows without a background job.
    purge_expired(pool).await?;
    Ok(raw)
}

/// Validate a raw token, refreshing `last_seen_at` on success.
pub async fn verify(pool: &PgPool, raw: &str) -> Result<Option<SessionInfo>, AppError> {
    let login = sqlx::query_scalar::<_, String>(
        "UPDATE admin_sessions SET last_seen_at = now() \
         WHERE token_hash = $1 AND expires_at > now() \
         RETURNING login",
    )
    .bind(hash_token(raw))
    .fetch_optional(pool)
    .await?;
    Ok(login.map(|login| SessionInfo { login }))
}

/// Drop a session on logout; unknown tokens are a no-op.
pub async fn revoke(pool: &PgPool, raw: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM admin_sessions WHERE token_hash = $1")
        .bind(hash_token(raw))
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete expired sessions; called on every login (see [`create`]).
async fn purge_expired(pool: &PgPool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM admin_sessions WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(())
}
