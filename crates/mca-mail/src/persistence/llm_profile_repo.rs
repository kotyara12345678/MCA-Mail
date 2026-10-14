//! Runtime-switchable LLM profiles, edited from the admin panel.
//!
//! The environment configuration stays the bootstrap source of truth; a
//! profile here overrides model, endpoint and pricing for as long as it is
//! active, with no restart. The API key is stored only so requests can be
//! signed — list views return a prefix, never the value.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

const SUMMARY_COLUMNS: &str = "id, name, base_url, model, \
     price_prompt_per_million_usd, price_completion_per_million_usd, \
     is_active, left(api_key, 8) AS key_prefix, created_at, updated_at";

/// Row shape for list views: the key is reduced to a prefix.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ProfileSummary {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub price_prompt_per_million_usd: f64,
    pub price_completion_per_million_usd: f64,
    pub is_active: bool,
    pub key_prefix: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Full row, including the key. Never leaves the process: the provider reads
/// it and the update route accepts a replacement.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProfileFull {
    pub id: Uuid,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub price_prompt_per_million_usd: f64,
    pub price_completion_per_million_usd: f64,
    pub is_active: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Payload for creating a profile.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewProfile {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub price_prompt_per_million_usd: f64,
    #[serde(default)]
    pub price_completion_per_million_usd: f64,
}

/// Payload for editing; `None` keeps the current value, an empty `api_key`
/// keeps the stored key.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProfilePatch {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub price_prompt_per_million_usd: Option<f64>,
    pub price_completion_per_million_usd: Option<f64>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<ProfileSummary>, AppError> {
    let rows = sqlx::query_as::<_, ProfileSummary>(&format!(
        "SELECT {SUMMARY_COLUMNS} FROM llm_profiles ORDER BY is_active DESC, created_at"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<ProfileFull>, AppError> {
    let row = sqlx::query_as::<_, ProfileFull>(
        "SELECT id, name, base_url, api_key, model, price_prompt_per_million_usd, \
         price_completion_per_million_usd, is_active, updated_at \
         FROM llm_profiles WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The profile every call uses while it exists; `None` falls back to the
/// environment configuration.
pub async fn active(pool: &PgPool) -> Result<Option<ProfileFull>, AppError> {
    let row = sqlx::query_as::<_, ProfileFull>(
        "SELECT id, name, base_url, api_key, model, price_prompt_per_million_usd, \
         price_completion_per_million_usd, is_active, updated_at \
         FROM llm_profiles WHERE is_active LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create(pool: &PgPool, new: &NewProfile) -> Result<Uuid, AppError> {
    validate(&new.name, &new.base_url, &new.model)?;
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO llm_profiles \
         (name, base_url, api_key, model, price_prompt_per_million_usd, \
          price_completion_per_million_usd) \
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(new.name.trim())
    .bind(new.base_url.trim())
    .bind(new.api_key.trim())
    .bind(new.model.trim())
    .bind(new.price_prompt_per_million_usd.max(0.0))
    .bind(new.price_completion_per_million_usd.max(0.0))
    .fetch_one(pool)
    .await
    .map_err(|e| unique_or_db(e, "profile name already exists"))?;
    Ok(inserted)
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    patch: ProfilePatch,
) -> Result<Option<ProfileFull>, AppError> {
    // Read-merge-write keeps the "None = unchanged" contract in one place;
    // a concurrent edit loses to the last writer, which is the intent for a
    // panel with a single operator.
    let Some(current) = get(pool, id).await? else {
        return Ok(None);
    };
    let name = patch.name.unwrap_or(current.name);
    let base_url = patch.base_url.unwrap_or(current.base_url);
    let model = patch.model.unwrap_or(current.model);
    let api_key = match patch.api_key.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => current.api_key,
    };
    let prompt = patch
        .price_prompt_per_million_usd
        .unwrap_or(current.price_prompt_per_million_usd)
        .max(0.0);
    let completion = patch
        .price_completion_per_million_usd
        .unwrap_or(current.price_completion_per_million_usd)
        .max(0.0);
    validate(&name, &base_url, &model)?;

    sqlx::query(
        "UPDATE llm_profiles SET name = $2, base_url = $3, api_key = $4, model = $5, \
         price_prompt_per_million_usd = $6, price_completion_per_million_usd = $7, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name.trim())
    .bind(base_url.trim())
    .bind(api_key)
    .bind(model.trim())
    .bind(prompt)
    .bind(completion)
    .execute(pool)
    .await
    .map_err(|e| unique_or_db(e, "profile name already exists"))?;
    get(pool, id).await
}

pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let done = sqlx::query("DELETE FROM llm_profiles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}

/// Make `id` the single active profile (or, with `None`, clear the
/// selection). One statement, so the unique index arbitrates concurrency.
/// `id = NULL` would be NULL, not FALSE — the guard keeps the assignment
/// boolean even when clearing.
pub async fn set_active(pool: &PgPool, id: Option<Uuid>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE llm_profiles SET is_active = ($1::uuid IS NOT NULL AND id = $1), \
         updated_at = now()",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

fn validate(name: &str, base_url: &str, model: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::InvalidInput("profile name is empty".into()));
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(AppError::InvalidInput(
            "base URL must start with http:// or https://".into(),
        ));
    }
    if model.trim().is_empty() {
        return Err(AppError::InvalidInput("model is empty".into()));
    }
    Ok(())
}

/// `llm_profiles.name` is UNIQUE: a duplicate reads as a conflict in the
/// panel, not as a 500.
fn unique_or_db(error: sqlx::Error, message: &'static str) -> AppError {
    if crate::persistence::is_unique_violation(&error) {
        AppError::Conflict(message.into())
    } else {
        AppError::Database(error)
    }
}
