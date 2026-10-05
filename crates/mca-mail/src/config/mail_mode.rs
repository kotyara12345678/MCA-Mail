//! Whether the agent may change mailbox state.
//!
//! `read_only` is the default and the only safe choice: the enum never has a
//! wildcard variant, so an unrecognised `.env` value cannot resolve to `Work`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MailMode {
    /// Read and analyse only. No SMTP, no IMAP APPEND, no flag or folder change.
    #[default]
    ReadOnly,
    /// Only non-destructive move/copy and safe flags are permitted.
    ReadWrite,
    /// Backwards-compatible alias for `ReadWrite`.
    Work,
}

impl MailMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            MailMode::ReadOnly => "read_only",
            MailMode::ReadWrite => "read_write",
            MailMode::Work => "work",
        }
    }

    /// Whether an operation that changes mailbox state may reach the network.
    pub const fn allows_mailbox_write(&self) -> bool {
        matches!(self, MailMode::ReadWrite | MailMode::Work)
    }

    /// Reject anything that is not a known mode.
    ///
    /// Used by the loader so a typo fails loudly instead of silently keeping or
    /// gaining write access.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "read_only" | "readonly" | "read-only" => Ok(MailMode::ReadOnly),
            "read_write" | "read-write" => Ok(MailMode::ReadWrite),
            "work" => Ok(MailMode::ReadWrite),
            other => Err(format!(
                "MAIL_MODE must be 'read_only' or 'read_write', got '{other}'"
            )),
        }
    }
}

impl std::fmt::Display for MailMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_read_only() {
        assert_eq!(MailMode::default(), MailMode::ReadOnly);
        assert!(!MailMode::default().allows_mailbox_write());
    }

    #[test]
    fn work_allows_mailbox_write() {
        assert!(MailMode::ReadWrite.allows_mailbox_write());
        assert!(MailMode::Work.allows_mailbox_write());
    }

    #[test]
    fn known_spellings_parse() {
        assert_eq!(MailMode::parse("read_only").unwrap(), MailMode::ReadOnly);
        assert_eq!(MailMode::parse("READ-ONLY").unwrap(), MailMode::ReadOnly);
        assert_eq!(
            MailMode::parse(" read_write ").unwrap(),
            MailMode::ReadWrite
        );
        assert_eq!(MailMode::parse(" work ").unwrap(), MailMode::ReadWrite);
    }

    #[test]
    fn empty_value_is_read_only_not_work() {
        assert_eq!(MailMode::parse("").unwrap(), MailMode::ReadOnly);
    }

    #[test]
    fn unknown_value_is_rejected_and_never_becomes_work() {
        for bad in ["write", "auto", "true", "1", "work_mode", "dry_run"] {
            assert!(MailMode::parse(bad).is_err(), "{bad} must not parse");
        }
    }

    #[test]
    fn mode_survives_config_serialization_and_invalid_values_fail() {
        let mode = MailMode::Work;
        let serialized = serde_json::to_string(&mode).unwrap();
        assert_eq!(serde_json::from_str::<MailMode>(&serialized).unwrap(), mode);
        assert!(serde_json::from_str::<MailMode>("\"write\"").is_err());
    }
}
