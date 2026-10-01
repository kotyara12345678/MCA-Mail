//! `mca-mail api-key` subcommands: create, list, revoke API credentials.

use anyhow::{bail, Context};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::args::parse_create;
use crate::persistence::api_key_repo;

const USAGE: &str = "\
mca-mail api-key <create|list|revoke>

  create --name <name> --role <viewer|operator|manager|admin>
         [--expires-days <n>] [--created-by <who>]
  list   show all keys: id, prefix, role, activity
  revoke <id|prefix>  deactivate a key";

pub async fn run(pool: &PgPool, args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("create") => create(pool, &args[1..]).await,
        Some("list") => list_cmd(pool).await,
        Some("revoke") => revoke(pool, &args[1..]).await,
        Some(other) => bail!("api-key: unknown subcommand `{other}`\n\n{USAGE}"),
        None => bail!("api-key: missing subcommand\n\n{USAGE}"),
    }
}

async fn create(pool: &PgPool, args: &[String]) -> anyhow::Result<()> {
    let opts = parse_create(args)?;
    let expires = opts
        .expires_days
        .map(|d| Utc::now() + chrono::Duration::days(i64::from(d)));
    let (raw, id) =
        api_key_repo::create(pool, &opts.name, opts.role, &opts.created_by, expires).await?;

    println!("API key created — shown once, store it now:");
    println!("  key:      {raw}");
    println!("  id:       {id}");
    println!("  name:     {}", opts.name);
    println!("  role:     {}", opts.role.as_str());
    match expires {
        Some(at) => println!("  expires:  {}", at.to_rfc3339()),
        None => println!("  expires:  never"),
    }
    Ok(())
}

async fn list_cmd(pool: &PgPool) -> anyhow::Result<()> {
    let rows = api_key_repo::list(pool).await?;
    if rows.is_empty() {
        println!("no API keys — create one with `mca-mail api-key create`");
        return Ok(());
    }
    println!(
        "{:<38} {:<8} {:<9} {:<20} {:<7} {:<10} {:<10}",
        "ID", "PREFIX", "ROLE", "NAME", "ACTIVE", "LAST USED", "EXPIRES"
    );
    for r in rows {
        println!(
            "{:<38} {:<8} {:<9} {:<20} {:<7} {:<10} {:<10}",
            r.id,
            r.prefix,
            r.role,
            fit(&r.name, 20),
            if r.is_active { "yes" } else { "no" },
            date_opt(r.last_used_at),
            date_opt(r.expires_at),
        );
    }
    Ok(())
}

async fn revoke(pool: &PgPool, args: &[String]) -> anyhow::Result<()> {
    let target = args.first().context(USAGE)?;
    let id = match target.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => api_key_repo::id_by_prefix(pool, target)
            .await?
            .with_context(|| format!("no active API key matches `{target}`"))?,
    };
    api_key_repo::revoke(pool, id).await?;
    println!("revoked API key {id}");
    Ok(())
}

fn date_opt(at: Option<DateTime<Utc>>) -> String {
    at.map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn fit(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "api_key_test.rs"]
mod tests;
