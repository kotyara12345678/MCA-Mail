//! Phase 5: manager handoff when the communication plan requests it.

use tracing::info;

use super::pipeline::call_agent;
use super::{AgentContext, Orchestrator};
use crate::agents::HandoffAgent;
use crate::domain::{
    AgentKind, Handoff, HandoffReason, HandoffState, LeadId, LeadStatus, Priority,
};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{handoff_repo, lead_repo};

impl Orchestrator {
    pub(super) async fn phase_handoff(
        &self,
        corr: &Correlation,
        ctx: &AgentContext,
        lead_id: LeadId,
        company_name: Option<String>,
        handoff_reason: Option<&str>,
    ) -> Result<(), AppError> {
        info!("Handoff Agent triggered");
        let agent = HandoffAgent::new(self.llm.clone());
        let ho = call_agent(
            corr,
            AgentKind::Handoff,
            |_| "prepared".to_string(),
            agent.handoff(ctx, lead_id),
        )
        .await?;

        let reason = match handoff_reason {
            Some("callback_requested") => HandoffReason::CallbackRequested,
            Some("pricing_or_legal") => HandoffReason::PricingOrLegal,
            Some("compliance_review") => HandoffReason::ComplianceReview,
            _ => HandoffReason::ReadyForManager,
        };

        let handoff = Handoff {
            id: uuid::Uuid::new_v4(),
            lead_id,
            thread_id: Some(ctx.thread_id),
            run_id: Some(ctx.run_id),
            email_id: Some(ctx.email_id),
            reason,
            priority: Priority::Normal,
            state: HandoffState::Open,
            contact_email: ctx.email.from_address.clone(),
            contact_name: ctx.email.from_name.clone(),
            contact_phone: None,
            company_name,
            company_inn: None,
            original_request: ho.original_request,
            category: None,
            spam_verdict: None,
            cargo_summary: ho.cargo_summary,
            route_summary: ho.route_summary,
            requested_service: ho.requested_service,
            missing_information: ho.missing_information,
            open_questions: ho.open_questions,
            conversation_digest: ho.conversation_digest,
            research_digest: None,
            checks_performed: ho.checks_performed,
            unresolved_topics: ho.unresolved_topics,
            assigned_to: None,
            acknowledged_at: None,
            created_at: chrono::Utc::now(),
        };

        handoff_repo::upsert_open(&self.pool, &handoff).await?;
        lead_repo::lock_automation(&self.pool, lead_id).await?;
        lead_repo::update_status(&self.pool, lead_id, LeadStatus::HandedOff).await?;
        crate::observability::actions::handoff_created(corr, handoff.id, lead_id, reason.as_str());
        Ok(())
    }
}
