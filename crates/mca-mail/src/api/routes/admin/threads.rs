//! The dialog feed: a list of conversations and the chronological block feed
//! of one conversation.
//!
//! The feed is the panel's centre of gravity, so it is served as one request
//! that merges three record streams the schema already keeps apart:
//!
//! * inbound mail (`emails`),
//! * the agent's replies (`email_drafts`, which is where "what did we answer"
//!   lives — outbound turns are never written into `emails`),
//! * processing events (`processing_events`), so a manager can see *why* the
//!   pipeline answered the way it did.
//!
//! Each source is fetched newest-first up to the page limit and the merged
//! page is then cut to the limit again — for an ordering by time, the newest
//! `N` of a union is exactly the merge of the newest `N` of each part, so the
//! page is correct without loading the whole thread.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::AuthenticatedRequest;
use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::lead_repo;

const DEFAULT_PAGE: i64 = 50;
const DEFAULT_FEED: i64 = 60;
/// Bodies are clamped server-side: a forwarded mail chain can carry megabytes
/// of text that no feed view should ever ship in one response.
const MAX_BODY_CHARS: usize = 60_000;
const PREVIEW_CHARS: i32 = 240;

/// Unprocessed mail states that keep a thread in the "needs attention" queue.
const ATTENTION_STATUSES: &str = "'pending','processing','failed','needs_review'";

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ThreadListQuery {
    /// Case-insensitive substring over subject and sender address.
    search: Option<String>,
    /// `all` (default) or `attention`.
    status: Option<String>,
    /// `true` / `false` — only red-flagged threads, or only the rest.
    flagged: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// One row of the dialog list. Built identically for the list and for the
/// feed's header, so the manager never sees two different answers about the
/// same conversation.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ThreadListItem {
    pub id: Uuid,
    pub subject: String,
    pub participant_name: Option<String>,
    pub participant_address: Option<String>,
    pub preview: String,
    pub last_activity_at: String,
    pub message_count: i64,
    pub inbound_count: i64,
    pub draft_count: i64,
    pub pending_drafts: i64,
    pub flagged: bool,
    pub needs_attention: bool,
    pub lead_id: Option<Uuid>,
}

#[derive(Serialize)]
struct ThreadListResponse {
    items: Vec<ThreadListItem>,
    total: i64,
}

const LIST_SELECT: &str = "\
SELECT t.id,
       t.normalized_subject,
       last_e.last_at,
       last_e.last_from_name,
       last_e.last_from_address,
       last_e.last_subject,
       last_e.preview,
       agg.message_count,
       agg.inbound_count,
       agg.flagged,
       agg.attention,
       agg.lead_id,
       drafts.draft_count,
       drafts.pending_drafts,
       drafts.last_draft_at
FROM email_threads t
LEFT JOIN LATERAL (
    SELECT e.received_at AS last_at,
           e.from_name AS last_from_name,
           e.from_address AS last_from_address,
           e.subject AS last_subject,
           left(regexp_replace(e.text_body, '\\s+', ' ', 'g'), $preview) AS preview
    FROM emails e
    WHERE e.thread_id = t.id
    ORDER BY e.received_at DESC, e.id DESC
    LIMIT 1
) last_e ON TRUE
JOIN LATERAL (
    SELECT count(*)::bigint AS message_count,
           count(*) FILTER (WHERE e.direction = 'inbound')::bigint AS inbound_count,
           COALESCE(bool_or(e.order_flag), FALSE) AS flagged,
           COALESCE(bool_or(e.status IN ($attention)), FALSE) AS attention,
           (SELECT e2.lead_id FROM emails e2 WHERE e2.thread_id = t.id
            ORDER BY e2.received_at DESC, e2.id DESC LIMIT 1) AS lead_id
    FROM emails e
    WHERE e.thread_id = t.id
) agg ON TRUE
LEFT JOIN LATERAL (
    SELECT count(*)::bigint AS draft_count,
           count(*) FILTER (WHERE d.status = 'pending_approval')::bigint AS pending_drafts,
           max(d.created_at) AS last_draft_at
    FROM email_drafts d
    JOIN emails e ON e.id = d.email_id
    WHERE e.thread_id = t.id
) drafts ON TRUE";

/// Predicate shared by the list and the count query. It touches only
/// `email_threads t` and uncorrelated subqueries, never the display laterals,
/// so the same text serves both statements. Parameters: `$1` search,
/// `$2` status, `$3` flagged.
const LIST_PREDICATE: &str = "\
($1::text IS NULL
   OR t.normalized_subject ILIKE '%' || $1 || '%'
   OR EXISTS (SELECT 1 FROM emails e
              WHERE e.thread_id = t.id
                AND (e.from_address ILIKE '%' || $1 || '%'
                     OR e.subject ILIKE '%' || $1 || '%')))
