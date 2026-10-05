//! Root mail settings and the derived durations the transport uses.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{ImapSettings, MailProviderKind, SmtpSettings};
use crate::config::Secret;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailSettings {
    /// Whether mailbox mutations are permitted. Enforced in the mail layer, not
    /// by prompting.
    pub mode: crate::config::MailMode,
    pub provider: MailProviderKind,
    pub username: String,
    pub password: Secret,
    pub poll_interval_seconds: u64,
    /// Fallback poll interval used while IDLE is watching the mailbox.
    ///
    /// IDLE reacts in seconds, so the safety net behind it does not need to be
    /// quick — it only has to be regular and cheap. Defaults to 90 seconds.
    pub idle_fallback_poll_seconds: u64,
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
    /// Exact folder name a convicted message is moved into, spelled the way the
    /// server lists it. Empty falls back to the SPECIAL-USE `\Junk` role, which
    /// not every server advertises.
    pub spam_folder: String,
    /// Exact folder a delivered message is `APPEND`ed to, spelled the way the
    /// server lists it. Empty falls back to the SPECIAL-USE `\Sent` role. An
    /// empty value for both this and a missing role disables archiving.
    pub sent_folder: String,
}

impl Default for MailSettings {
    fn default() -> Self {
        Self {
            mode: crate::config::MailMode::ReadOnly,
            provider: MailProviderKind::Mock,
            username: String::new(),
            password: Secret::empty(),
            poll_interval_seconds: 300,
            idle_fallback_poll_seconds: 90,
            fetch_batch_size: 50,
            max_attempts: 5,
            retry_backoff_seconds: 30,
            reconnect_after_errors: 3,
            imap: ImapSettings::default(),
            smtp: SmtpSettings::default(),
            mock_corpus_dir: "fixtures/emails".to_string(),
            spam_folder: String::new(),
            sent_folder: String::new(),
        }
    }
}

impl MailSettings {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_seconds.max(5))
    }
    /// Interval of the safety-net poller that runs alongside IDLE.
    pub fn idle_fallback_interval(&self) -> Duration {
        Duration::from_secs(self.idle_fallback_poll_seconds.clamp(5, 3600))
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
    /// Destination of a quarantine move, or `None` when no folder is configured
    /// and the Spam role has to be resolved from the server's SPECIAL-USE
    /// flags instead.
    pub fn quarantine_folder(&self) -> Option<&str> {
        let name = self.spam_folder.trim();
        (!name.is_empty()).then_some(name)
    }
    /// Folder a delivered message is archived into, or `None` when it must be
    /// resolved from the server's SPECIAL-USE `\Sent` flags instead.
    pub fn sent_folder(&self) -> Option<&str> {
        let name = self.sent_folder.trim();
        (!name.is_empty()).then_some(name)
    }
}
