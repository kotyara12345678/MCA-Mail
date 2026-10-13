//! Phases 1-2: spam detection and classification.

use tracing::info;

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::{ClassificationAgent, SpamAgent};
use crate::domain::{
    confident_spam, is_order_related, AgentKind, EmailCategory, EmailStatus, EmailThread,
    ProcessingStage, RunId, SpamVerdict,
};
use crate::error::AppError;
use crate::observability::Correlation;
use crate::persistence::{email_repo, run_repo};

impl Orchestrator {
    pub(super) async fn phase_spam(
        &self,
        corr: &Correlation,
        ctx_builder: &AgentContextBuilder,
        run_id: &RunId,
        email_id: &uuid::Uuid,
    ) -> Result<Option<EmailStatus>, AppError> {
        if !self.config.agents.spam {
            return Ok(None);
        }
        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Spam, false).await?;

        let ctx = ctx_builder.build_for(AgentKind::Spam).await?;
        let agent = SpamAgent::new(self.llm.clone());
        let assessment = call_agent(
            corr,
            AgentKind::Spam,
            |o| o.verdict.as_str().to_string(),
            agent.assess(&ctx),
        )
        .await?;

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Spam, true).await?;
        email_repo::set_spam_verdict(&self.pool, *email_id, assessment.verdict).await?;

        match assessment.verdict {
            SpamVerdict::Spam | SpamVerdict::Advertisement => {
                if confident_spam(assessment.verdict, assessment.confidence) {
                    info!(verdict = ?assessment.verdict, "high-confidence spam detected");
                    self.quarantine_spam(corr, &ctx.email).await;
                    return Ok(Some(EmailStatus::Quarantined));
                }
                info!(verdict = ?assessment.verdict, "low-confidence spam sent for review");
                return Ok(Some(EmailStatus::NeedsReview));
            }
            SpamVerdict::PhishingSuspected => {
                info!("phishing suspected, needs review");
                return Ok(Some(EmailStatus::NeedsReview));
            }
            _ => {}
        }
        Ok(None)
    }

    pub(super) async fn phase_classification(
        &self,
        corr: &Correlation,
        ctx_builder: &AgentContextBuilder,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        thread: &EmailThread,
    ) -> Result<(EmailCategory, Option<EmailStatus>), AppError> {
        if !self.config.agents.classification {
            return Ok((EmailCategory::Uncertain, None));
        }
        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Classification, false).await?;

        let ctx = ctx_builder.build_for(AgentKind::Classification).await?;
        let agent = ClassificationAgent::new(self.llm.clone());
        let outcome = call_agent(
            corr,
            AgentKind::Classification,
            |o| o.category.as_str().to_string(),
            agent.classify(&ctx),
        )
        .await?;
        let category = outcome.category;

        run_repo::record_stage(&self.pool, *run_id, ProcessingStage::Classification, true).await?;
        email_repo::set_category(&self.pool, *email_id, category).await?;
        self.raise_order_flag(*email_id, thread, category, outcome.confidence)
            .await?;

        let early = match category {
            EmailCategory::Spam if confident_spam(SpamVerdict::Spam, outcome.confidence) => {
                Some(EmailStatus::Quarantined)
            }
            EmailCategory::Spam => Some(EmailStatus::NeedsReview),
            EmailCategory::Advertisement | EmailCategory::Internal => Some(EmailStatus::Processed),
            // "other" means the model recognised no business shape: a person
            // has to look, and no lead or reply may be produced from it.
            EmailCategory::Other => Some(EmailStatus::NeedsReview),
            _ => None,
        };
        Ok((category, early))
    }

    /// Mark the message as potential order traffic when the signals the
    /// pipeline already holds say so.
    ///
    /// Runs once per classification, right after the category is stored, and
    /// is one-way: the flag is raised, never lowered (see
    /// [`crate::persistence::email_repo::raise_order_flag`]). The verdict is
    /// the domain rule [`crate::domain::is_order_related`] — no extra model
    /// call, no second opinion.
    async fn raise_order_flag(
        &self,
        email_id: uuid::Uuid,
        thread: &EmailThread,
        category: EmailCategory,
        confidence: f32,
    ) -> Result<(), AppError> {
        let spam = email_repo::spam_verdict(&self.pool, email_id).await?;
        let thread_context = email_repo::thread_has_order(&self.pool, thread.id, email_id).await?;
        if !is_order_related(category, spam, confidence, thread_context) {
            return Ok(());
        }
        email_repo::raise_order_flag(&self.pool, email_id).await?;
        info!(email_id = %email_id, category = category.as_str(), "order flag raised");
        Ok(())
    }
}