AND ($2::text IS NULL OR $2 <> 'attention'
   OR (EXISTS (SELECT 1 FROM emails e
                WHERE e.thread_id = t.id AND e.status IN ($attention))
       OR EXISTS (SELECT 1 FROM email_drafts d
                  JOIN emails e ON e.id = d.email_id
                  WHERE e.thread_id = t.id AND d.status = 'pending_approval')))
AND ($3::bool IS NULL
   OR (EXISTS (SELECT 1 FROM emails e
                WHERE e.thread_id = t.id AND e.order_flag)) = $3)";

fn display_select() -> String {
    LIST_SELECT
        .replace("$preview", &PREVIEW_CHARS.to_string())
        .replace("$attention", ATTENTION_STATUSES)
}

fn list_predicate() -> String {
    LIST_PREDICATE.replace("$attention", ATTENTION_STATUSES)
}

/// A thread row as stored, before the list DTO decides what a manager sees.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ThreadRowMeta {
    id: Uuid,
    normalized_subject: String,
    last_at: Option<chrono::DateTime<chrono::Utc>>,
    last_from_name: Option<String>,
    last_from_address: Option<String>,
    last_subject: Option<String>,
    preview: Option<String>,
    message_count: i64,
    inbound_count: i64,
    flagged: bool,
    attention: bool,
    lead_id: Option<Uuid>,
    draft_count: i64,
    pending_drafts: i64,
    last_draft_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ThreadRowMeta {
    fn into_item(self) -> ThreadListItem {
        let subject = self
            .last_subject
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.normalized_subject.clone());
        let last_activity_at = self
            .last_at
            .or(self.last_draft_at)
            .unwrap_or_else(chrono::Utc::now);
        ThreadListItem {
            id: self.id,
            subject,
            participant_name: self.last_from_name.clone(),
            participant_address: self.last_from_address.clone(),
            preview: self.preview.unwrap_or_default(),
            last_activity_at: last_activity_at.to_rfc3339(),
            message_count: self.message_count,
            inbound_count: self.inbound_count,
            draft_count: self.draft_count,
            pending_drafts: self.pending_drafts,
            flagged: self.flagged,
            needs_attention: self.attention || self.pending_drafts > 0,
            lead_id: self.lead_id,
        }
    }
}

async fn list_threads(
    State(state): State<SharedState>,
    Query(query): Query<ThreadListQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<ThreadListResponse>, AppError> {
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let status = match query.status.as_deref() {
        None | Some("") | Some("all") => None,
        Some("attention") => Some("attention".to_string()),
        Some(other) => {
            return Err(AppError::InvalidInput(format!(
                "unknown thread status filter {other:?}; expected all or attention"
            )))
        }
    };

    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let predicate = list_predicate();

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM email_threads t WHERE {predicate}"
    ))
    .bind(&search)
    .bind(&status)
    .bind(query.flagged)
    .fetch_one(&state.pool)
    .await?;

    let sql = format!(
        "{select} WHERE {predicate} \
         ORDER BY COALESCE(last_e.last_at, drafts.last_draft_at, t.updated_at) DESC, t.id DESC \
         LIMIT $4 OFFSET $5",
        select = display_select(),
    );
    let rows = sqlx::query_as::<_, ThreadRowMeta>(&sql)
        .bind(&search)
        .bind(&status)
        .bind(query.flagged)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(ThreadListResponse {
        items: rows.into_iter().map(ThreadRowMeta::into_item).collect(),
        total,
    }))
}

