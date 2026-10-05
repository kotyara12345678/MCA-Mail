//! Stage F: the manager card is queued the moment a lead is qualified.
//!
//! The card is built from what is already in the database and pushed into the
//! same outbox the customer replies use, so exactly one component decides
//! whether it may leave the building. `SEND_MANAGER_CARD=false` keeps the row
//! in the queue for the audit trail and lets the send worker hold it.

use tracing::info;

use super::Orchestrator;
use crate::domain::{LeadId, ManagerCard};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{
    conversation_repo, lead_repo, outbox_repo, outbox_repo::OutboundKind, requirement_repo,
};

/// Idempotency key for a lead's manager card: one card per lead, ever.
pub(crate) fn manager_card_key(lead_id: LeadId) -> String {
    format!("manager_card:{lead_id}")
}

impl Orchestrator {
    /// Build and enqueue the manager card for `lead_id`.
    ///
    /// Failures here are logged, not propagated: the customer's reply already
    /// went out, and a missing card must not replay the whole email.
    pub(crate) async fn enqueue_manager_card(
        &self,
        corr: &Correlation,
        lead_id: LeadId,
    ) -> Result<(), AppError> {
        let settings = self.config.security.manager_card.clone();
        let recipient = settings.recipient.trim().to_string();
        if recipient.is_empty() {
            info!("manager card recipient not configured; card skipped");
            return Ok(());
        }

        let lead = lead_repo::get(&self.pool, lead_id).await?;
        let requirements = requirement_repo::all(&self.pool, lead_id).await?;
        let history = conversation_repo::history(&self.pool, lead_id, 50).await?;

        let card = ManagerCard {
            lead: &lead,
            requirements: &requirements,
            history: &history,
            generated_at: chrono::Utc::now(),
        };

        let intent = outbox_repo::OutboundIntent {
            message_type: OutboundKind::ManagerCard,
            lead_id: Some(lead_id),
            email_id: lead.primary_email_id,
            run_id: None,
            recipient,
            subject: format!(
                "Карточка клиента: {} — {}",
                lead.company_name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("без названия"),
                lead.contact_email
            ),
            body_text: card.to_text(),
            body_html: card.to_html(),
            in_reply_to: None,
            ref_headers: Vec::new(),
            idempotency_key: manager_card_key(lead_id),
            correlation_id: Some(corr.processing_id.to_string()),
        };

        match outbox_repo::enqueue_send(&self.pool, &intent).await? {
            Some(outbox_id) => {
                crate::observability::actions::manager_card_queued(
                    corr,
                    outbox_id,
                    lead_id,
                    settings.enabled,
                );
                info!(
                    outbox_id = %outbox_id,
                    enabled = settings.enabled,
                    "manager card queued"
                );
            }
            None => info!("manager card already queued for this lead"),
        }
        Ok(())
    }
}
