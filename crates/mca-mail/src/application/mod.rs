//! Application bootstrap: wiring together all dependencies.

mod handlers;
pub mod workers;

use handlers::{make_crm_handler, make_mail_tool_handler, make_support_tool_handler};

use std::sync::Arc;

use sqlx::PgPool;
use tracing::{error, info};

use crate::api::{self, ApiState};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::llm;
use crate::orchestration::{Orchestrator, OrchestratorBuilder};
use crate::persistence;
use crate::tools::{crm, mail as mail_tools, support, ToolRegistry};

/// Bootstrap the application: config, pool, LLM, tools, orchestrator, and API.
pub struct App {
    pub config: AppConfig,
    pub pool: PgPool,
    pub orchestrator: Arc<Orchestrator>,
    pub state: Arc<ApiState>,
}

impl App {
    pub async fn bootstrap() -> Result<Self, AppError> {
        // 1. Load config
        let config = AppConfig::load()?;
        config.validate()?;

        info!(env = %config.app.env, "configuration loaded");

        // 2. Connect to database
        let pool = persistence::pool::connect(&config.database).await?;

        // 3. Run migrations
        if config.database.auto_migrate {
            let started = std::time::Instant::now();
            crate::observability::system::migration_started();
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .map_err(AppError::Migration)?;
            crate::observability::system::migration_completed(started.elapsed().as_millis() as u64);
            info!("database migrations applied");
        }

        // 4. Build LLM provider
        let llm_provider = llm::build(&config.llm)?;
        info!(provider = %llm_provider.provider_name(), "LLM provider initialized");

        // 5. Build tool registry
        let mut tools = ToolRegistry::new();

        for def in crm::all_tools() {
            let name = def.name.clone();
            let handler = make_crm_handler(&name);
            tools.register(def, handler);
        }

        for def in mail_tools::all_tools() {
            let name = def.name.clone();
            let handler = make_mail_tool_handler(&name);
            tools.register(def, handler);
        }

        for def in support::all_tools() {
            let name = def.name.clone();
            let handler = make_support_tool_handler(&name);
            tools.register(def, handler);
        }

        info!(tool_count = %tools.describe_for_agent(crate::domain::AgentKind::Handoff).len(), "tool registry populated");

        // 6. Build orchestrator
        let orchestrator = OrchestratorBuilder::new()
            .config(config.clone())
            .pool(pool.clone())
            .tools(tools.clone())
            .llm(llm_provider.clone())
            .build()?;

        let orchestrator = Arc::new(orchestrator);

        // 7. Build API state
        let db_health = persistence::pool::health(&pool).await;
        let state = Arc::new(ApiState::new(pool.clone(), db_health));

        Ok(App {
            config,
            pool,
            orchestrator,
            state,
        })
    }
}
