//! Phase 3: lead qualification, requirement extraction and conversation intake.

use tracing::warn;

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::LeadQualificationAgent;
use crate::domain::{
    normalize, AgentKind, ConversationDirection, ConversationEntry, EmailCategory, EmailThread,
    LeadId, OutboundState, ProcessingStage, RawRequirement, RequirementField, RequirementScope,
    RunId,
};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{conversation_repo, email_repo, lead_repo, requirement_repo, run_repo};

use super::dialogue;

impl Orchestrator {
    /// Returns the lead plus its company name when qualification ran, or
    /// `None` when the stage is switched off or the category may not carry a
    /// commercial record.
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
        let scope = RequirementScope::from_category(category);
        // The lead is resolved before the model is called so the prompt can
        // carry what has already been asked — that is what stops turn two from
        // repeating turn one.
        let (lead_id, created) = lead_repo::ensure(
            &self.pool,
            &thread.conversation_key,
            &ctx.email.from_address,
            Some(thread.id),
            Some(*email_id),
            scope,
        )
        .await?;
        if !created {
            conversation_repo::attach_email(&self.pool, lead_id, *email_id).await?;
        }
        let asked = if created {
            Vec::new()
        } else {
            conversation_repo::asked_questions(&self.pool, lead_id).await?
        };

        let agent = LeadQualificationAgent::new(self.llm.clone());
        let output = call_agent(
            corr,
            AgentKind::LeadQualification,
            |o| o.scope.clone(),
            agent.qualify(&ctx, &asked),
        )
        .await?;

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Qualification, true).await?;

        // Identity comes from the address that wrote to us, never the model.
        lead_repo::update_identity(&self.pool, lead_id, output.company_name.as_deref(), None)
            .await?;
        lead_repo::update_summary(
            &self.pool,
            lead_id,
            &output.summary,
            &output.questions,
            &output.regulated_topics,
            output.confidence.clamp(0.0, 1.0),
        )
        .await?;
        if output.needs_expert {
            lead_repo::mark_callback(&self.pool, lead_id, true, output.contact_phone.as_deref())
                .await?;
        }

        self.persist_requirements(lead_id, scope, &output.requirements, corr)
            .await?;
        dialogue::record_inbound(
            &self.pool,
            &ConversationEntry {
                id: uuid::Uuid::new_v4(),
                lead_id,
                email_id: Some(*email_id),
                direction: ConversationDirection::Inbound,
                state: OutboundState::Approved,
                subject: ctx.email.subject.clone(),
                body: ctx.email.text_body.clone(),
                sent_at: None,
                created_at: chrono::Utc::now(),
                idempotency_key: dialogue::inbound_key(*email_id),
            },
        )
        .await?;
        email_repo::attach_lead(&self.pool, *email_id, lead_id).await?;
        run_repo::set_lead(&self.pool, *run_id, lead_id).await?;

        // "I want a call" is the customer asking for a person: lock the
        // conversation to automation immediately rather than after a reply.
        if output.needs_expert && self.config.agents.handoff {
            let handoff_ctx = ctx_builder.build_for(AgentKind::Handoff).await?;
            self.phase_handoff(
                corr,
                &handoff_ctx,
                lead_id,
                output.company_name.clone(),
                Some("callback_requested"),
            )
            .await?;
        }

        Ok(Some((lead_id, output.company_name)))
    }

    /// Validate what the model produced, then write it through the existing
    /// upsert. Nothing reaches `lead_requirements` without passing
    /// [`crate::domain::normalize`] first.
    async fn persist_requirements(
        &self,
        lead_id: LeadId,
        scope: RequirementScope,
        raw: &[RawRequirement],
        corr: &Correlation,
    ) -> Result<(), AppError> {
        let (requirements, report) = normalize(lead_id, raw);
        if !report.is_clean() {
            warn!(
                unknown_fields = ?report.unknown_fields,
                invalid_states = ?report.invalid_states,
                "qualification output contained values that were discarded"
            );
        }
        let written = requirement_repo::upsert_many(&self.pool, lead_id, &requirements).await?;
        crate::observability::actions::requirements_updated(corr, lead_id, written as i64);

        // One row per in-scope field: everything the customer has not given is
        // explicitly `unknown`, everything irrelevant to this scope is
        // explicitly `not_applicable`. Both stop the same question being asked
        // twice, for opposite reasons.
        let (relevant, irrelevant): (Vec<RequirementField>, Vec<RequirementField>) =
            RequirementField::ALL
                .iter()
                .copied()
                .partition(|f| f.applies_to(scope));
        requirement_repo::seed_missing(&self.pool, lead_id, &relevant).await?;
        requirement_repo::mark_not_applicable(&self.pool, lead_id, &irrelevant).await?;
        Ok(())
    }
}
