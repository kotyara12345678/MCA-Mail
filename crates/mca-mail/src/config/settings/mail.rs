//! Mail transport settings.

use serde::{Deserialize, Serialize};

/// Which mail transport to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MailProviderKind {
    /// In-memory provider seeded from `fixtures/`. Never touches a real mailbox.
    #[default]
    Mock,
    /// IMAP for reading, SMTP for sending.
    Imap,
}

impl MailProviderKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            MailProviderKind::Mock => "mock",
            MailProviderKind::Imap => "imap",
        }
    }
}

/// TLS mode for the mail transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// TLS negotiated immediately (IMAPS 993, SMTPS 465).
    Implicit,
    /// Plain connect then `STARTTLS` (IMAP 143, SMTP 587).
    #[default]
    StartTls,
    /// No TLS. Refused at startup outside development.
    None,
}

impl TlsMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            TlsMode::Implicit => "implicit",
            TlsMode::StartTls => "starttls",
            TlsMode::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImapSettings {
    pub host: String,
    pub port: u16,
    pub tls: TlsMode,
    /// IMAP folder to read, provider specific (`INBOX`, `INBOX.MCA`).
    pub inbox: String,
    /// Accept self-signed certificates. Development only.
    pub allow_invalid_certs: bool,
    pub connect_timeout_seconds: u64,
    pub command_timeout_seconds: u64,
    /// Use IMAP IDLE when the server advertises it.
    pub use_idle: bool,
    /// Reconnect and re-issue schedule for the IDLE connection.
    pub idle: IdleSettings,
    /// IMAP namespace prefix used by some webmail providers.
    pub namespace_prefix: String,
}

impl Default for ImapSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 993,
            tls: TlsMode::Implicit,
            inbox: "INBOX".to_string(),
            allow_invalid_certs: false,
            connect_timeout_seconds: 20,
            command_timeout_seconds: 60,
            use_idle: false,
            idle: IdleSettings::default(),
            namespace_prefix: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub tls: TlsMode,
    /// Envelope sender used for bounces.
    pub from_address: String,
    pub from_name: String,
    /// Send a single `RCPT TO` per `send` call, keeping provider logs readable.
    pub batch_recipients: bool,
}

impl Default for SmtpSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            tls: TlsMode::StartTls,
            from_address: String::new(),
            from_name: "MCA Logistics".to_string(),
            batch_recipients: false,
        }
    }
}

#[path = "mail/idle.rs"]
mod idle;
#[path = "mail/root.rs"]
mod root;
pub use idle::IdleSettings;
pub use root::MailSettings;
