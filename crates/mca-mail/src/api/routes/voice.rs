//! Voice-channel API endpoints.
//!
//! Contract with the call agent: every verb maps one tool call to one CDP-safe
//! action and returns enough state for the agent to decide the next question.
//! Server-side authority lives in `crate::voice`; these handlers are thin —
//! they validate, call a repository, and shape the JSON reply.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::ApiState;
use crate::domain::{HandoffReason, LeadStatus, Priority, RequirementField};
use crate::error::AppError;
use crate::persistence::api_key_repo::Role;
use crate::persistence::audit_repo::{self, AuditEntry, Outcome as AuditOutcome};
use crate::persistence::{handoff_repo, lead_repo, requirement_repo, voice_repo};
use crate::voice::{self, handoff, order_card, phone, InputRequirement};

// ---------------------------------------------------------------------------
// wire shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateLeadBody {
    caller_number: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct UpdateLeadBody {
    #[serde(default)]
    company_name: Option<String>,
    #[serde(default)]
    company_inn: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    /// Email dictated during the call. Replaces the caller number the voice
    /// lead starts with; without it no order card can ever be addressed.
    #[serde(default)]
    contact_email: Option<String>,
}

#[derive(Deserialize)]
struct SaveRequirementsBody {
    requirements: Vec<InputRequirement>,
}

#[derive(Deserialize)]
struct AppendMessageBody {
    direction: String,
    body: String,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Deserialize)]
struct QualifyBody {}

#[derive(Deserialize)]
struct HandoffBody {
    reason: String,
    #[serde(default)]
    priority: String,
    manager_key: String,
    #[serde(default)]
    original_request: Option<String>,
}

#[derive(Deserialize)]
struct TranscriptLine {
    /// AVA's final transcript uses OpenAI-style `role`/`content` keys;
    /// the MCA wire names (`direction`/`body`) stay canonical.
    #[serde(alias = "role")]
    direction: String,
    #[serde(alias = "content")]
    body: String,
}

#[derive(Deserialize)]
struct FinishCallBody {
    lead_id: Uuid,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    transcript: Vec<TranscriptLine>,
}

#[derive(Deserialize)]
struct LookupQuery {
    phone: String,
}

#[derive(Serialize)]
struct CreatedLead {
    lead_id: Uuid,
    status: String,
    scope: String,
    created: bool,
}

#[derive(Serialize)]
struct GapItem {
    field: String,
    label: String,
}

#[derive(Serialize)]
struct RequirementItem {
    field: String,
    label: String,
    value: Option<String>,
    unit: Option<String>,
    state: String,
    source: String,
    is_blocking: bool,
}

#[derive(Serialize)]
struct RequirementsView {
    requirements: Vec<RequirementItem>,
    blocking_gaps: Vec<GapItem>,
    open_gaps: Vec<GapItem>,
    /// Questions the agent already asked this caller, so the model can skip
    /// closed ones instead of asking the customer twice for the same fact.
    asked_questions: Vec<voice::AskedQuestion>,
}

#[derive(Serialize)]
struct SaveResponse {
    saved: usize,
    blocking_gaps: Vec<GapItem>,
}

#[derive(Serialize)]
struct MessageLine {
    direction: String,
    body: String,
    created_at: String,
}

#[derive(Serialize)]
struct ConversationView {
    messages: Vec<MessageLine>,
}

#[derive(Serialize)]
struct AppendResponse {
    message_id: Uuid,
}

#[derive(Serialize)]
struct QualifyResponse {
    status: String,
    qualified: bool,
    blocking_gaps: Vec<GapItem>,
}

#[derive(Serialize)]
struct HandoffResponse {
    handoff_id: Uuid,
    status: String,
    contact_email: String,
}

#[derive(Serialize)]
struct FinishCallResponse {
    lead_id: Uuid,
    status: String,
    order_card: OrderCardView,
}

