//! Agent Orchestrator — manages the agent pipeline for each inbound email.

use std::sync::Arc;

use crate::config::AppConfig;
use crate::llm::LlmProvider;
use crate::tools::ToolRegistry;

pub mod builder;
pub mod communication;
pub mod context;
mod dialogue;
mod handoff;
pub mod loop_guard;
mod manager_card;
mod pipeline;
mod processing;
mod quarantine;
mod reply;
mod stages;

pub use builder::OrchestratorBuilder;
pub use context::{AgentContext, AgentContextBuilder};
pub use dialogue::Dialogue;
pub use loop_guard::{BudgetTracker, IterationGuard, ToolCallGuard};

/// Owns the agent pipeline for one process: LLM, tools, database.
pub struct Orchestrator {
    llm: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    pool: sqlx::PgPool,
    config: AppConfig,
    /// The one handle allowed to mutate the mailbox, shared with the send
    /// worker. `None` in tests and in deployments whose transport failed to
    /// build; the spam stage then logs and leaves the message in the inbox.
    mailbox: Option<Arc<dyn crate::mail::MaybeWritable>>,
}

impl Orchestrator {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        config: &AppConfig,
        pool: sqlx::PgPool,
        mailbox: Option<Arc<dyn crate::mail::MaybeWritable>>,
    ) -> Self {
        Orchestrator {
            llm,
            tools,
            pool,
            config: config.clone(),
            mailbox,
        }
    }
}
