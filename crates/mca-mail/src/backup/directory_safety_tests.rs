use super::super::*;

#[test]
fn a_missing_directory_is_empty() {
    let missing = temp_dir("missing").join("absent");
    assert!(directory::scan(&missing).unwrap().is_empty());
}

#[test]
fn foreign_files_are_never_considered_backups() {
    let dir = temp_dir("foreign");
    for name in ["notes.txt", "dump.tar.gz", "mca-backup.dump", "README"] {
        std::fs::write(dir.join(name), b"user data").unwrap();
    }
    write_backup(&dir, 0);
    let listing = directory::scan(&dir).unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing.foreign.len(), 4);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn temp_files_are_separate_from_completed_backups() {
    let dir = temp_dir("temps");
    let name = BackupName::new(chrono::Utc::now());
    std::fs::write(dir.join(name.temp_file_name()), b"partial").unwrap();
    std::fs::write(dir.join(name.file_name()), b"complete").unwrap();
    let listing = directory::scan(&dir).unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing.stale_temp_files.len(), 1);
    std::fs::remove_dir_all(dir).ok();
}

#[cfg(unix)]
#[test]
fn symlinked_backup_directory_is_rejected() {
    let base = temp_dir("symlink-dir");
    let target = base.join("target");
    let alias = base.join("alias");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert!(directory::ensure_dir(&alias).is_err());
    assert!(directory::scan(&alias).is_err());
    assert!(std::fs::read_dir(&target).unwrap().next().is_none());
    std::fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn backup_named_symlink_cannot_delete_external_file() {
    let base = temp_dir("symlink-entry");
    let dir = base.join("backups");
    let victim = base.join("user-data.txt");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(&victim, b"preserve me").unwrap();
    let link = dir.join(BackupName::new(chrono::Utc::now()).file_name());
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    assert_eq!(directory::scan(&dir).unwrap().len(), 0);
    let _ = rotate(&dir, 1, 0);
    assert_eq!(std::fs::read(&victim).unwrap(), b"preserve me");
    std::fs::remove_dir_all(base).ok();
}