/// What became of the customer's order card for this call. `reason` and
/// `missing_fields` explain a skip; `outbox_id` anchors a queued row so the
/// same card can be found in the outbox later. Queuing is not sending: the
/// send worker's policy check still decides delivery.
#[derive(Serialize)]
struct OrderCardView {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outbox_id: Option<Uuid>,
}

impl From<&order_card::OrderCardOutcome> for OrderCardView {
    fn from(outcome: &order_card::OrderCardOutcome) -> Self {
        match outcome {
            order_card::OrderCardOutcome::Queued { outbox_id } => Self {
                state: outcome.state().to_string(),
                reason: None,
                missing_fields: Vec::new(),
                outbox_id: Some(*outbox_id),
            },
            order_card::OrderCardOutcome::AlreadyQueued => Self {
                state: outcome.state().to_string(),
                reason: None,
                missing_fields: Vec::new(),
                outbox_id: None,
            },
            order_card::OrderCardOutcome::Skipped { reason } => Self {
                state: outcome.state().to_string(),
                reason: Some(reason.as_str().to_string()),
                missing_fields: reason.missing().iter().map(|f| (*f).to_string()).collect(),
                outbox_id: None,
            },
        }
    }
}

#[derive(Serialize)]
struct LookupResponse {
    found: bool,
    lead_id: Option<Uuid>,
    status: Option<String>,
    scope: Option<String>,
    company_name: Option<String>,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn gap_items(fields: &[RequirementField]) -> Vec<GapItem> {
    fields
        .iter()
        .map(|field| GapItem {
            field: field.as_str().to_string(),
            label: voice::fields::label(*field).to_string(),
        })
        .collect()
}

fn build_requirements_view(
    requirements: &[crate::domain::LeadRequirement],
    asked_questions: Vec<voice::AskedQuestion>,
) -> RequirementsView {
    let blocking = crate::domain::Lead::blocking_gaps(requirements);
    let open = crate::domain::Lead::open_gaps(requirements);
    let items = requirements
        .iter()
        .map(|r| RequirementItem {
            field: r.field.as_str().to_string(),
            label: voice::fields::label(r.field).to_string(),
            value: r.value.clone(),
            unit: r.unit.clone(),
            state: r.state.as_str().to_string(),
            source: r.source.as_str().to_string(),
            is_blocking: r.field.is_quote_blocking(),
        })
        .collect();
    RequirementsView {
        requirements: items,
        blocking_gaps: gap_items(&blocking),
        open_gaps: gap_items(&open),
        asked_questions,
    }
}

fn parse_direction(raw: &str) -> Result<crate::domain::ConversationDirection, AppError> {
    raw.parse()
        .map_err(|_| AppError::invalid(format!("unknown direction: {raw}")))
}

fn parse_reason(raw: &str) -> Result<HandoffReason, AppError> {
    raw.parse::<HandoffReason>()
        .map_err(|_: crate::domain::WireParseError| {
            AppError::invalid(format!("unknown reason: {raw}"))
        })
}

fn parse_priority(raw: &str) -> Result<Priority, AppError> {
    raw.parse::<Priority>()
        .map_err(|_: crate::domain::WireParseError| {
            AppError::invalid(format!("unknown priority: {raw}"))
        })
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/voice/leads` — get_or_create_lead.
async fn create_lead(
    State(state): State<Arc<ApiState>>,
    auth: AuthenticatedRequest,
    Json(body): Json<CreateLeadBody>,
) -> Result<Json<CreatedLead>, AppError> {
    auth.require(Role::Operator)?;
    let scope = body
        .scope
        .as_deref()
        .map(voice::parse_scope)
        .transpose()?
        .unwrap_or(voice::DEFAULT_SCOPE);
    let (lead_id, created) =
        voice_repo::ensure_voice_lead(&state.pool, &body.caller_number, scope).await?;
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    Ok(Json(CreatedLead {
        lead_id,
        status: lead.status.as_str().to_string(),
        scope: lead.scope.as_str().to_string(),
        created,
    }))
}

/// `PATCH /api/v1/voice/leads/{id}` — update identity and service scope.
async fn update_lead(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    auth: AuthenticatedRequest,
    Json(body): Json<UpdateLeadBody>,
) -> Result<Json<CreatedLead>, AppError> {
    auth.require(Role::Operator)?;
    // Validate everything before the first write, so a bad address cannot
    // leave a half-applied update behind.
    let contact_email = body
        .contact_email
        .as_deref()
        .map(voice::normalize_email)
        .transpose()?;
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    if lead.status.automation_locked() {
        return Err(AppError::Conflict(format!(
            "lead {lead_id} automation is locked"
        )));
    }
    if let Some(scope_raw) = body.scope {
        let scope = voice::parse_scope(&scope_raw)?;
        voice_repo::update_scope(&state.pool, lead_id, scope).await?;
    }
    lead_repo::update_identity(
        &state.pool,
        lead_id,
        body.company_name.as_deref(),
        body.company_inn.as_deref(),
    )
    .await?;
    if let Some(email) = &contact_email {
        lead_repo::set_contact_email(&state.pool, lead_id, email).await?;
    }
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    Ok(Json(CreatedLead {
        lead_id,
        status: lead.status.as_str().to_string(),
        scope: lead.scope.as_str().to_string(),
        created: false,
    }))
}

/// `GET /api/v1/voice/leads/{id}/requirements` — get_missing_requirements.
///
/// Also reports the questions already asked on this lead: the call agent
/// calls this at the start of every turn, and `asked_questions` is what lets
/// a resumed call continue instead of opening with the same greeting and the
/// same first question.
async fn get_requirements(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Result<Json<RequirementsView>, AppError> {
    let requirements = requirement_repo::all(&state.pool, lead_id).await?;
    let history = voice_repo::conversation(&state.pool, lead_id).await?;
    Ok(Json(build_requirements_view(
        &requirements,
        voice::asked_questions(&history),
    )))
}

/// `PUT /api/v1/voice/leads/{id}/requirements` — save_requirements.
async fn save_requirements(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    auth: AuthenticatedRequest,
    Json(body): Json<SaveRequirementsBody>,
) -> Result<Json<SaveResponse>, AppError> {
    auth.require(Role::Operator)?;
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    if lead.status.automation_locked() {
        return Err(AppError::Conflict(format!(
            "lead {lead_id} automation is locked"
        )));
    }
    let mut validated = Vec::with_capacity(body.requirements.len());
    for item in &body.requirements {
        validated.push(item.validate()?);
    }
    voice_repo::save_requirements(&state.pool, lead_id, &validated).await?;
    let requirements = requirement_repo::all(&state.pool, lead_id).await?;
    let blocking = crate::domain::Lead::blocking_gaps(&requirements);
    Ok(Json(SaveResponse {
        saved: validated.len(),
        blocking_gaps: gap_items(&blocking),
    }))
}

/// `GET /api/v1/voice/leads/{id}/conversation` — get_conversation_state.
async fn get_conversation(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    _auth: AuthenticatedRequest,
) -> Result<Json<ConversationView>, AppError> {
    let messages = voice_repo::conversation(&state.pool, lead_id)
        .await?
        .into_iter()
        .map(|entry| MessageLine {
            direction: entry.direction.as_str().to_string(),
            body: entry.body,
            created_at: entry.created_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(ConversationView { messages }))
}

/// `POST /api/v1/voice/leads/{id}/conversation` — append_conversation_message.
async fn append_conversation(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    auth: AuthenticatedRequest,
    Json(body): Json<AppendMessageBody>,
) -> Result<Json<AppendResponse>, AppError> {
    auth.require(Role::Operator)?;
    let direction = parse_direction(&body.direction)?;
    if body.body.trim().is_empty() {
        return Err(AppError::invalid("message body is empty"));
    }
    let key = body
        .idempotency_key
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let message_id =
        voice_repo::append_conversation(&state.pool, lead_id, direction, &body.body, &key).await?;
    Ok(Json(AppendResponse { message_id }))
}

/// `POST /api/v1/voice/leads/{id}/qualify` — server-side qualification.
async fn qualify(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    auth: AuthenticatedRequest,
    _body: Json<QualifyBody>,
) -> Result<Json<QualifyResponse>, AppError> {
    auth.require(Role::Operator)?;
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    if lead.status.automation_locked() {
        return Err(AppError::Conflict(format!(
            "lead {lead_id} automation is locked"
        )));
    }
    let requirements = requirement_repo::all(&state.pool, lead_id).await?;
    let blocking = crate::domain::Lead::blocking_gaps(&requirements);
    let status = if blocking.is_empty() {
        if lead.status != LeadStatus::Qualified {
            lead_repo::update_status(&state.pool, lead_id, LeadStatus::Qualified).await?;
        }
        LeadStatus::Qualified
    } else {
        if lead.status == LeadStatus::Qualified {
            lead_repo::update_status(&state.pool, lead_id, LeadStatus::AwaitingCustomer).await?;
        }
        LeadStatus::AwaitingCustomer
    };
    Ok(Json(QualifyResponse {
        status: status.as_str().to_string(),
        qualified: blocking.is_empty(),
        blocking_gaps: gap_items(&blocking),
    }))
}

/// `POST /api/v1/voice/leads/{id}/handoff` — request_manager_handoff.
async fn request_handoff(
    State(state): State<Arc<ApiState>>,
    Path(lead_id): Path<Uuid>,
    auth: AuthenticatedRequest,
    Json(body): Json<HandoffBody>,
) -> Result<Json<HandoffResponse>, AppError> {
    auth.require(Role::Operator)?;
    handoff::ensure_manager_key(&body.manager_key)?;
    let lead = lead_repo::get(&state.pool, lead_id).await?;
    if lead.status.automation_locked() {
        return Err(AppError::Conflict(format!(
            "lead {lead_id} automation is locked"
        )));
    }
    let reason = parse_reason(&body.reason)?;
    let priority = parse_priority(&body.priority)?;
    let requirements = requirement_repo::all(&state.pool, lead_id).await?;
    let conversation = voice_repo::conversation(&state.pool, lead_id).await?;
    let original_request = body.original_request.unwrap_or_default();
    let handoff = handoff::build_handoff(
        &lead,
        &requirements,
        &conversation,
        reason,
        priority,
        &original_request,
    );
    let handoff_id = handoff_repo::upsert_open(&state.pool, &handoff).await?;
    lead_repo::lock_automation(&state.pool, lead_id).await?;
    Ok(Json(HandoffResponse {
        handoff_id,
        status: LeadStatus::HandedOff.as_str().to_string(),
        contact_email: lead.contact_email,
    }))
}

/// Map a finished-call transcript line to a conversation direction.
///
/// Accepts the MCA wire values (`inbound`/`outbound`) and the role names the
/// call agent (AVA) emits in its final transcript (`user`/`assistant`);
/// service lines (tool calls, system) have no speaker and are skipped so one
/// malformed entry cannot drop the whole recording.
fn transcript_direction(raw: &str) -> Option<crate::domain::ConversationDirection> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "inbound" | "user" | "caller" | "customer" => {
            Some(crate::domain::ConversationDirection::Inbound)
        }
        "outbound" | "assistant" | "agent" | "ai" => {
            Some(crate::domain::ConversationDirection::Outbound)
        }
        _ => None,
    }
}

/// `POST /api/v1/voice/calls/finish` — outcome transcript permanently recorded.
async fn finish_call(
    State(state): State<Arc<ApiState>>,
    auth: AuthenticatedRequest,
    Json(body): Json<FinishCallBody>,
) -> Result<Json<FinishCallResponse>, AppError> {
    auth.require(Role::Operator)?;
    let lead = lead_repo::get(&state.pool, body.lead_id).await?;
    let call_key = body.call_id.as_deref().unwrap_or("call");
    for (index, line) in body.transcript.iter().enumerate() {
        let Some(direction) = transcript_direction(&line.direction) else {
            continue;
        };
        if line.body.trim().is_empty() {
            continue;
        }
        let key = format!("voice:{call_key}:{index}");
        voice_repo::append_conversation(&state.pool, body.lead_id, direction, &line.body, &key)
            .await?;
    }
    if let Some(summary) = body.summary {
        if !summary.trim().is_empty() {
            lead_repo::update_summary(
                &state.pool,
                body.lead_id,
                &summary,
                &lead.open_questions,
                &lead.unresolved_topics,
                lead.confidence.unwrap_or(0.0),
            )
            .await?;
        }
    }
    let status = lead_repo::get(&state.pool, body.lead_id).await?.status;
    // The card follows the transcript and the summary: it renders whatever
    // this call (and earlier ones) left stored. A skip must be auditable —
    // "why is there no email for this lead" has a durable answer here.
    let card = order_card::enqueue(&state.pool, body.lead_id).await?;
    let (audit_outcome, details) = match &card {
        order_card::OrderCardOutcome::Skipped { reason } => (
            AuditOutcome::Denied,
            serde_json::json!({
                "reason": reason.as_str(),
                "missing": reason.missing(),
            }),
        ),
        other => (
            AuditOutcome::Success,
            serde_json::json!({ "state": other.state() }),
        ),
    };
    let lead_key = body.lead_id.to_string();
    let entry = AuditEntry::new(
        &auth.api_key_prefix,
        &auth.role,
        "voice_order_card",
        "lead",
        audit_outcome,
    )
    .resource(&lead_key)
    .details(details);
    if let Err(e) = audit_repo::record(&state.pool, &entry).await {
        tracing::warn!(error = %e, "order card outcome not written to audit log");
    }
    Ok(Json(FinishCallResponse {
        lead_id: body.lead_id,
        status: status.as_str().to_string(),
        order_card: OrderCardView::from(&card),
    }))
}

/// `GET /api/v1/voice/calls/lookup?phone=...` — pre-call lookup without creating.
async fn lookup_call(
    State(state): State<Arc<ApiState>>,
    auth: AuthenticatedRequest,
    Query(query): Query<LookupQuery>,
) -> Result<Json<LookupResponse>, AppError> {
    auth.require(Role::Operator)?;
    let key = phone::voice_key(&query.phone);
    let lead_id = lead_repo::find_by_key(&state.pool, &key).await?;
    match lead_id {
        Some(id) => {
            let lead = lead_repo::get(&state.pool, id).await?;
            Ok(Json(LookupResponse {
                found: true,
                lead_id: Some(id),
                status: Some(lead.status.as_str().to_string()),
                scope: Some(lead.scope.as_str().to_string()),
                company_name: lead.company_name,
            }))
        }
        None => Ok(Json(LookupResponse {
            found: false,
            lead_id: None,
            status: None,
            scope: None,
            company_name: None,
        })),
    }
}

// ---------------------------------------------------------------------------
// router
// ---------------------------------------------------------------------------

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/v1/voice/leads", post(create_lead))
        .route("/api/v1/voice/leads/{id}", patch(update_lead))
        .route(
            "/api/v1/voice/leads/{id}/requirements",
            get(get_requirements).put(save_requirements),
        )
        .route(
            "/api/v1/voice/leads/{id}/conversation",
            get(get_conversation).post(append_conversation),
        )
        .route("/api/v1/voice/leads/{id}/qualify", post(qualify))
        .route("/api/v1/voice/leads/{id}/handoff", post(request_handoff))
        .route("/api/v1/voice/calls/finish", post(finish_call))
        .route("/api/v1/voice/calls/lookup", get(lookup_call))
}
