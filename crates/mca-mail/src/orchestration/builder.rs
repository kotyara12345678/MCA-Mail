//! Builder pattern for constructing Orchestrator with explicit dependencies.

use std::sync::Arc;

use super::Orchestrator;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::llm::LlmProvider;
use crate::tools::ToolRegistry;

pub struct OrchestratorBuilder {
    config: Option<AppConfig>,
    pool: Option<sqlx::PgPool>,
    tools: Option<ToolRegistry>,
    llm: Option<Arc<dyn LlmProvider>>,
}

impl OrchestratorBuilder {
    pub fn new() -> Self {
        OrchestratorBuilder {
            config: None,
            pool: None,
            tools: None,
            llm: None,
        }
    }

    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }
    pub fn pool(mut self, pool: sqlx::PgPool) -> Self {
        self.pool = Some(pool);
        self
    }
    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }
    pub fn llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn build(self) -> Result<Orchestrator, AppError> {
        let config = self
            .config
            .ok_or_else(|| AppError::internal("config required"))?;
        let pool = self
            .pool
            .ok_or_else(|| AppError::internal("pool required"))?;
        let tools = self.tools.unwrap_or_default();
        let llm = self
            .llm
            .unwrap_or_else(|| Arc::new(crate::llm::mock::MockLlmProvider::new(&config.llm)));
        Ok(Orchestrator::new(llm, tools, &config, pool))
    }
}

impl Default for OrchestratorBuilder {
    fn default() -> Self {
        Self::new()
    }
}
