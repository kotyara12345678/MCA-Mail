use crate::config::{AppConfig, MailProviderKind, TlsMode};
use crate::error::ConfigError;

pub fn check(config: &AppConfig) -> Result<(), ConfigError> {
    // Order matters: refuse the mock provider in production *before* returning
    // early on it, otherwise a mock silently boots in production.
    if config.app.is_production() && config.mail.provider == MailProviderKind::Mock {
        return Err(ConfigError::Unsafe {
            mode: "APP_ENV".into(),
            reason: "MAIL_PROVIDER=mock is refused when APP_ENV=production".into(),
        });
    }
    if config.mail.provider == MailProviderKind::Mock {
        return Ok(());
    }
    for (field, value) in [
        ("MAIL_IMAP_HOST", &config.mail.imap.host),
        ("MAIL_SMTP_HOST", &config.mail.smtp.host),
        ("MAIL_USERNAME", &config.mail.username),
        ("MAIL_FROM_ADDRESS", &config.mail.smtp.from_address),
    ] {
        if value.trim().is_empty() {
            return Err(ConfigError::Missing(field.into()));
        }
    }
    if !config.mail.password.is_present() {
        return Err(ConfigError::Missing("MAIL_PASSWORD".into()));
    }
    if config.mail.imap.tls == TlsMode::None && config.app.is_production() {
        return Err(ConfigError::Unsafe {
            mode: "MAIL_IMAP_TLS".into(),
            reason: "plaintext IMAP is refused when APP_ENV=production".into(),
        });
    }
    if config.mail.imap.allow_invalid_certs && config.app.is_production() {
        return Err(ConfigError::Unsafe {
            mode: "MAIL_ALLOW_INVALID_CERTS".into(),
            reason: "disabled certificate validation is refused in production".into(),
        });
    }
    Ok(())
}
