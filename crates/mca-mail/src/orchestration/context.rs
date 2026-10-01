//! Agent context: data passed to each agent during a processing run.

use std::sync::Arc;

use tracing::info;

use crate::config::AgentSettings;
use crate::domain::{AgentKind, EmailId, RunId, StoredEmail};
use crate::error::AppError;
use crate::llm::LlmProvider;
use crate::persistence::{email_repo, lead_repo, thread_repo};
use crate::tools::ToolRegistry;

/// Context provided to an agent for its processing turn.
#[derive(Clone)]
pub struct AgentContext {
    pub run_id: RunId,
    pub email_id: EmailId,
    pub email: StoredEmail,
    pub thread_id: crate::domain::ThreadId,
    pub agent_kind: AgentKind,
    pub llm: Arc<dyn LlmProvider>,
    pub tools: ToolRegistry,
    pub pool: sqlx::PgPool,
    pub settings: AgentSettings,
    pub email_mode: crate::config::EmailMode,
}

pub struct AgentContextBuilder {
    pool: sqlx::PgPool,
    run_id: RunId,
    email_id: EmailId,
    llm: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    settings: AgentSettings,
    email_mode: crate::config::EmailMode,
}

impl AgentContextBuilder {
    pub fn new(
        pool: sqlx::PgPool,
        run_id: RunId,
        email_id: EmailId,
        llm: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        settings: AgentSettings,
        email_mode: crate::config::EmailMode,
    ) -> Self {
        AgentContextBuilder {
            pool,
            run_id,
            email_id,
            llm,
            tools,
            settings,
            email_mode,
        }
    }

    pub async fn build_for(&self, agent: AgentKind) -> Result<AgentContext, AppError> {
        let email = email_repo::get(&self.pool, self.email_id).await?;

        let thread = thread_repo::get(&self.pool, email.thread_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("thread {}", email.thread_id)))?;

        info!(agent = %agent.as_str(), email_id = %self.email_id, "building agent context");

        Ok(AgentContext {
            run_id: self.run_id,
            email_id: self.email_id,
            email,
            thread_id: thread.id,
            agent_kind: agent,
            llm: self.llm.clone(),
            tools: self.tools.clone(),
            pool: self.pool.clone(),
            settings: self.settings.clone(),
            email_mode: self.email_mode,
        })
    }
}
