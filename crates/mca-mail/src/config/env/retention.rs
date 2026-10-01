//! Company research and data retention variables.

pub const RESEARCH: &[(&str, &str)] = &[
    ("COMPANY_RESEARCH_ENABLED", "research.enabled"),
    ("COMPANY_RESEARCH_API_URL", "research.api_url"),
    ("COMPANY_RESEARCH_API_KEY", "research.api_key"),
    (
        "COMPANY_RESEARCH_TIMEOUT_SECONDS",
        "research.timeout_seconds",
    ),
    (
        "COMPANY_RESEARCH_CACHE_TTL_SECONDS",
        "research.cache_ttl_seconds",
    ),
];

pub const RETENTION: &[(&str, &str)] = &[
    ("RETENTION_DAYS", "retention.email_days"),
    ("RETENTION_ATTACHMENT_DAYS", "retention.attachment_days"),
    ("RETENTION_EVENT_DAYS", "retention.event_days"),
    ("RETENTION_AUDIT_DAYS", "retention.audit_days"),
    ("RETENTION_CONVERSATION_DAYS", "retention.conversation_days"),
    ("RETENTION_ENABLED", "retention.enabled"),
    (
        "RETENTION_ANONYMIZE",
        "retention.anonymize_instead_of_delete",
    ),
    (
        "RETENTION_CHECK_INTERVAL_HOURS",
        "retention.check_interval_hours",
    ),
];
