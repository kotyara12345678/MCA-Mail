//! Pipeline dispatcher and the agent-call wrapper that emits agent events.

use std::future::Future;
use std::time::Instant;

use super::{AgentContextBuilder, Orchestrator};
use crate::domain::{AgentKind, EmailCategory, EmailStatus, EmailThread, RunId};
use crate::error::AppError;
use crate::observability::{agents, errors, Correlation};

/// Runs one agent step wrapped in `agent_started`/`agent_completed`/`agent_failed`.
///
/// The model output itself is never logged — `result_of` extracts only a
/// short structured verdict (category, disposition, scope...).
pub(super) async fn call_agent<T, E, Fut>(
    corr: &Correlation,
    kind: AgentKind,
    result_of: impl FnOnce(&T) -> String,
    fut: Fut,
) -> Result<T, AppError>
where
    E: Into<AppError>,
    Fut: Future<Output = Result<T, E>>,
{
    let agent = kind.as_str();
    agents::agent_started(corr, agent);
    let started = Instant::now();
    match fut.await {
        Ok(value) => {
            let result = result_of(&value);
            agents::agent_completed(corr, agent, &result, started.elapsed().as_millis() as u64);
            Ok(value)
        }
        Err(err) => {
            let app: AppError = err.into();
            let duration_ms = started.elapsed().as_millis() as u64;
            agents::agent_failed(
                corr,
                agent,
                errors::app_error_type(&app),
                &app.to_string(),
                duration_ms,
            );
            Err(app)
        }
    }
}

impl Orchestrator {
    pub(super) async fn run_pipeline(
        &self,
        corr: &Correlation,
        run_id: &RunId,
        email_id: &uuid::Uuid,
        thread: &EmailThread,
    ) -> Result<EmailStatus, AppError> {
        let ctx_builder = AgentContextBuilder::new(
            self.pool.clone(),
            *run_id,
            *email_id,
            self.llm.clone(),
            self.tools.clone(),
            self.config.agents.clone(),
            self.config.security.email_mode,
        );

        if let Some(status) = self
            .phase_spam(corr, &ctx_builder, run_id, email_id)
            .await?
        {
            return Ok(status);
        }
        let (category, early) = self
            .phase_classification(corr, &ctx_builder, run_id, email_id)
            .await?;
        if let Some(status) = early {
            return Ok(status);
        }

        // Qualification off (or no usable lead) means the pipeline ends here.
        let Some((lead_id, company_name)) = self
            .phase_qualification(corr, &ctx_builder, run_id, email_id, thread, category)
            .await?
        else {
            return Ok(EmailStatus::Processed);
        };

        self.phase_communication(corr, &ctx_builder, run_id, email_id, lead_id, company_name)
            .await?;
        Ok(EmailStatus::Processed)
    }
}
