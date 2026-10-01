use crate::config::AppConfig;
use crate::error::ConfigError;

pub fn check(config: &AppConfig) -> Result<(), ConfigError> {
    let agents = &config.agents;
    if agents.max_iterations == 0 || agents.max_iterations > 100 {
        return Err(ConfigError::Invalid {
            field: "AGENT_MAX_ITERATIONS".into(),
            reason: "must be between 1 and 100".into(),
        });
    }
    if agents.task_timeout_seconds < 5 {
        return Err(ConfigError::Invalid {
            field: "AGENT_TASK_TIMEOUT_SECONDS".into(),
            reason: "must be at least 5 seconds".into(),
        });
    }
    if agents.tools.max_calls_per_task == 0 {
        return Err(ConfigError::Invalid {
            field: "AGENT_MAX_TOOL_CALLS_PER_TASK".into(),
            reason: "must be at least 1".into(),
        });
    }
    if agents.tools.max_argument_bytes == 0 || agents.tools.max_result_bytes == 0 {
        return Err(ConfigError::Invalid {
            field: "AGENT_TOOL_CALL_TIMEOUT_SECONDS".into(),
            reason: "tool argument and result caps must be greater than zero".into(),
        });
    }
    if config.app.processing_concurrency == 0 {
        return Err(ConfigError::Invalid {
            field: "PROCESSING_CONCURRENCY".into(),
            reason: "must be at least 1".into(),
        });
    }
    Ok(())
}
