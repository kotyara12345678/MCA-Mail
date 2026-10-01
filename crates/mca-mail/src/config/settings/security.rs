use serde::{Deserialize, Serialize};

use crate::config::security::{
    AttachmentPolicy, ContextPolicy, EmailMode, InboundPolicy, OutboundPolicy,
};

/// Where every guard rail is configured, in one place, so the API can report the
/// effective policy and an operator can see what the system believes it may do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecuritySettings {
    pub email_mode: EmailMode,
    pub outbound: OutboundPolicy,
    pub inbound: InboundPolicy,
    pub attachments: AttachmentPolicy,
    pub context: ContextPolicy,
    /// Directory holding `*.md` prompt files.
    pub prompts_dir: String,
    /// Per-task USD ceiling expressed in micro-dollars.
    pub max_cost_micros_per_task: i64,
    /// Global hourly USD ceiling expressed in micro-dollars.
    pub max_cost_micros_per_hour: i64,
    pub max_cost_micros_per_day: i64,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            email_mode: EmailMode::default(),
            outbound: OutboundPolicy::default(),
            inbound: InboundPolicy::default(),
            attachments: AttachmentPolicy::default(),
            context: ContextPolicy::default(),
            prompts_dir: "prompts".to_string(),
            max_cost_micros_per_task: 500_000,
            max_cost_micros_per_hour: 5_000_000,
            max_cost_micros_per_day: 50_000_000,
        }
    }
}
