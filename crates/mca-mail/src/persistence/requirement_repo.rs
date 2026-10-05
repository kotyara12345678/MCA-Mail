use sqlx::PgPool;
use uuid::Uuid;

use super::parse_enum;
use crate::domain::{FieldState, LeadId, LeadRequirement, RequirementField, RequirementSource};
use crate::error::AppError;

#[derive(Debug, sqlx::FromRow)]
struct RequirementRow {
    lead_id: Uuid,
    field: String,
    value: Option<String>,
    state: String,
    source: String,
    unit: Option<String>,
    confidence: Option<f32>,
    evidence: Option<String>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn to_domain(row: RequirementRow) -> Result<LeadRequirement, AppError> {
    Ok(LeadRequirement {
        lead_id: row.lead_id,
        field: parse_enum::<RequirementField>(&row.field, "lead_requirements.field")?,
        value: row.value,
        state: parse_enum::<FieldState>(&row.state, "lead_requirements.state")?,
        source: parse_enum::<RequirementSource>(&row.source, "lead_requirements.source")?,
        unit: row.unit,
        confidence: row.confidence,
        evidence: row.evidence,
        updated_at: row.updated_at,
    })
}

const COLUMNS: &str = "lead_id, field, value, state, source, unit, confidence, evidence, \
     updated_at";

pub async fn all(pool: &PgPool, lead_id: LeadId) -> Result<Vec<LeadRequirement>, AppError> {
    let rows = sqlx::query_as::<_, RequirementRow>(&format!(
        "SELECT {COLUMNS} FROM lead_requirements WHERE lead_id = $1"
    ))
    .bind(lead_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(to_domain).collect()
}

/// Upsert a batch of requirements in one transaction.
///
/// A value the customer stated (`known`) never gets overwritten by an inference
/// (`needs_confirmation`), and a manager's value always wins: this ordering is
/// what keeps the CRM honest about where each fact came from. The same rule is
/// applied to `evidence`, so a kept value never loses the span that justifies
/// it. Re-running the same batch is a no-op, which is what makes a replayed
/// worker safe.
pub async fn upsert_many(
    pool: &PgPool,
    lead_id: LeadId,
    requirements: &[LeadRequirement],
) -> Result<u64, AppError> {
    if requirements.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut changed = 0u64;
    for r in requirements {
        let rows = sqlx::query(
            "INSERT INTO lead_requirements (lead_id, field, value, state, source, unit, \
             confidence, evidence) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT (lead_id, field) DO UPDATE SET \
               value = CASE WHEN EXCLUDED.source = 'customer' THEN EXCLUDED.value \
                             WHEN lead_requirements.source IN ('customer','manager') \
                                  AND EXCLUDED.state <> 'not_applicable' \
                             THEN lead_requirements.value ELSE EXCLUDED.value END, \
               state = CASE WHEN lead_requirements.source IN ('customer','manager') \
                                  AND EXCLUDED.source = 'ai_inference' \
                             THEN lead_requirements.state ELSE EXCLUDED.state END, \
               source = CASE WHEN lead_requirements.source IN ('customer','manager') \
                                  AND EXCLUDED.source = 'ai_inference' \
                             THEN lead_requirements.source ELSE EXCLUDED.source END, \
               unit = COALESCE(EXCLUDED.unit, lead_requirements.unit), \
               confidence = COALESCE(EXCLUDED.confidence, lead_requirements.confidence), \
               evidence = CASE WHEN EXCLUDED.source = 'customer' THEN EXCLUDED.evidence \
                               WHEN lead_requirements.source IN ('customer','manager') \
                                    AND EXCLUDED.state <> 'not_applicable' \
                               THEN lead_requirements.evidence ELSE EXCLUDED.evidence END, \
               updated_at = now()",
        )
        .bind(lead_id)
        .bind(r.field.as_str())
        .bind(&r.value)
        .bind(r.state.as_str())
        .bind(r.source.as_str())
        .bind(&r.unit)
        .bind(r.confidence)
        .bind(&r.evidence)
        .execute(&mut *tx)
        .await
        .map_err(|e| tx_failure("upsert_many", e))?;
        changed += rows.rows_affected();
    }
    tx.commit()
        .await
        .map_err(|e| tx_failure("upsert_many", e))?;
    Ok(changed)
}

/// Emit `transaction_failed` and wrap the sqlx error without double-logging
/// through the central `From<sqlx::Error>` hook.
fn tx_failure(operation: &str, e: sqlx::Error) -> AppError {
    let error_type = crate::observability::errors::sqlx_error_type(&e);
    let text = e.to_string();
    crate::observability::system::transaction_failed(operation, error_type, &text);
    AppError::Database(e)
}

/// Mark fields outside the requested service scope as not applicable, so the
/// agent stops asking for information the customer never intended to provide.
pub async fn mark_not_applicable(
    pool: &PgPool,
    lead_id: LeadId,
    fields: &[RequirementField],
) -> Result<u64, AppError> {
    if fields.is_empty() {
        return Ok(0);
    }
    let names: Vec<String> = fields.iter().map(|f| f.as_str().to_string()).collect();
    let done = sqlx::query(
        "INSERT INTO lead_requirements (lead_id, field, state, source) \
         SELECT $1, unnest($2::text[]), 'not_applicable', 'ai_inference' \
         ON CONFLICT (lead_id, field) DO UPDATE SET state = 'not_applicable' \
         WHERE lead_requirements.source <> 'customer'",
    )
    .bind(lead_id)
    .bind(&names)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// Mark fields that are still missing as `unknown`, so the CRM always has one
/// row per field and a manager can see the full picture.
pub async fn seed_missing(
    pool: &PgPool,
    lead_id: LeadId,
    fields: &[RequirementField],
) -> Result<u64, AppError> {
    if fields.is_empty() {
        return Ok(0);
    }
    let names: Vec<String> = fields.iter().map(|f| f.as_str().to_string()).collect();
    let done = sqlx::query(
        "INSERT INTO lead_requirements (lead_id, field, state, source) \
         SELECT $1, unnest($2::text[]), 'unknown', 'ai_inference' \
         ON CONFLICT (lead_id, field) DO NOTHING",
    )
    .bind(lead_id)
    .bind(&names)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

pub async fn get_field(
    pool: &PgPool,
    lead_id: LeadId,
    field: RequirementField,
) -> Result<Option<LeadRequirement>, AppError> {
    let row = sqlx::query_as::<_, RequirementRow>(&format!(
        "SELECT {COLUMNS} FROM lead_requirements WHERE lead_id = $1 AND field = $2"
    ))
    .bind(lead_id)
    .bind(field.as_str())
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

/// Record a value entered by a manager through the API.
pub async fn set_by_manager(
    pool: &PgPool,
    lead_id: LeadId,
    field: RequirementField,
    value: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO lead_requirements (lead_id, field, value, state, source, confidence) \
         VALUES ($1,$2,$3,'known','manager',1.0) \
         ON CONFLICT (lead_id, field) DO UPDATE SET value = EXCLUDED.value, \
         state = 'known', source = 'manager', confidence = 1.0",
    )
    .bind(lead_id)
    .bind(field.as_str())
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}
