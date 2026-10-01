//! Output artifacts and mailbox actions: drafts, handoffs, move, label.

use tracing::info;

use super::{Correlation, EVENT_TARGET};

/// A reply draft was created (or the existing idempotent one reused).
pub fn draft_created(
    c: &Correlation,
    draft_id: crate::domain::DraftId,
    lead_id: crate::domain::LeadId,
    disposition: &str,
) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        draft_id = %draft_id,
        lead_id = %lead_id,
        disposition,
        "draft_created"
    );
}

/// A manager handoff was opened for the lead.
pub fn handoff_created(
    c: &Correlation,
    handoff_id: crate::domain::HandoffId,
    lead_id: crate::domain::LeadId,
    reason: &str,
) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        handoff_id = %handoff_id,
        lead_id = %lead_id,
        reason,
        "handoff_created"
    );
}

/// An agent moved an email to another folder (tool-triggered).
pub fn email_moved(email_id: Option<crate::domain::EmailId>, folder: &str) {
    if let Some(id) = email_id {
        info!(target: EVENT_TARGET, email_id = %id, folder, "email_moved");
    } else {
        info!(target: EVENT_TARGET, folder, "email_moved");
    }
}

/// An agent added a label to an email (tool-triggered).
pub fn email_labeled(email_id: Option<crate::domain::EmailId>, label: &str) {
    if let Some(id) = email_id {
        info!(target: EVENT_TARGET, email_id = %id, label, "email_labeled");
    } else {
        info!(target: EVENT_TARGET, label, "email_labeled");
    }
}
