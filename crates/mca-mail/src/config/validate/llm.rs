use crate::config::{AppConfig, LlmProviderKind};
use crate::error::ConfigError;

pub fn check(config: &AppConfig) -> Result<(), ConfigError> {
    if config.llm.provider == LlmProviderKind::Mock || config.llm.disabled {
        return Ok(());
    }
    if config.llm.base_url.trim().is_empty() {
        return Err(ConfigError::Missing("LLM_BASE_URL".into()));
    }
    if config.llm.model.trim().is_empty() {
        return Err(ConfigError::Missing("LLM_MODEL".into()));
    }
    if !config.llm.api_key.is_present() {
        return Err(ConfigError::Missing("LLM_API_KEY".into()));
    }
    if !(0.0..=2.0).contains(&config.llm.temperature) {
        return Err(ConfigError::Invalid {
            field: "LLM_TEMPERATURE".into(),
            reason: "must be between 0.0 and 2.0".into(),
        });
    }
    if config.llm.max_tokens == 0 {
        return Err(ConfigError::Invalid {
            field: "LLM_MAX_TOKENS".into(),
            reason: "must be at least 1".into(),
        });
    }
    Ok(())
}
