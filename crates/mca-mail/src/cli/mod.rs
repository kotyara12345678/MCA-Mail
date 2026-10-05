//! Admin CLI: operational subcommands (`mca-mail api-key ...`).
//!
//! The binary starts the server when invoked with no arguments; any argument
//! routes here instead.

pub mod api_key;
mod args;

use anyhow::{bail, Context};
use sqlx::PgPool;

use crate::config::AppConfig;
use crate::persistence;

pub const USAGE: &str = "\
MCA Mail admin CLI

USAGE:
    mca-mail                                     start the server
    mca-mail api-key create --name <name> --role <role> [--expires-days <n>] [--created-by <who>]
    mca-mail api-key list
    mca-mail api-key revoke <id|prefix>
    mca-mail healthcheck                         probe a running instance (for Docker)
    mca-mail help

ROLES: viewer, operator, manager, admin";

pub async fn run(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("api-key") => {
            let pool = connect().await?;
            api_key::run(&pool, &args[1..]).await
        }
        Some("healthcheck") => healthcheck().await,
        Some("help") | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown command `{other}`\n\n{USAGE}"),
        None => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

/// Docker HEALTHCHECK: GET /ready on a local instance, exit non-zero when it
/// is unreachable or reports 503. `/ready` rather than `/health` on purpose:
/// liveness only proves the process answers HTTP, while readiness is the one
/// that fails once the database is gone — the state an operator actually needs
/// surfaced in `docker compose ps`. Uses loopback because the bound host is
/// often `0.0.0.0`, which is not a connectable destination on some systems.
async fn healthcheck() -> anyhow::Result<()> {
    let config = AppConfig::load().context("loading configuration (.env)")?;
    let url = format!("http://127.0.0.1:{}/ready", config.api.port);
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("GET {url} -> {status}: {body}");
    }
    println!("{body}");
    Ok(())
}

/// Config + pool only: the CLI must work without LLM or mailbox credentials,
/// so it deliberately skips `App::bootstrap`.
async fn connect() -> anyhow::Result<PgPool> {
    let config = AppConfig::load().context("loading configuration (.env)")?;
    let pool = persistence::pool::connect(&config.database).await?;
    if config.database.auto_migrate {
        sqlx::migrate!("../../migrations").run(&pool).await?;
    }
    Ok(pool)
}
