//! Application bootstrap: wiring together all dependencies.

pub mod workers;

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
            sqlx::migrate!("../../migrations")
                .run(&pool)
                .await
                .map_err(AppError::Migration)?;
            info!("database migrations applied");
        }

        // 4. Build LLM provider
        let llm_provider = llm::build(&config.llm)?;
        info!(provider = %llm_provider.provider_name(), "LLM provider initialized");

        // 5. Build tool registry
        let mut tools = ToolRegistry::new();

        // Register CRM tools
        for def in crm::all_tools() {
            let name = def.name.clone();
            let handler = make_crm_handler(&name);
            tools.register(def, handler);
        }

        // Register mail tools
        for def in mail_tools::all_tools() {
            let name = def.name.clone();
            let handler = make_mail_tool_handler(&name);
            tools.register(def, handler);
        }

        // Register support tools
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

/// Stub handlers for tool registration — real implementations will use persistence.
fn make_crm_handler(
    name: &str,
) -> std::sync::Arc<
    dyn Fn(serde_json::Value) -> Result<crate::tools::ToolResult, crate::error::ToolError>
        + Send
        + Sync,
> {
    let n = name.to_string();
    std::sync::Arc::new(move |args| {
        Ok(crate::tools::ToolResult {
            success: true,
            summary: format!(
                "{n} called with {} args",
                args.as_object().map(|o| o.len()).unwrap_or(0)
            ),
            data: serde_json::json!({"called": n, "args": args}),
            error: None,
        })
    })
}

fn make_mail_tool_handler(
    name: &str,
) -> std::sync::Arc<
    dyn Fn(serde_json::Value) -> Result<crate::tools::ToolResult, crate::error::ToolError>
        + Send
        + Sync,
> {
    let n = name.to_string();
    std::sync::Arc::new(move |_args| {
        Ok(crate::tools::ToolResult {
            success: true,
            summary: format!("{n} called"),
            data: serde_json::json!({"called": n}),
            error: None,
        })
    })
}

fn make_support_tool_handler(
    name: &str,
) -> std::sync::Arc<
    dyn Fn(serde_json::Value) -> Result<crate::tools::ToolResult, crate::error::ToolError>
        + Send
        + Sync,
> {
    let n = name.to_string();
    std::sync::Arc::new(move |args| {
        Ok(crate::tools::ToolResult {
            success: true,
            summary: format!("{n} called"),
            data: serde_json::json!({"called": n, "args": args}),
            error: None,
        })
    })
}
