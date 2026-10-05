//! Shared helpers for the backup contract tests.

use std::path::{Path, PathBuf};

use mca_mail::config::BackupSettings;

/// Whether the PostgreSQL client tools this suite depends on are on `PATH`.
///
/// `pg_dump` and `pg_restore` are external binaries, not library calls, so a
/// green run without them would prove nothing about the thing an operator relies
/// on after a server loss.
pub fn tools_available() -> bool {
    which("pg_dump") && which("pg_restore")
}

/// Look for a tool on `PATH` without spawning it.
pub fn which(program: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    let separator = if cfg!(windows) { ';' } else { ':' };
    path.split(separator).filter(|d| !d.is_empty()).any(|dir| {
        exts.iter()
            .any(|ext| Path::new(dir).join(format!("{program}{ext}")).is_file())
    })
}

/// A scratch directory, created, and named per test so suites can share a
/// database and a temp root.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = crate::support::temp_dir(&format!("backup-it-{tag}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Enabled settings pointing at `dir`, with a short command timeout so a hung
/// `pg_dump` fails the test rather than the suite.
pub fn settings(dir: PathBuf) -> BackupSettings {
    BackupSettings {
        enabled: true,
        dir,
        interval_hours: 6,
        retention_days: 7,
        retention_weeks: 4,
        max_size_mb: 1024,
        command_timeout_seconds: 120,
        run_on_startup: true,
    }
}

/// Skip a test that needs the client tools, saying so on stderr so a skipped
/// run is never mistaken for a passing one.
pub fn require_tools() -> bool {
    let present = tools_available();
    if !present {
        eprintln!("skipped: pg_dump/pg_restore are not on PATH");
    }
    present
}
