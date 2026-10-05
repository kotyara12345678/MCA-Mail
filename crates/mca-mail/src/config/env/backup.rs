//! Named environment variable -> nested configuration path.
//!
//! The deployment contract is a flat `.env`, so the mapping is declared rather
//! than inferred. `BACKUP_*` and `MAIL_MODE` live in their own table because they
//! are the two settings an operator is most likely to reach for.

pub const BACKUP: &[(&str, &str)] = &[
    ("BACKUP_ENABLED", "backup.enabled"),
    ("BACKUP_DIR", "backup.dir"),
    ("BACKUP_INTERVAL_HOURS", "backup.interval_hours"),
    ("BACKUP_RETENTION_DAYS", "backup.retention_days"),
    ("BACKUP_RETENTION_WEEKS", "backup.retention_weeks"),
    ("BACKUP_MAX_SIZE_MB", "backup.max_size_mb"),
    (
        "BACKUP_COMMAND_TIMEOUT_SECONDS",
        "backup.command_timeout_seconds",
    ),
    ("BACKUP_RUN_ON_STARTUP", "backup.run_on_startup"),
];
