use super::{directory, name::BackupName, retention::RetentionPolicy};
use std::path::{Path, PathBuf};

#[path = "directory_tests.rs"]
mod directory_tests;
#[path = "service_tests.rs"]
mod service_tests;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mca-backup-test-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_backup(dir: &Path, days_ago: i64) -> PathBuf {
    let name = BackupName::new(chrono::Utc::now() - chrono::Duration::days(days_ago));
    let path = dir.join(name.file_name());
    std::fs::write(&path, b"PGDMP fake custom-format dump").expect("seed");
    path
}

fn rotate(dir: &Path, days: u32, weeks: u32) -> Vec<String> {
    let policy = RetentionPolicy::new(days, weeks);
    let backups = directory::scan(dir).expect("scan").backups;
    policy
        .select(&backups, chrono::Utc::now())
        .into_iter()
        .filter_map(|backup| {
            let name = backup.file_name();
            std::fs::remove_file(dir.join(&name)).ok().map(|_| name)
        })
        .collect()
}