// ---------------------------------------------------------------------------
// feed
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct ThreadFeedQuery {
    limit: Option<i64>,
    /// Return blocks strictly older than this timestamp (RFC 3339). The first
    /// page omits it; "load older" passes the oldest block's `at`.
    before: Option<String>,
    /// `important` (default: warnings and errors), `all`, or `off`.
    events: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FeedItem {
    Message {
        id: Uuid,
        at: String,
        direction: String,
        from_name: Option<String>,
        from_address: String,
        to: Vec<String>,
        subject: String,
        preview: String,
        body: String,
        status: String,
        category: Option<String>,
        spam_verdict: Option<String>,
        order_flag: bool,
        lead_id: Option<Uuid>,
    },
    Draft {
        id: Uuid,
        at: String,
        email_id: Option<Uuid>,
        to: Vec<String>,
        subject: String,
        body: String,
        preview: String,
        status: String,
        reviewed_by: Option<String>,
        reviewed_at: Option<String>,
        sent_at: Option<String>,
        suppression_reason: Option<String>,
        /// Who produced the text: the agent in every case this endpoint
        /// serves; a human reviewer only ever changes the status.
        generated_by: &'static str,
    },
    Event {
        id: Uuid,
        at: String,
        severity: String,
        stage: Option<String>,
        code: String,
        message: String,
    },
}

impl FeedItem {
    fn at(&self) -> chrono::DateTime<chrono::Utc> {
        match self {
            FeedItem::Message { at, .. }
            | FeedItem::Draft { at, .. }
            | FeedItem::Event { at, .. } => chrono::DateTime::parse_from_rfc3339(at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        }
    }

    /// Stable page ordering: time first, then kind, then id, so blocks created
    /// inside the same microsecond still repeat in the same order.
    fn sort_key(&self) -> (chrono::DateTime<chrono::Utc>, u8, Uuid) {
        let kind = match self {
            FeedItem::Message { .. } => 0,
            FeedItem::Draft { .. } => 1,
            FeedItem::Event { .. } => 2,
        };
        let id = match self {
            FeedItem::Message { id, .. }
            | FeedItem::Draft { id, .. }
            | FeedItem::Event { id, .. } => *id,
        };
        (self.at(), kind, id)
    }
}

#[derive(Serialize)]
struct ThreadFeedResponse {
    thread: ThreadListItem,
    lead: Option<LeadBrief>,
    items: Vec<FeedItem>,
    has_more: bool,
}

#[derive(Serialize)]
struct LeadBrief {
    id: Uuid,
    status: String,
    category: Option<String>,
    company_name: Option<String>,
    company_inn: Option<String>,
    contact_email: String,
    summary: Option<String>,
    confidence: Option<f32>,
    open_questions: Vec<String>,
    callback_requested: bool,
    automation_locked: bool,
    last_activity_at: String,
}

#[derive(sqlx::FromRow)]
struct FeedEmailRow {
    id: Uuid,
    direction: String,
    status: String,
    category: Option<String>,
    spam_verdict: Option<String>,
    from_address: String,
    from_name: Option<String>,
    to_addresses: Vec<String>,
    subject: String,
    text_body: String,
    order_flag: bool,
    received_at: chrono::DateTime<chrono::Utc>,
    lead_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct FeedDraftRow {
    id: Uuid,
    email_id: Option<Uuid>,
    to_addresses: Vec<String>,
    subject: String,
    body: String,
    status: String,
    reviewed_by: Option<String>,
    reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
    suppression_reason: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct FeedEventRow {
    id: Uuid,
    stage: Option<String>,
    severity: String,
    code: String,
    message: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn clamp_body(body: &str) -> String {
    if body.chars().count() <= MAX_BODY_CHARS {
        return body.to_string();
    }
    let mut clipped: String = body.chars().take(MAX_BODY_CHARS).collect();
    clipped.push_str("\n… [truncated]");
    clipped
}

fn preview_of(body: &str) -> String {
    let single_line: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    single_line.chars().take(PREVIEW_CHARS as usize).collect()
}

async fn thread_feed(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ThreadFeedQuery>,
    _auth: AuthenticatedRequest,
) -> Result<Json<ThreadFeedResponse>, AppError> {
    let limit = query.limit.unwrap_or(DEFAULT_FEED).clamp(1, 200);
    let before = match query.before.as_deref() {
        None => None,
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(raw)
                .map(|d| d.with_timezone(&chrono::Utc))
                .map_err(|_| {
                    AppError::InvalidInput(format!("invalid `before` timestamp: {raw:?}"))
                })?,
        ),
    };
    let events_mode = match query.events.as_deref() {
        None | Some("") | Some("important") => "important",
        Some("all") => "all",
        Some("off") => "off",
        Some(other) => {
            return Err(AppError::InvalidInput(format!(
                "unknown events filter {other:?}; expected important, all or off"
            )))
        }
    };

    let meta_sql = format!("{select} WHERE t.id = $1", select = display_select(),);
    let meta = sqlx::query_as::<_, ThreadRowMeta>(&meta_sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("thread {id} not found")))?;

    // --- source 1: inbound mail -------------------------------------------
    let messages = sqlx::query_as::<_, FeedEmailRow>(
        "SELECT id, direction, status, category, spam_verdict, from_address, from_name, \
         to_addresses, subject, text_body, order_flag, received_at, lead_id \
         FROM emails WHERE thread_id = $1 \
         AND ($2::timestamptz IS NULL OR received_at < $2) \
         ORDER BY received_at DESC, id DESC LIMIT $3",
    )
    .bind(id)
    .bind(before)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let messages_full = messages.len() as i64 >= limit;

    // --- source 2: the agent's replies ------------------------------------
    let drafts = sqlx::query_as::<_, FeedDraftRow>(
        "SELECT d.id, d.email_id, d.to_addresses, d.subject, d.body, d.status, \
         d.reviewed_by, d.reviewed_at, d.sent_at, d.suppression_reason, d.created_at \
         FROM email_drafts d JOIN emails e ON e.id = d.email_id \
         WHERE e.thread_id = $1 \
         AND ($2::timestamptz IS NULL OR d.created_at < $2) \
         ORDER BY d.created_at DESC, d.id DESC LIMIT $3",
    )
    .bind(id)
    .bind(before)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let drafts_full = drafts.len() as i64 >= limit;

    // --- source 3: processing events --------------------------------------
    let mut events_full = false;
    let events = match events_mode {
        "off" => Vec::new(),
        mode => {
            let severities: Option<Vec<String>> = match mode {
                "important" => Some(vec!["warning".to_string(), "error".to_string()]),
                _ => None,
            };
            let rows = sqlx::query_as::<_, FeedEventRow>(
                "SELECT pe.id, pe.stage, pe.severity, pe.code, pe.message, pe.created_at \
                 FROM processing_events pe JOIN emails e ON e.id = pe.email_id \
                 WHERE e.thread_id = $1 \
                 AND ($2::timestamptz IS NULL OR pe.created_at < $2) \
                 AND ($4::text[] IS NULL OR pe.severity = ANY($4::text[])) \
                 ORDER BY pe.created_at DESC, pe.id DESC LIMIT $3",
            )
            .bind(id)
            .bind(before)
            .bind(limit)
            .bind(severities)
            .fetch_all(&state.pool)
            .await?;
            events_full = rows.len() as i64 >= limit;
            rows
        }
    };

    // --- merge --------------------------------------------------------------
    let mut items: Vec<FeedItem> = Vec::with_capacity(
        messages
            .len()
            .saturating_add(drafts.len())
            .saturating_add(events.len()),
    );
    for m in messages {
        let preview = preview_of(&m.text_body);
        items.push(FeedItem::Message {
            id: m.id,
            at: m.received_at.to_rfc3339(),
            direction: m.direction,
            from_name: m.from_name,
            from_address: m.from_address,
            to: m.to_addresses,
            subject: m.subject,
            preview,
            body: clamp_body(&m.text_body),
            status: m.status,
            category: m.category,
            spam_verdict: m.spam_verdict,
            order_flag: m.order_flag,
            lead_id: m.lead_id,
        });
    }
    for d in drafts {
        let preview = preview_of(&d.body);
        items.push(FeedItem::Draft {
            id: d.id,
            at: d.created_at.to_rfc3339(),
            email_id: d.email_id,
            to: d.to_addresses,
            subject: d.subject,
            preview,
            body: clamp_body(&d.body),
            status: d.status,
            reviewed_by: d.reviewed_by,
            reviewed_at: d.reviewed_at.map(|t| t.to_rfc3339()),
            sent_at: d.sent_at.map(|t| t.to_rfc3339()),
            suppression_reason: d.suppression_reason,
            generated_by: "ai",
        });
    }
    for e in events {
        items.push(FeedItem::Event {
            id: e.id,
            at: e.created_at.to_rfc3339(),
            severity: e.severity,
            stage: e.stage,
            code: e.code,
            message: e.message,
        });
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.sort_key()));
    items.truncate(limit as usize);
    items.reverse();

    let has_more = messages_full || drafts_full || events_full;

    // --- right panel --------------------------------------------------------
    let lead = match meta.lead_id {
        Some(lead_id) => match lead_repo::get(&state.pool, lead_id).await {
            Ok(lead) => {
                // The lock flag lives on the row, not on the domain struct:
                // the panel needs it verbatim.
                let automation_locked: bool =
                    sqlx::query_scalar("SELECT automation_locked FROM leads WHERE id = $1")
                        .bind(lead_id)
                        .fetch_optional(&state.pool)
                        .await?
                        .unwrap_or(false);
                Some(LeadBrief {
                    id: lead.id,
                    status: lead.status.as_str().to_string(),
                    category: lead.category.map(|c| c.as_str().to_string()),
                    company_name: lead.company_name,
                    company_inn: lead.company_inn,
                    contact_email: lead.contact_email,
                    summary: lead.summary,
                    confidence: lead.confidence,
                    open_questions: lead.open_questions,
                    callback_requested: lead.callback_requested,
                    automation_locked,
                    last_activity_at: lead.last_activity_at.to_rfc3339(),
                })
            }
            Err(e) => {
                tracing::warn!(lead_id = %lead_id, error = %e, "thread lead missing");
                None
            }
        },
        None => None,
    };

    Ok(Json(ThreadFeedResponse {
        thread: meta.into_item(),
        lead,
        items,
        has_more,
    }))
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/admin/threads", get(list_threads))
        .route("/api/admin/threads/{id}", get(thread_feed))
}
