//! Stage F: the manager card is queued the moment a lead is qualified.
//!
//! The card is built from what is already in the database and pushed into the
//! same outbox the customer replies use, so exactly one component decides
//! whether it may leave the building. `SEND_MANAGER_CARD=false` keeps the row
//! in the queue for the audit trail and lets the send worker hold it.
//!
//! Several managers may share one card: `MANAGER_CARD_RECIPIENT` names them
//! all, and each address gets its own row, its own hourly ceiling and its own
//! delivery history.

use tracing::info;

use super::Orchestrator;
use crate::domain::{LeadId, ManagerCard};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{
    conversation_repo, lead_repo, outbox_repo, outbox_repo::OutboundKind, requirement_repo,
};

/// Idempotency key for one lead's card to one address: one card per lead per
/// recipient, ever. The address is part of the key because two managers each
/// get their own copy and neither may get a second one — a reprocessed
/// pipeline collapses onto the row that already carries it.
pub(crate) fn manager_card_key(lead_id: LeadId, recipient: &str) -> String {
    format!(
        "manager_card:{lead_id}:{}",
        recipient.trim().to_ascii_lowercase()
    )
}

impl Orchestrator {
    /// Build and enqueue the manager card for `lead_id`, once per recipient.
    ///
    /// Failures here are logged, not propagated: the customer's reply already
    /// went out, and a missing card must not replay the whole email.
    pub(crate) async fn enqueue_manager_card(
        &self,
        corr: &Correlation,
        lead_id: LeadId,
    ) -> Result<(), AppError> {
        let settings = &self.config.security.manager_card;
        let recipients = settings.recipients();
        if recipients.is_empty() {
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
        let subject = format!(
            "Карточка клиента: {} — {}",
            lead.company_name
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("без названия"),
            lead.contact_email
        );
        let body_text = card.to_text();
        let body_html = card.to_html();

        for recipient in recipients {
            let intent = outbox_repo::OutboundIntent {
                message_type: OutboundKind::ManagerCard,
                lead_id: Some(lead_id),
                email_id: lead.primary_email_id,
                run_id: None,
                recipient: recipient.to_string(),
                subject: subject.clone(),
                body_text: body_text.clone(),
                body_html: body_html.clone(),
                in_reply_to: None,
                ref_headers: Vec::new(),
                idempotency_key: manager_card_key(lead_id, recipient),
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
                        recipient,
                        enabled = settings.enabled,
                        "manager card queued"
                    );
                }
                None => info!(recipient, "manager card already queued for this lead"),
            }
        }
        Ok(())
    }
}
