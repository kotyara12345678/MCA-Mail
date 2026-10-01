//! Pure argument parsing for `api-key create`, kept free of I/O so it can be
//! unit-tested without a database.

use anyhow::{bail, Context};

use crate::persistence::api_key_repo::Role;

const CREATE_USAGE: &str =
    "usage: mca-mail api-key create --name <name> --role <role> [--expires-days <n>] [--created-by <who>]";

#[derive(Debug)]
pub struct CreateArgs {
    pub name: String,
    pub role: Role,
    pub expires_days: Option<u32>,
    pub created_by: String,
}

pub fn parse_create(args: &[String]) -> anyhow::Result<CreateArgs> {
    let mut name = None;
    let mut role = None;
    let mut expires_days = None;
    let mut created_by = "cli".to_string();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--name" => name = Some(next_value(&mut it, arg)?),
            "--role" => {
                let raw = next_value(&mut it, arg)?;
                role = Some(raw.parse::<Role>().map_err(|_| {
                    anyhow::anyhow!("invalid role `{raw}` (viewer|operator|manager|admin)")
                })?);
            }
            "--expires-days" => {
                let raw = next_value(&mut it, arg)?;
                let days = raw
                    .parse::<u32>()
                    .context("--expires-days must be a whole number")?;
                expires_days = Some(days);
            }
            "--created-by" => created_by = next_value(&mut it, arg)?,
            other => bail!("unexpected argument `{other}`\n\n{CREATE_USAGE}"),
        }
    }

    let name = name
        .filter(|s| !s.trim().is_empty())
        .context("--name is required")?;
    let role = role.context("--role is required")?;
    Ok(CreateArgs {
        name: name.trim().to_string(),
        role,
        expires_days,
        created_by,
    })
}

fn next_value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> anyhow::Result<String> {
    it.next()
        .map(|s| s.to_string())
        .with_context(|| format!("`{flag}` requires a value"))
}
