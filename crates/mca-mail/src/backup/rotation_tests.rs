use super::super::*;

#[test]
fn rotation_keeps_newest_and_foreign_files() {
    let dir = temp_dir("rotate");
    let newest = write_backup(&dir, 0);
    let foreign = dir.join("customer-export.csv");
    std::fs::write(&foreign, b"user data").unwrap();
    for age in [3, 10, 20, 40, 90] {
        write_backup(&dir, age);
    }
    assert!(!rotate(&dir, 3, 0).is_empty());
    assert!(newest.exists());
    assert!(foreign.exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn newest_backup_survives_and_weeklies_extend_retention() {
    let dir = temp_dir("week");
    let only = write_backup(&dir, 365);
    assert!(rotate(&dir, 1, 0).is_empty());
    assert!(only.exists());
    for age in 0..10 {
        write_backup(&dir, age);
    }
    let removed = rotate(&dir, 3, 4);
    let remaining = directory::scan(&dir).unwrap().len();
    assert!((3..=7).contains(&remaining));
    assert_eq!(remaining + removed.len(), 11);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn ensure_dir_creates_a_usable_directory() {
    let base = temp_dir("ensure");
    let target = base.join("nested").join("backups");
    assert!(directory::ensure_dir(&target).is_ok());
    assert!(target.is_dir());
    assert!(directory::ensure_dir(&target).is_ok());
    std::fs::remove_dir_all(base).ok();
}
