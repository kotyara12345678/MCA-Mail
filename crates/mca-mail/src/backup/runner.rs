//! Turning a `DATABASE_URL` into `pg_dump` / `pg_restore` invocations.
//!
//! Both are external processes, so they are called from `spawn_blocking` rather
//! than run on the async runtime's worker threads: a dump is CPU- and IO-heavy
//! and would otherwise stall the mail poll loop for its whole duration.

use std::path::Path;

use super::error::BackupError;
use super::pgpass::{write_pgpass, PgPassGuard};
use super::process;

/// Connection details, split so no single argument carries the password.
pub struct Connection<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub dbname: &'a str,
}

/// The argument vector a dump would be given, with the password absent.
///
/// Exposed so the integration suite can assert the property that matters most
/// for credential safety without having to spawn a process and inspect it.
pub fn connection_arguments(url: &url::Url) -> Result<Vec<String>, BackupError> {
    Ok(connection_args(&parse_url(url)?))
}

/// Split a `DATABASE_URL` into its parts. The password is not returned; it goes
/// into the `.pgpass` file instead.
pub fn parse_url(url: &url::Url) -> Result<Connection<'_>, BackupError> {
    let dbname = url.path().trim_start_matches('/');
    if dbname.is_empty() {
        return Err(BackupError::Config(
            "DATABASE_URL has no database name".into(),
        ));
    }
    Ok(Connection {
        host: url.host_str().unwrap_or("localhost"),
        port: url.port().unwrap_or(5432),
        user: url.username(),
        dbname,
    })
}

/// The flags every connection shares.
///
/// `--no-password` is the important one: it makes the tool fail instead of
/// prompting, so a credential problem surfaces as an error instead of a process
/// that hangs until the timeout.
fn connection_args(conn: &Connection<'_>) -> Vec<String> {
    [
        "--host",
        conn.host,
        "--port",
        &conn.port.to_string(),
        "--username",
        conn.user,
        "--dbname",
        conn.dbname,
        "--no-password",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Take a dump in PostgreSQL custom format to `target`.
///
/// The `.pgpass` guard is dropped when this returns, on both paths: the
/// credential file must not outlive the process that was given it.
pub fn pg_dump(
    url: &url::Url,
    dir: &Path,
    target: &Path,
    timeout_secs: u64,
) -> Result<(), BackupError> {
    let conn = parse_url(url)?;
    let pgpass = write_pgpass(dir, url)?;
    let _guard = PgPassGuard::new(pgpass.clone());

    let mut args = connection_args(&conn);
    args.extend([
        "--format=custom".to_string(),
        "--no-owner".to_string(),
        "--no-acl".to_string(),
        "--file".to_string(),
        target.display().to_string(),
    ]);
    process::run("pg_dump", &args, &pgpass, timeout_secs)
}

/// Validate a dump by asking `pg_restore` to read its table of contents.
///
/// A file that cannot be listed cannot be restored, so this is the minimum bar
/// for calling a backup successful.
pub fn pg_restore_list(
    url: &url::Url,
    dir: &Path,
    dump: &Path,
    timeout_secs: u64,
) -> Result<(), BackupError> {
    let conn = parse_url(url)?;
    let pgpass = write_pgpass(dir, url)?;
    let _guard = PgPassGuard::new(pgpass.clone());

    let args = restore_list_args(&conn, dump);
    process::run("pg_restore", &args, &pgpass, timeout_secs)
}

fn restore_list_args(conn: &Connection<'_>, dump: &Path) -> Vec<String> {
    let mut args = connection_args(conn);
    args.push("--list".to_string());
    args.push(dump.display().to_string());
    args
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
