//! Phase 4: reply planning and draft creation.

use tracing::info;

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::CommunicationAgent;
use crate::domain::{AgentKind, LeadId, ProcessingStage, RunId};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{draft_repo, run_repo};

impl Orchestrator {
    pub(super) async fn phase_communication(
        &self,
        corr: &Correlation,
        ctx_builder: &AgentContextBuilder,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        lead_id: LeadId,
        company_name: Option<String>,
    ) -> Result<(), AppError> {
        if !self.config.agents.communication {
            return Ok(());
        }
        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, false).await?;

        let ctx = ctx_builder.build_for(AgentKind::EmailCommunication).await?;
        let agent = CommunicationAgent::new(self.llm.clone());
        let plan = call_agent(
            corr,
            AgentKind::EmailCommunication,
            |o| o.disposition.clone(),
            agent.plan_response(&ctx, lead_id),
        )
        .await?;

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::ReplyPlanning, true).await?;

        match plan.disposition.as_str() {
            "suppress" => info!("response suppressed"),
            "draft" | "send" => {
                let draft = crate::domain::EmailDraft {
                    id: uuid::Uuid::new_v4(),
                    lead_id: Some(lead_id),
                    email_id: Some(*email_id),
                    in_reply_to: ctx.email.internet_message_id.clone(),
                    to_addresses: vec![ctx.email.from_address.clone()],
                    cc_addresses: vec![],
                    subject: plan.subject.clone(),
                    body: plan.body.clone(),
                    status: crate::domain::DraftStatus::PendingApproval,
                    idempotency_key: draft_repo::idempotency_key(*run_id, lead_id, &plan.body),
                    suppression_reason: None,
                    reviewed_by: None,
                    reviewed_at: None,
                    sent_at: None,
                    provider_message_id: None,
                    created_at: chrono::Utc::now(),
                };
                let (draft_id, created) = draft_repo::create(&self.pool, &draft).await?;
                crate::observability::actions::draft_created(
                    corr,
                    draft_id,
                    lead_id,
                    &plan.disposition,
                );
                info!(
                    draft_id = %draft_id,
                    created,
                    disposition = %plan.disposition,
                    "draft prepared"
                );
            }
            _ => {}
        }

        // Phase 5: handoff if requested
        if plan.handoff_requested {
            self.phase_handoff(
                corr,
                &ctx,
                lead_id,
                company_name,
                plan.handoff_reason.as_deref(),
            )
            .await?;
        }
        Ok(())
    }
}
