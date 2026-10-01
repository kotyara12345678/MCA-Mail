//! Agent Orchestrator — manages the agent pipeline for each inbound email.

use std::sync::Arc;

use crate::config::AppConfig;
use crate::llm::LlmProvider;
use crate::tools::ToolRegistry;

pub mod builder;
pub mod communication;
pub mod context;
mod handoff;
pub mod loop_guard;
mod pipeline;
mod processing;
mod reply;
mod stages;

pub use builder::OrchestratorBuilder;
pub use context::{AgentContext, AgentContextBuilder};
pub use loop_guard::{BudgetTracker, IterationGuard, ToolCallGuard};

/// Owns the agent pipeline for one process: LLM, tools, database.
pub struct Orchestrator {
    llm: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    pool: sqlx::PgPool,
    config: AppConfig,
}

impl Orchestrator {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        config: &AppConfig,
        pool: sqlx::PgPool,
    ) -> Self {
        Orchestrator {
            llm,
            tools,
            pool,
            config: config.clone(),
        }
    }
}
