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
use crate::mail::MaybeWritable;
use crate::orchestration::{Orchestrator, OrchestratorBuilder};
use crate::persistence;
use crate::tools::{crm, mail as mail_tools, support, ToolRegistry};

/// Bootstrap the application: config, pool, LLM, tools, orchestrator, and API.
pub struct App {
    pub config: AppConfig,
    pub pool: PgPool,
    pub orchestrator: Arc<Orchestrator>,
    pub state: Arc<ApiState>,
    /// Shared with the orchestrator and the workers, so exactly one transport
    /// instance exists per process instead of one per component.
    pub mailbox: Option<Arc<dyn MaybeWritable>>,
}

impl App {
    pub async fn bootstrap() -> Result<Self, AppError> {
        let config = load_config()?;
        let pool = open_database(&config).await?;

        // A profile selected in the admin panel overrides the environment
        // values at runtime; the mock provider is left alone for tests.
        let llm_provider = llm::build_with_profiles(&config.llm, Some(pool.clone()))?;
        info!(provider = %llm_provider.provider_name(), "LLM provider initialized");

        let tools = build_tools(&config);
        info!(tool_count = %tools.describe_for_agent(crate::domain::AgentKind::Handoff).len(), "tool registry populated");

        // Built once: the orchestrator quarantines spam through it and the
        // workers read and deliver through the same one.
        let mailbox = build_mailbox(&config);

        let orchestrator = OrchestratorBuilder::new()
            .config(config.clone())
            .pool(pool.clone())
            .tools(tools.clone())
            .llm(llm_provider.clone())
            .mailbox(mailbox.clone())
            .build()?;
        let orchestrator = Arc::new(orchestrator);

        let state = build_api_state(&config, &pool);

        Ok(App {
            config,
            pool,
            orchestrator,
            state,
            mailbox,
        })
    }
}

/// Step 1: read and validate the configuration.
fn load_config() -> Result<AppConfig, AppError> {
    let config = AppConfig::load()?;
    config.validate()?;
    info!(
        env = %config.app.env,
        mail_mode = %config.mail.mode,
        mailbox_writes_allowed = config.mail.mode.allows_mailbox_write(),
        "configuration loaded"
    );
    Ok(config)
}

/// Steps 2–3: connect to PostgreSQL and, when enabled, apply migrations.
async fn open_database(config: &AppConfig) -> Result<PgPool, AppError> {
    let pool = persistence::pool::connect(&config.database).await?;
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
    Ok(pool)
}

/// Step 5: register CRM, mail, and support tools.
fn build_tools(config: &AppConfig) -> ToolRegistry {
    let mut tools = ToolRegistry::new();

    for def in crm::all_tools() {
        let name = def.name.clone();
        let handler = make_crm_handler(&name);
        tools.register(def, handler);
    }

    // Mutating mail tools are withheld in read-only mode so an agent is never
    // offered a call the mail layer will refuse. The guard in the transport
    // is the actual enforcement; this only avoids wasted iterations.
    for def in crate::tools::filter_for_mode(mail_tools::all_tools(), config.mail.mode) {
        let name = def.name.clone();
        let handler = make_mail_tool_handler(&name);
        tools.register(def, handler);
    }

    for def in support::all_tools() {
        let name = def.name.clone();
        let handler = make_support_tool_handler(&name);
        tools.register(def, handler);
    }

    tools
}

/// Step 6: the single mailbox handle; `None` when the transport cannot be
/// built (the poll loop then stays disabled instead of crashing).
fn build_mailbox(config: &AppConfig) -> Option<Arc<dyn MaybeWritable>> {
    match crate::mail::build_writable(&config.mail) {
        Ok(provider) => Some(Arc::from(provider)),
        Err(e) => {
            error!(error = %e, "failed to build mail provider; poll loop disabled");
            None
        }
    }
}

/// Step 8: API state. Health is probed per request by the /health and
/// /ready handlers, so nothing is snapshotted here. The admin panel login
/// defaults to the mailbox credentials (one login/password for mail and
/// panel), or `ADMIN_LOGIN` / `ADMIN_PASSWORD` when set.
fn build_api_state(config: &AppConfig, pool: &PgPool) -> Arc<ApiState> {
    let mut state = ApiState::new(pool.clone()).with_config(Arc::new(config.clone()));
    if let Some((login, password)) = panel_credentials(config) {
        state = state.with_admin(login, password);
    } else {
        error!("admin panel login disabled: no password configured (set ADMIN_PASSWORD or mailbox password)");
    }
    Arc::new(state)
}

/// Panel login/password: explicit env first, mailbox credentials second.
/// `None` when there is no password at all — the panel then refuses to log
/// in (503) rather than start with a guessable default.
fn panel_credentials(config: &AppConfig) -> Option<(String, String)> {
    let login = {
        let configured = config.api.admin_login.trim();
        if configured.is_empty() {
            config.mail.username.trim().to_string()
        } else {
            configured.to_string()
        }
    };
    let password = {
        let explicit = config.api.admin_password.expose();
        if explicit.is_empty() {
            config.mail.password.expose()
        } else {
            explicit
        }
    };
    if login.is_empty() || password.is_empty() {
        return None;
    }
    Some((login, password.to_string()))
}
