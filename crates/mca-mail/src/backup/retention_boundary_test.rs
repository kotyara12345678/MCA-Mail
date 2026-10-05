use chrono::Utc;

use super::name::BackupName;
use super::retention::RetentionPolicy;

fn at(text: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn only_the_newest_backup_per_utc_day_is_kept() {
    let candidates = [
        at("2026-01-15T02:00:00Z"),
        at("2026-01-15T06:00:00Z"),
        at("2026-01-14T04:00:00Z"),
        at("2026-01-14T08:00:00Z"),
    ]
    .map(BackupName::new);
    let removed = RetentionPolicy::new(7, 0).select(&candidates, at("2026-01-15T12:00:00Z"));
    assert_eq!(removed.len(), 2);
    assert!(removed
        .iter()
        .any(|b| b.created_at == candidates[0].created_at));
    assert!(removed
        .iter()
        .any(|b| b.created_at == candidates[2].created_at));
}

#[test]
fn unique_names_with_the_same_timestamp_do_not_both_survive() {
    let now = Utc::now();
    let candidates = [BackupName::new(now), BackupName::new(now)];
    assert_eq!(RetentionPolicy::new(7, 0).select(&candidates, now).len(), 1);
}

#[test]
fn weekly_points_cross_iso_year_boundaries() {
    let now = at("2026-01-12T12:00:00Z");
    let candidates: Vec<_> = [
        "2026-01-12T08:00:00Z",
        "2026-01-11T08:00:00Z",
        "2026-01-10T08:00:00Z",
        "2026-01-05T08:00:00Z",
        "2026-01-04T08:00:00Z",
        "2025-12-29T08:00:00Z",
        "2025-12-28T08:00:00Z",
    ]
    .iter()
    .map(|text| BackupName::new(at(text)))
    .collect();
    let removed = RetentionPolicy::new(2, 3).select(&candidates, now);
    assert_eq!(removed.len(), 2);
    assert!(removed
        .iter()
        .any(|b| b.created_at == candidates[3].created_at));
    assert!(removed
        .iter()
        .any(|b| b.created_at == candidates[5].created_at));
}
