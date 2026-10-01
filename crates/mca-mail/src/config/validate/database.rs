use crate::config::AppConfig;
use crate::error::ConfigError;

pub fn check(config: &AppConfig) -> Result<(), ConfigError> {
    if config.database.url.trim().is_empty() {
        return Err(ConfigError::Missing("DATABASE_URL".into()));
    }
    if !config.database.url.starts_with("postgres://")
        && !config.database.url.starts_with("postgresql://")
    {
        return Err(ConfigError::Invalid {
            field: "DATABASE_URL".into(),
            reason: "expected a postgres:// or postgresql:// URL".into(),
        });
    }
    if config.database.max_connections < 1 {
        return Err(ConfigError::Invalid {
            field: "DATABASE_MAX_CONNECTIONS".into(),
            reason: "must be at least 1".into(),
        });
    }
    if config.database.min_connections > config.database.max_connections {
        return Err(ConfigError::Invalid {
            field: "DATABASE_MIN_CONNECTIONS".into(),
            reason: "must not exceed DATABASE_MAX_CONNECTIONS".into(),
        });
    }
    Ok(())
}
