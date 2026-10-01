//! Root mail settings and the derived durations the transport uses.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{ImapSettings, MailProviderKind, SmtpSettings};
use crate::config::Secret;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailSettings {
    pub provider: MailProviderKind,
    pub username: String,
    pub password: Secret,
    pub poll_interval_seconds: u64,
    /// Number of messages fetched per poll cycle.
    pub fetch_batch_size: usize,
    /// Give up and requeue a message after this many failed attempts.
    pub max_attempts: i32,
    /// Exponential backoff base for retries.
    pub retry_backoff_seconds: u64,
    /// Reconnect after this many consecutive connection errors.
    pub reconnect_after_errors: u32,
    pub imap: ImapSettings,
    pub smtp: SmtpSettings,
    /// Directory the mock provider reads its corpus from.
    pub mock_corpus_dir: String,
}

impl Default for MailSettings {
    fn default() -> Self {
        Self {
            provider: MailProviderKind::Mock,
            username: String::new(),
            password: Secret::empty(),
            poll_interval_seconds: 300,
            fetch_batch_size: 50,
            max_attempts: 5,
            retry_backoff_seconds: 30,
            reconnect_after_errors: 3,
            imap: ImapSettings::default(),
            smtp: SmtpSettings::default(),
            mock_corpus_dir: "fixtures/emails".to_string(),
        }
    }
}

impl MailSettings {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_seconds.max(5))
    }
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.imap.connect_timeout_seconds.max(1))
    }
    pub fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.imap.command_timeout_seconds.max(1))
    }
    pub fn retry_backoff(&self) -> Duration {
        Duration::from_secs(self.retry_backoff_seconds.max(1))
    }
    /// Folder actually read, guarding against an empty configured value.
    pub fn inbox(&self) -> &str {
        let trimmed = self.imap.inbox.trim();
        if trimmed.is_empty() {
            "INBOX"
        } else {
            trimmed
        }
    }
}
