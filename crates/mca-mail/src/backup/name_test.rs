use super::*;
use chrono::{DateTime, Utc};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn round_trips_a_canonical_name() {
    let name = BackupName::new(at("2026-01-05T14:22:33Z"));
    let file_name = name.file_name();
    assert!(file_name.starts_with("mca-backup-20260105T142233Z-"));
    assert!(file_name.ends_with(".dump"));
    assert_eq!(BackupName::parse(&file_name), Some(name));
}

#[test]
fn equal_timestamps_still_produce_unique_names() {
    let at = at("2026-01-05T14:22:33Z");
    assert_ne!(
        BackupName::new(at).file_name(),
        BackupName::new(at).file_name()
    );
}

#[test]
fn ordering_matches_chronological_order() {
    let older = BackupName::new(at("2026-01-05T14:22:33Z")).file_name();
    let newer = BackupName::new(at("2026-01-06T09:00:00Z")).file_name();
    assert!(older < newer, "names must sort oldest first");
}

#[test]
fn temp_names_are_not_backups() {
    let name = BackupName::new(at("2026-01-05T14:22:33Z"));
    let temp = name.temp_file_name();
    assert!(temp.ends_with(TEMP_SUFFIX));
    assert!(BackupName::is_temp(&temp));
    assert!(!BackupName::is_complete(&temp));
    assert_eq!(BackupName::parse(&temp), None);
    assert!(!BackupName::is_temp("mca-backup-user-file.dump.partial"));
}

#[test]
fn foreign_and_damaged_names_are_rejected() {
    for bad in [
        "",
        "notes.txt",
        "mca-backup.dump",
        "mca-backup-20260105T142233Z-550e8400-e29b-41d4-a716-446655440000.sql",
        "mca-backup-20260105T142233.dump",
        "mca-backup-20260105X142233Z-550e8400-e29b-41d4-a716-446655440000.dump",
        "mca-backup-20261345T992233Z-550e8400-e29b-41d4-a716-446655440000.dump",
        "mca-backup-20260105T142233Z-550e8400-e29b-41d4-a716-446655440000.dump.bak",
        "../mca-backup-20260105T142233Z-550e8400-e29b-41d4-a716-446655440000.dump",
    ] {
        assert_eq!(BackupName::parse(bad), None, "{bad} must not parse");
    }
}
