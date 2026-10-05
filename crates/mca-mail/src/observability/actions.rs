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

/// A batch of extracted commercial requirements was written for a lead.
pub fn requirements_updated(c: &Correlation, lead_id: crate::domain::LeadId, written: i64) {
    if written <= 0 {
        return;
    }
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        lead_id = %lead_id,
        written,
        "requirements_updated"
    );
}

/// A customer reply entered the outbox, on its way to the send worker.
pub fn reply_queued(c: &Correlation, outbox_id: uuid::Uuid, lead_id: crate::domain::LeadId) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        outbox_id = %outbox_id,
        lead_id = %lead_id,
        "reply_queued"
    );
}

/// A lead reached `qualified`: every quote-blocking requirement is answered.
pub fn lead_qualified(c: &Correlation, lead_id: crate::domain::LeadId) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        lead_id = %lead_id,
        "lead_qualified"
    );
}

/// The manager card entered the outbox. `enabled` records whether policy will
/// let it out, so a held card is distinguishable from a delivered one later.
pub fn manager_card_queued(
    c: &Correlation,
    outbox_id: uuid::Uuid,
    lead_id: crate::domain::LeadId,
    enabled: bool,
) {
    info!(
        target: EVENT_TARGET,
        email_id = %c.email_id,
        processing_id = %c.processing_id,
        outbox_id = %outbox_id,
        lead_id = %lead_id,
        enabled,
        "manager_card_queued"
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
