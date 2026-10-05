use chrono::{Duration, Utc};

use super::name::BackupName;
use super::retention::RetentionPolicy;

fn backup(days_ago: i64) -> BackupName {
    BackupName::new(Utc::now() - Duration::days(days_ago))
}

fn names(n: i64) -> Vec<BackupName> {
    (0..n).map(backup).collect()
}

#[test]
fn recent_backups_are_kept() {
    let selected = RetentionPolicy::new(7, 4).select(&names(5), Utc::now());
    assert!(selected.is_empty());
}

#[test]
fn dailies_beyond_the_window_are_dropped() {
    let now = Utc::now();
    let selected = RetentionPolicy::new(7, 0).select(&names(20), now);
    assert_eq!(selected.len(), 13);
    assert!(selected
        .iter()
        .all(|b| { b.created_at.date_naive() < now.date_naive() - Duration::days(6) }));
}

#[test]
fn weeklies_extend_retention_without_new_files() {
    let selected = RetentionPolicy::new(7, 4).select(&names(60), Utc::now());
    assert!(selected.len() <= 60 - 7 - 4);
    assert!(selected.len() >= 60 - 7 - 8);
}

#[test]
fn the_newest_backup_is_never_deleted() {
    let selected = RetentionPolicy::new(1, 0).select(&[backup(500)], Utc::now());
    assert!(selected.is_empty());
}

#[test]
fn an_empty_directory_deletes_nothing() {
    assert!(RetentionPolicy::new(7, 4)
        .select(&[], Utc::now())
        .is_empty());
}

#[test]
fn the_daily_window_boundary_is_inclusive() {
    let now = Utc::now();
    let exact = BackupName::new(now - Duration::days(7));
    assert!(RetentionPolicy::new(7, 0)
        .select(std::slice::from_ref(&exact), now)
        .is_empty());
}
