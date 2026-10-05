//! Retention policy: keep N recent dailies, plus a weekly tail.
//!
//! Weeklies are not separate files. A dump older than the daily window is
//! promoted by keeping the newest one per ISO week, so the weekly count costs no
//! extra storage and cannot drift out of sync with the dailies.

use chrono::{DateTime, Datelike, Duration, Utc};
use std::collections::BTreeSet;

use super::name::BackupName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub days: u32,
    pub weeks: u32,
    /// The newest backup is never a candidate for deletion, whatever the policy.
    pub keep_newest: bool,
}

impl RetentionPolicy {
    pub fn new(days: u32, weeks: u32) -> Self {
        // At least one day is always kept: a policy that could delete everything
        // is a configuration error, caught separately in validation.
        Self {
            days: days.max(1),
            weeks,
            keep_newest: true,
        }
    }

    /// Decide what to delete, given the backups that exist and `now`.
    ///
    /// `candidates` are the parsed backups. Pure and total: it performs no I/O,
    /// which is what makes the boundary cases testable.
    pub fn select(&self, candidates: &[BackupName], now: DateTime<Utc>) -> Vec<BackupName> {
        let mut ordered: Vec<BackupName> = candidates.to_vec();
        ordered.sort_by_key(|backup| (backup.created_at, backup.file_name()));

        let keep = self.kept_refs(&ordered, now);
        ordered
            .into_iter()
            .filter(|backup| !keep.contains(&backup.file_name()))
            .collect()
    }

    fn kept_refs(&self, ordered: &[BackupName], now: DateTime<Utc>) -> Vec<String> {
        let mut keep: Vec<String> = Vec::new();
        let daily_cutoff = now.date_naive() - Duration::days(i64::from(self.days - 1));
        let mut seen_days = BTreeSet::new();

        for backup in ordered.iter().rev() {
            let day = backup.created_at.date_naive();
            if day >= daily_cutoff && seen_days.insert(day) {
                keep.push(backup.file_name());
            }
        }

        if self.weeks > 0 {
            let weekly_cutoff = daily_cutoff - Duration::weeks(i64::from(self.weeks));
            // Newest first, so the first backup seen in a week is that week's keeper.
            let mut seen_weeks: BTreeSet<(i32, u32)> = BTreeSet::new();
            for backup in ordered.iter().rev() {
                let day = backup.created_at.date_naive();
                if day < weekly_cutoff {
                    break;
                }
                if day >= daily_cutoff {
                    continue;
                }
                let week = backup.created_at.iso_week();
                if seen_weeks.insert((week.year(), week.week())) {
                    keep.push(backup.file_name());
                    if seen_weeks.len() >= self.weeks as usize {
                        break;
                    }
                }
            }
        }

        if self.keep_newest {
            if let Some(newest) = ordered.last() {
                let file_name = newest.file_name();
                if !keep.contains(&file_name) {
                    keep.push(file_name);
                }
            }
        }
        keep
    }
}
