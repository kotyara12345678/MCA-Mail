//! Phase 3: lead qualification for commercial emails.

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::LeadQualificationAgent;
use crate::domain::{
    AgentKind, EmailCategory, EmailThread, LeadId, ProcessingStage, RequirementScope, RunId,
};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{email_repo, lead_repo, run_repo};

impl Orchestrator {
    /// Returns the new lead plus its company name when qualification ran,
    /// or `None` when the qualification stage is switched off.
    pub(super) async fn phase_qualification(
        &self,
        corr: &Correlation,
        ctx_builder: &AgentContextBuilder,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        thread: &EmailThread,
        category: EmailCategory,
    ) -> Result<Option<(LeadId, Option<String>)>, AppError> {
        if !self.config.agents.qualification {
            return Ok(None);
        }
        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Qualification, false).await?;

        let ctx = ctx_builder.build_for(AgentKind::LeadQualification).await?;
        let agent = LeadQualificationAgent::new(self.llm.clone());
        let qual_result = call_agent(
            corr,
            AgentKind::LeadQualification,
            |o| o.scope.clone(),
            agent.qualify(&ctx),
        )
        .await?;

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Qualification, true).await?;

        // Create or find lead
        let scope = RequirementScope::from_category(category);
        let (lead_id, _) = lead_repo::ensure(
            &self.pool,
            &thread.conversation_key,
            &ctx.email.from_address,
            Some(ctx.thread_id),
            Some(*email_id),
            scope,
        )
        .await?;

        run_repo::set_lead(&self.pool, *run_id, lead_id).await?;
        email_repo::attach_lead(&self.pool, *email_id, lead_id).await?;

        Ok(Some((lead_id, qual_result.company_name)))
    }
}
