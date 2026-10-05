use crate::config::{AppConfig, IdleSettings, MailProviderKind, TlsMode};
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
    // Read-only mode reads the mailbox, so credentials still travel over the wire
    // and TLS is exactly as mandatory as in `work`.
    if config.mail.imap.tls == TlsMode::None {
        return Err(ConfigError::Unsafe {
            mode: "MAIL_IMAP_TLS".into(),
            reason: "plaintext IMAP is refused: read-only mode still reads credentials".into(),
        });
    }
    if config.mail.imap.allow_invalid_certs && config.app.is_production() {
        return Err(ConfigError::Unsafe {
            mode: "MAIL_ALLOW_INVALID_CERTS".into(),
            reason: "disabled certificate validation is refused in production".into(),
        });
    }
    if config.mail.imap.use_idle {
        check_idle(config)?;
    }
    Ok(())
}

/// IDLE settings are only examined when IDLE is switched on: a stale value in a
/// disabled deployment must not stop the service from starting.
fn check_idle(config: &AppConfig) -> Result<(), ConfigError> {
    let idle = &config.mail.imap.idle;
    if !(60..=IdleSettings::MAX_WAIT_SECONDS).contains(&idle.wait_seconds) {
        return Err(ConfigError::Invalid {
            field: "MAIL_IDLE_WAIT_SECONDS".into(),
            reason: format!(
                "must be 60..={} so the connection is re-issued inside the RFC 2177 window",
                IdleSettings::MAX_WAIT_SECONDS
            ),
        });
    }
    if idle.reconnect_min_seconds == 0 || idle.reconnect_max_seconds < idle.reconnect_min_seconds {
        return Err(ConfigError::Invalid {
            field: "MAIL_IDLE_BACKOFF_MAX_SECONDS".into(),
            reason: "must be at least MAIL_IDLE_BACKOFF_MIN_SECONDS, which must not be 0".into(),
        });
    }
    if idle.jitter_percent > 100 {
        return Err(ConfigError::Invalid {
            field: "MAIL_IDLE_JITTER_PERCENT".into(),
            reason: "must be between 0 and 100".into(),
        });
    }
    if config.mail.idle_fallback_poll_seconds < 5 {
        return Err(ConfigError::Invalid {
            field: "MAIL_IDLE_FALLBACK_POLL_SECONDS".into(),
            reason: "must be at least 5; the fallback poller is a safety net, not a hot loop"
                .into(),
        });
    }
    Ok(())
}
