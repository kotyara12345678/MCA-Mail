//! Non-secret views of the configuration, safe to log or expose over HTTP.

use crate::config::AppConfig;

/// Turn a figment error into a message without dumping the whole environment.
pub fn describe(raw: &str) -> String {
    raw.lines()
        .filter(|line| !line.contains("API_KEY") && !line.contains("PASSWORD"))
        .take(6)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Startup banner and `/api/v1/settings` body.
///
/// Contains no secret material by construction: it reports *which* provider is
/// configured and whether a key is present, never the key itself.
pub fn redacted(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "app_env": config.app.env,
        "log_level": config.app.log_level,
        "mail_provider": config.mail.provider.as_str(),
        "mail_poll_interval_seconds": config.mail.poll_interval_seconds,
        "llm_provider": config.llm.provider.as_str(),
        "llm_model": config.llm.model,
        "llm_api_key_present": config.llm.api_key.is_present(),
        "email_mode": config.security.email_mode.as_str(),
        "email_auto_send": config.security.outbound.auto_send,
        "prompts_dir": config.security.prompts_dir,
        "company_research_configured": config.research.provider_configured(),
        "retention_days": config.retention.email_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmSettings, MailSettings, Secret};

    #[test]
    fn redacted_summary_never_contains_secrets() {
        let config = AppConfig {
            llm: LlmSettings {
                api_key: Secret::new("sk-super-secret-value"),
                model: "m".into(),
                base_url: "https://api.example.com/v1".into(),
                ..Default::default()
            },
            mail: MailSettings {
                password: Secret::new("mail-secret"),
                ..Default::default()
            },
            ..AppConfig::default()
        };
        let rendered = redacted(&config).to_string();
        assert!(!rendered.contains("sk-super-secret-value"));
        assert!(!rendered.contains("mail-secret"));
    }

    #[test]
    fn describe_filters_secret_named_lines() {
        let raw = "LLM_API_KEY=abc\nDATABASE_URL=postgres\nPASSWORD=hunter2\nother";
        let out = describe(raw);
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("API_KEY"));
        assert!(out.contains("DATABASE_URL"));
    }
}
