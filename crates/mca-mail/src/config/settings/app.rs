use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

/// Process-level settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub env: String,
    pub name: String,
    pub log_level: String,
    pub log_format: LogFormat,
    /// Directory the process may write non-essential state to.
    pub data_dir: String,
    /// Number of worker tasks processing the mail queue concurrently.
    pub processing_concurrency: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            env: "development".to_string(),
            name: "mca-mail".to_string(),
            log_level: "info".to_string(),
            log_format: LogFormat::Json,
            data_dir: "/var/lib/mca-mail".to_string(),
            processing_concurrency: 2,
        }
    }
}

impl AppSettings {
    pub fn is_production(&self) -> bool {
        matches!(self.env.as_str(), "production" | "prod")
    }
}
