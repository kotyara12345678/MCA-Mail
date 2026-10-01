use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ids::{DraftId, LeadId};

crate::domain::wire_enum! {
    DraftStatus {
        /// Generated, waiting for a human to approve or reject.
        PendingApproval => "pending_approval",
        Approved => "approved",
        Rejected => "rejected",
        Sent => "sent",
        /// Blocked by policy (rate limit, dry-run, automation lock).
        Suppressed => "suppressed",
    }
}

/// A generated reply awaiting approval, or already dispatched.
///
/// Drafts are the unit of at-most-once delivery: the idempotency key is a
/// unique index in the database, so a reprocess or a concurrent worker cannot
/// produce a second message to the same customer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailDraft {
    pub id: DraftId,
    pub lead_id: Option<LeadId>,
    pub email_id: Option<super::ids::EmailId>,
    pub in_reply_to: Option<String>,
    pub to_addresses: Vec<String>,
    pub cc_addresses: Vec<String>,
    pub subject: String,
    pub body: String,
    pub status: DraftStatus,
    /// SHA-256 over run id, lead id and body; unique in the database.
    pub idempotency_key: String,
    /// Why the send was suppressed, when it was.
    pub suppression_reason: Option<String>,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub sent_at: Option<DateTime<Utc>>,
    pub provider_message_id: Option<String>,
    pub created_at: DateTime<Utc>,
}
