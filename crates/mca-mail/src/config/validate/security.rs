use crate::config::AppConfig;
use crate::error::ConfigError;

pub fn check(config: &AppConfig) -> Result<(), ConfigError> {
    let mode = config.security.email_mode;
    if config.security.outbound.auto_send && !mode.allows_sending() {
        return Err(ConfigError::Unsafe {
            mode: "EMAIL_AUTO_SEND".into(),
            reason: format!(
                "auto send requires EMAIL_MODE=auto, but EMAIL_MODE={} forbids sending",
                mode.as_str()
            ),
        });
    }
    if config.app.is_production() && config.security.attachments.allow_outbound_attachments {
        return Err(ConfigError::Unsafe {
            mode: "EMAIL_ALLOW_OUTBOUND_ATTACHMENTS".into(),
            reason: "the agent must not originate attachments in production".into(),
        });
    }
    if config.security.outbound.max_sends_per_hour == 0 {
        return Err(ConfigError::Invalid {
            field: "EMAIL_MAX_SENDS_PER_HOUR".into(),
            reason: "must be at least 1".into(),
        });
    }
    if config.security.outbound.max_sends_per_lead_per_hour
        > config.security.outbound.max_sends_per_hour
    {
        return Err(ConfigError::Invalid {
            field: "EMAIL_MAX_SENDS_PER_LEAD_PER_HOUR".into(),
            reason: "must not exceed the global hourly limit".into(),
        });
    }
    if config.security.inbound.max_llm_body_chars == 0 {
        return Err(ConfigError::Invalid {
            field: "MAX_LLM_BODY_CHARS".into(),
            reason: "must be at least 1".into(),
        });
    }
    if config.security.max_cost_micros_per_task > config.security.max_cost_micros_per_day {
        return Err(ConfigError::Invalid {
            field: "MAX_COST_MICROS_PER_TASK".into(),
            reason: "must not exceed MAX_COST_MICROS_PER_DAY".into(),
        });
    }
    if config.security.max_cost_micros_per_hour > config.security.max_cost_micros_per_day {
        return Err(ConfigError::Invalid {
            field: "MAX_COST_MICROS_PER_HOUR".into(),
            reason: "must not exceed MAX_COST_MICROS_PER_DAY".into(),
        });
    }
    let card = &config.security.manager_card;
    if card.enabled && card.recipients().is_empty() {
        return Err(ConfigError::Invalid {
            field: "MANAGER_CARD_RECIPIENT".into(),
            reason: "SEND_MANAGER_CARD=true needs at least one recipient address".into(),
        });
    }
    if card.enabled && card.max_per_hour == 0 {
        return Err(ConfigError::Invalid {
            field: "MANAGER_CARD_MAX_PER_HOUR".into(),
            reason: "must be at least 1".into(),
        });
    }
    Ok(())
}
