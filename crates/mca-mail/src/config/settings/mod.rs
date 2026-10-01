//! Settings groups, one module per subsystem so each can be reviewed and
//! changed without touching the others.

mod api;
mod app;
mod database;
mod mail;
mod security;

pub use api::ApiSettings;
pub use app::{AppSettings, LogFormat};
pub use database::DatabaseSettings;
pub use mail::{ImapSettings, MailProviderKind, MailSettings, SmtpSettings, TlsMode};
pub use security::SecuritySettings;
