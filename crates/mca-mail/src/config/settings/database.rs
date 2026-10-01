use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseSettings {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_seconds: u64,
    pub acquire_timeout_seconds: u64,
    /// Run pending migrations during startup.
    pub auto_migrate: bool,
    /// Statement timeout, protects against a runaway analytics query.
    pub statement_timeout_seconds: u64,
}

impl Default for DatabaseSettings {
    /// Hand-written rather than derived: a derived `Default` would leave
    /// `max_connections` at 0, which validation rejects, so every caller that
    /// overrode only the URL would end up with an unusable pool config.
    fn default() -> Self {
        Self {
            url: String::new(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout_seconds: 10,
            acquire_timeout_seconds: 15,
            auto_migrate: true,
            statement_timeout_seconds: 30,
        }
    }
}

impl DatabaseSettings {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_seconds.max(1))
    }
    pub fn acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.acquire_timeout_seconds.max(1))
    }
}
