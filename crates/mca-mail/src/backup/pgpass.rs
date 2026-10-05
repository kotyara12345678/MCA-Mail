//! Keep DATABASE_URL credentials out of argv and logs.

use std::path::{Path, PathBuf};

use super::BackupError;

/// The `.pgpass` line without its password, i.e. the first four of libpq's
/// five fields: `host:port:database:username`.
///
/// `write_pgpass` appends `:{password}` to complete it. Six fields — a stray
/// `:*` left in here — is malformed and libpq skips the whole line, which
/// surfaces as `no password supplied` from `pg_dump` instead of a mismatch.
fn authority_prefix(url: &url::Url) -> String {
    let dbname = url.path().trim_start_matches('/');
    let dbname = if dbname.is_empty() { "*" } else { dbname };
    format!(
        "{}:{}:{}:{}",
        url.host_str().unwrap_or("localhost"),
        url.port().unwrap_or(5432),
        dbname,
        url.username()
    )
}

fn password_of(url: &url::Url) -> Result<&str, BackupError> {
    url.password()
        .filter(|password| !password.is_empty())
        .ok_or_else(|| BackupError::Config("DATABASE_URL carries no password".into()))
}

pub(crate) fn write_pgpass(dir: &Path, url: &url::Url) -> Result<PathBuf, BackupError> {
    let path = dir.join(".pgpass");
    let contents = format!("{}:{}\n", authority_prefix(url), password_of(url)?);
    write_private(&path, contents.as_bytes())?;
    Ok(path)
}

fn write_private(path: &Path, contents: &[u8]) -> Result<(), BackupError> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| BackupError::Io(format!("create {}: {error}", path.display())))?;
    let written = file.write_all(contents);
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(path);
        return Err(BackupError::Io(format!(
            "write {}: {error}",
            path.display()
        )));
    }
    if let Err(error) = restrict_permissions(path) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), BackupError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| BackupError::Io(format!("chmod {}: {error}", path.display())))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), BackupError> {
    Ok(())
}

pub(crate) struct PgPassGuard {
    path: Option<PathBuf>,
}

impl PgPassGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

impl Drop for PgPassGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(error = %error, "could not remove pgpass file");
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pgpass_tests.rs"]
mod tests;
