//! Phases 1-2: spam detection and classification.

use tracing::info;

use super::pipeline::call_agent;
use super::{AgentContextBuilder, Orchestrator};
use crate::agents::{ClassificationAgent, SpamAgent};
use crate::domain::{AgentKind, EmailCategory, EmailStatus, ProcessingStage, RunId, SpamVerdict};
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
                info!(verdict = ?assessment.verdict, "spam detected");
                return Ok(Some(EmailStatus::Quarantined));
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

        let early = match category {
            EmailCategory::Spam => Some(EmailStatus::Quarantined),
            EmailCategory::Advertisement | EmailCategory::Internal => Some(EmailStatus::Processed),
            _ => None,
        };
        Ok((category, early))
    }
}
