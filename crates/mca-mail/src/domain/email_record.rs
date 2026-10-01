use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::EmailId;

crate::domain::wire_enum! {
    /// Lifecycle of a stored inbound message.
    EmailStatus {
        /// Received but not yet claimed by a processing run.
        Pending => "pending",
        /// A worker has claimed the message.
        Processing => "processing",
        /// Successfully analysed; terminal unless a human re-runs it.
        Processed => "processed",
        /// Routed to the spam/quarantine folder; never deleted.
        Quarantined => "quarantined",
        /// Awaiting a human decision through the review endpoints.
        NeedsReview => "needs_review",
        /// Processing failed and is scheduled for retry.
        Failed => "failed",
        /// Processing failed permanently; a human must intervene.
        Dead => "dead",
    }
}

crate::domain::wire_enum! {
    /// Folder placement requested by the gateway, kept separate from
    /// `EmailStatus` so a status change never implies a mailbox mutation.
    MailboxAction {
        None => "none",
        MoveToInbox => "move_to_inbox",
        MoveToQuarantine => "move_to_quarantine",
        AddLabel => "add_label",
        MarkRead => "mark_read",
        Archive => "archive",
    }
}

crate::domain::wire_enum! {
    /// Per-agent gate applied before any LLM call is issued.
    AgentKind {
        Spam => "spam",
        Classification => "classification",
        LeadQualification => "lead_qualification",
        LogisticsExpert => "logistics_expert",
        CompanyResearch => "company_research",
        EmailCommunication => "email_communication",
        Handoff => "handoff",
    }
}

impl AgentKind {
    pub const fn tier(&self) -> ModelTier {
        match self {
            AgentKind::Spam | AgentKind::Classification => ModelTier::Cheap,
            AgentKind::LeadQualification | AgentKind::Handoff => ModelTier::Standard,
            AgentKind::LogisticsExpert | AgentKind::EmailCommunication => ModelTier::Capable,
            AgentKind::CompanyResearch => ModelTier::Cheap,
        }
    }
}

/// Cost/latency class of model requested for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Cheap,
    Standard,
    Capable,
}

impl ModelTier {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Cheap => "cheap",
            ModelTier::Standard => "standard",
            ModelTier::Capable => "capable",
        }
    }
}

/// A stored inbound email, including provider-side dedup keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEmail {
    pub id: EmailId,
    pub thread_id: super::ids::ThreadId,
    pub status: EmailStatus,
    pub direction: Direction,
    pub provider: String,
    pub mailbox: String,
    pub provider_uid: Option<String>,
    pub provider_uid_validity: Option<i64>,
    pub internet_message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub to_addresses: Vec<String>,
    pub cc_addresses: Vec<String>,
    pub subject: String,
    pub normalized_subject: String,
    pub date: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub text_body: String,
    pub html_present: bool,
    pub size_bytes: i64,
    pub category: Option<super::class::EmailCategory>,
    pub spam_verdict: Option<super::class::SpamVerdict>,
    pub lead_id: Option<super::ids::LeadId>,
    pub labels: Vec<String>,
    pub mailbox_action: MailboxAction,
    pub last_error: Option<String>,
    pub processed_at: Option<DateTime<Utc>>,
}

crate::domain::wire_enum! {
    Direction {
        Inbound => "inbound",
        Outbound => "outbound",
    }
}

impl StoredEmail {
    /// Stable dedup key: provider UID when available, RFC 5322 Message-ID
    /// otherwise, with a hash of the envelope as the last resort.
    pub fn dedup_key(&self) -> String {
        match (&self.provider_uid, &self.internet_message_id) {
            (Some(uid), validity) => {
                format!("uid:{}:{}", validity.as_deref().unwrap_or("0"), uid)
            }
            (None, Some(mid)) => format!("mid:{}", mid.trim().to_ascii_lowercase()),
            (None, None) => format!(
                "hash:{:x}",
                fallback_hash(&format!(
                    "{}|{}|{}|{}",
                    self.from_address,
                    self.subject,
                    self.received_at.timestamp_millis(),
                    self.size_bytes
                ))
            ),
        }
    }
}

/// Small non-cryptographic digest used only for dedup fallback keys.
///
/// Deliberately not used for security decisions; `sha256` handles those.
fn fallback_hash(input: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut h);
    h.finish()
}
