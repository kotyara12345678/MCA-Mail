//! Spawning the PostgreSQL client tools.
//!
//! Separate from `runner` because this is the only place that touches a process
//! at all: a timeout, a kill and a bounded read of stderr are one concern, and
//! keeping them together is what makes the safety properties auditable.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::error::BackupError;

/// How often the child is checked for exit while the deadline is pending.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Upper bound on captured stderr, so a runaway tool cannot exhaust memory.
const MAX_STDERR_BYTES: usize = 4096;

/// Run `program` to completion, or fail with a timeout.
pub fn run(
    program: &str,
    args: &[String],
    pgpass: &Path,
    timeout_secs: u64,
) -> Result<(), BackupError> {
    let mut child = command(program, args, pgpass)
        .spawn()
        .map_err(|e| spawn_error(program, e))?;

    let status = wait_within(&mut child, timeout_secs, program)?;

    if !status.success() {
        return Err(BackupError::ToolFailed {
            code: status.code().unwrap_or(-1),
            stderr: read_stderr(&mut child),
        });
    }
    Ok(())
}

fn command(program: &str, args: &[String], pgpass: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        // `PGPASSFILE` is how the password reaches the child without ever
        // appearing in argv or in the environment of the parent process.
        .env("PGPASSFILE", pgpass)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn spawn_error(program: &str, e: std::io::Error) -> BackupError {
    if e.kind() == std::io::ErrorKind::NotFound {
        BackupError::ToolMissing(format!(
            "{program} is not installed or not on PATH; install the PostgreSQL client tools"
        ))
    } else {
        BackupError::Io(format!("spawn {program}: {e}"))
    }
}

/// Poll rather than block, so a hung child is killed on schedule. The poll
/// interval is short relative to the timeout, so a dump that finishes is not
/// delayed by more than that.
fn wait_within(
    child: &mut std::process::Child,
    timeout_secs: u64,
    program: &str,
) -> Result<std::process::ExitStatus, BackupError> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            // A killed child may leave a partial file, so the caller removes it.
            Ok(None) => return Err(kill(child, timeout_secs)),
            Err(e) => return Err(BackupError::Io(format!("wait {program}: {e}"))),
        }
    }
}

fn kill(child: &mut std::process::Child, timeout_secs: u64) -> BackupError {
    let _ = child.kill();
    let _ = child.wait();
    BackupError::Timeout(timeout_secs)
}

/// `pg_dump` does not echo the password in its diagnostics: it was never in
/// argv, and the parent env does not carry it either.
fn read_stderr(child: &mut std::process::Child) -> String {
    use std::io::Read;

    let Some(mut pipe) = child.stderr.take() else {
        return String::new();
    };
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    buf.truncate(MAX_STDERR_BYTES);
    String::from_utf8_lossy(&buf).trim().to_string()
}
