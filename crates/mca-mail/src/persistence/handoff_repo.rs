use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{Handoff, HandoffId, HandoffReason, HandoffState, Priority};
use crate::error::AppError;

#[derive(Debug, sqlx::FromRow)]
struct HandoffRow {
    id: Uuid,
    lead_id: Uuid,
    thread_id: Option<Uuid>,
    run_id: Option<Uuid>,
    email_id: Option<Uuid>,
    reason: String,
    priority: i32,
    state: String,
    contact_email: String,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    company_name: Option<String>,
    company_inn: Option<String>,
    original_request: String,
    category: Option<String>,
    spam_verdict: Option<String>,
    cargo_summary: String,
    route_summary: String,
    requested_service: String,
    missing_information: serde_json::Value,
    open_questions: serde_json::Value,
    conversation_digest: String,
    research_digest: Option<String>,
    checks_performed: serde_json::Value,
    unresolved_topics: serde_json::Value,
    assigned_to: Option<String>,
    acknowledged_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}

const COLUMNS: &str = "id, lead_id, thread_id, run_id, email_id, reason, priority, state, \
     contact_email, contact_name, contact_phone, company_name, company_inn, original_request, \
     category, spam_verdict, cargo_summary, route_summary, requested_service, \
     missing_information, open_questions, conversation_digest, research_digest, \
     checks_performed, unresolved_topics, assigned_to, acknowledged_at, created_at";

fn string_list(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn to_domain(row: HandoffRow) -> Result<Handoff, AppError> {
    let reason: HandoffReason = row
        .reason
        .parse()
        .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?;
    let state: HandoffState = row
        .state
        .parse()
        .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?;
    let priority = Priority::from_i32(row.priority);
    Ok(Handoff {
        id: row.id,
        lead_id: row.lead_id,
        thread_id: row.thread_id,
        run_id: row.run_id,
        email_id: row.email_id,
        reason,
        priority,
        state,
        contact_email: row.contact_email,
        contact_name: row.contact_name,
        contact_phone: row.contact_phone,
        company_name: row.company_name,
        company_inn: row.company_inn,
        original_request: row.original_request,
        category: row
            .category
            .as_deref()
            .map(|v| v.parse::<crate::domain::EmailCategory>())
            .transpose()
            .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?,
        spam_verdict: row
            .spam_verdict
            .as_deref()
            .map(|v| v.parse::<crate::domain::SpamVerdict>())
            .transpose()
            .map_err(|e: crate::domain::WireParseError| sqlx::Error::Decode(Box::new(e)))?,
        cargo_summary: row.cargo_summary,
        route_summary: row.route_summary,
        requested_service: row.requested_service,
        missing_information: string_list(&row.missing_information),
        open_questions: string_list(&row.open_questions),
        conversation_digest: row.conversation_digest,
        research_digest: row.research_digest,
        checks_performed: string_list(&row.checks_performed),
        unresolved_topics: string_list(&row.unresolved_topics),
        assigned_to: row.assigned_to,
        acknowledged_at: row.acknowledged_at,
        created_at: row.created_at,
    })
}

/// Create or update the open handoff for a lead.
///
/// A partial unique index allows only one open handoff per lead: a repeated
/// escalation updates the existing row instead of spamming the manager queue.
pub async fn upsert_open(pool: &PgPool, handoff: &Handoff) -> Result<HandoffId, AppError> {
    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM human_handoffs WHERE lead_id = $1 AND state IN ('open','acknowledged')",
    )
    .bind(handoff.lead_id)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        sqlx::query(
            "UPDATE human_handoffs SET reason = $2, priority = GREATEST(priority, $3), \
             original_request = $4, cargo_summary = $5, route_summary = $6, \
             requested_service = $7, missing_information = $8, open_questions = $9, \
             conversation_digest = $10, research_digest = COALESCE($11, research_digest), \
             checks_performed = $12, unresolved_topics = $13, contact_phone = \
             COALESCE($14, contact_phone) WHERE id = $1",
        )
        .bind(id)
        .bind(handoff.reason.as_str())
        .bind(handoff.priority.as_i32())
        .bind(&handoff.original_request)
        .bind(&handoff.cargo_summary)
        .bind(&handoff.route_summary)
        .bind(&handoff.requested_service)
        .bind(json_list(&handoff.missing_information))
        .bind(json_list(&handoff.open_questions))
        .bind(&handoff.conversation_digest)
        .bind(&handoff.research_digest)
        .bind(json_list(&handoff.checks_performed))
        .bind(json_list(&handoff.unresolved_topics))
        .bind(&handoff.contact_phone)
        .execute(pool)
        .await?;
        return Ok(id);
    }
    let sql =
        "INSERT INTO human_handoffs (lead_id, thread_id, run_id, email_id, reason, priority, \
         state, contact_email, contact_name, contact_phone, company_name, company_inn, \
         original_request, category, spam_verdict, cargo_summary, route_summary, \
         requested_service, missing_information, open_questions, conversation_digest, \
         research_digest, checks_performed, unresolved_topics) \
         VALUES ($1,$2,$3,$4,$5,$6,'open',$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,\
                 $20,$21,$22,$23) RETURNING id"
            .to_string();
    let id = sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(handoff.lead_id)
        .bind(handoff.thread_id)
        .bind(handoff.run_id)
        .bind(handoff.email_id)
        .bind(handoff.reason.as_str())
        .bind(handoff.priority.as_i32())
        .bind(&handoff.contact_email)
        .bind(&handoff.contact_name)
        .bind(&handoff.contact_phone)
        .bind(&handoff.company_name)
        .bind(&handoff.company_inn)
        .bind(&handoff.original_request)
        .bind(handoff.category.map(|c| c.as_str().to_string()))
        .bind(handoff.spam_verdict.map(|s| s.as_str().to_string()))
        .bind(&handoff.cargo_summary)
        .bind(&handoff.route_summary)
        .bind(&handoff.requested_service)
        .bind(json_list(&handoff.missing_information))
        .bind(json_list(&handoff.open_questions))
        .bind(&handoff.conversation_digest)
        .bind(&handoff.research_digest)
        .bind(json_list(&handoff.checks_performed))
        .bind(json_list(&handoff.unresolved_topics))
        .fetch_one(pool)
        .await?;
    Ok(id)
}

/// The list columns are `JSONB` arrays, not SQL arrays, so bind as JSON.
fn json_list(values: &[String]) -> serde_json::Value {
    serde_json::to_value(values).unwrap_or(serde_json::Value::Array(Vec::new()))
}

pub async fn get(pool: &PgPool, id: HandoffId) -> Result<Option<Handoff>, AppError> {
    let row = sqlx::query_as::<_, HandoffRow>(&format!(
        "SELECT {COLUMNS} FROM human_handoffs WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

pub async fn open_for_lead(pool: &PgPool, lead_id: Uuid) -> Result<Option<Handoff>, AppError> {
    let row = sqlx::query_as::<_, HandoffRow>(&format!(
        "SELECT {COLUMNS} FROM human_handoffs WHERE lead_id = $1 \
         AND state IN ('open','acknowledged') LIMIT 1"
    ))
    .bind(lead_id)
    .fetch_optional(pool)
    .await?;
    row.map(to_domain).transpose()
}

pub async fn acknowledge(pool: &PgPool, id: HandoffId, assigned_to: &str) -> Result<(), AppError> {
    let done = sqlx::query(
        "UPDATE human_handoffs SET state = 'acknowledged', assigned_to = $2, \
         acknowledged_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(assigned_to)
    .execute(pool)
    .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("handoff {id} not found")));
    }
    Ok(())
}

pub async fn resolve(
    pool: &PgPool,
    id: HandoffId,
    back_to_automation: bool,
) -> Result<(), AppError> {
    let state = if back_to_automation {
        "returned_to_automation"
    } else {
        "resolved"
    };
    let done = sqlx::query("UPDATE human_handoffs SET state = $2 WHERE id = $1")
        .bind(id)
        .bind(state)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("handoff {id} not found")));
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct HandoffFilter {
    pub state: Option<HandoffState>,
    pub priority_min: Option<i32>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn list(pool: &PgPool, filter: &HandoffFilter) -> Result<Vec<Handoff>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM human_handoffs \
         WHERE ($1::text IS NULL OR state = $1) AND ($2::int IS NULL OR priority >= $2) \
         ORDER BY priority DESC, created_at ASC LIMIT $3 OFFSET $4"
    );
    let rows = sqlx::query_as::<_, HandoffRow>(&sql)
        .bind(filter.state.map(|s| s.as_str().to_string()))
        .bind(filter.priority_min)
        .bind(filter.limit.clamp(1, 200))
        .bind(filter.offset.max(0))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(to_domain).collect()
}

/// Rendered text for the manager notification channel.
pub fn render(handoff: &Handoff) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", handoff.headline()));
    out.push_str(&format!(
        "Контакт: {} {}\n",
        handoff.contact_email,
        handoff.contact_name.as_deref().unwrap_or("")
    ));
    if let Some(phone) = &handoff.contact_phone {
        out.push_str(&format!("Телефон: {phone}\n"));
    }
    if let Some(inn) = &handoff.company_inn {
        out.push_str(&format!("ИНН: {inn}\n"));
    }
    out.push_str(&format!("Услуга: {}\n", handoff.requested_service));
    out.push_str(&format!("Груз: {}\n", handoff.cargo_summary));
    out.push_str(&format!("Маршрут: {}\n", handoff.route_summary));
    if !handoff.missing_information.is_empty() {
        out.push_str(&format!(
            "Не хватает данных: {}\n",
            handoff.missing_information.join(", ")
        ));
    }
    if !handoff.unresolved_topics.is_empty() {
        out.push_str(&format!(
            "Требует проверки: {}\n",
            handoff.unresolved_topics.join(", ")
        ));
    }
    if !handoff.checks_performed.is_empty() {
        out.push_str(&format!(
            "Проверки: {}\n",
            handoff.checks_performed.join(", ")
        ));
    }
    if let Some(research) = &handoff.research_digest {
        out.push_str(&format!("Проверка компании:\n{research}\n"));
    }
    if !handoff.conversation_digest.is_empty() {
        out.push_str(&format!("Переписка:\n{}\n", handoff.conversation_digest));
    }
    if !handoff.open_questions.is_empty() {
        out.push_str(&format!(
            "Открытые вопросы:\n- {}\n",
            handoff.open_questions.join("\n- ")
        ));
    }
    out
}
