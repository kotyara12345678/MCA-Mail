//! Process, HTTP, agent and security variables.

pub const APP: &[(&str, &str)] = &[
    ("APP_ENV", "app.env"),
    ("APP_NAME", "app.name"),
    ("LOG_LEVEL", "app.log_level"),
    ("LOG_FORMAT", "app.log_format"),
    ("DATA_DIR", "app.data_dir"),
    ("PROCESSING_CONCURRENCY", "app.processing_concurrency"),
    ("APP_HOST", "api.host"),
    ("APP_PORT", "api.port"),
    ("API_ADMIN_TOKEN", "api.admin_token"),
    ("API_ENABLE_SWAGGER_UI", "api.enable_swagger_ui"),
    ("API_CORS_ALLOW_ORIGIN", "api.cors_allow_origin"),
    ("API_REQUEST_TIMEOUT_SECONDS", "api.request_timeout_seconds"),
    ("API_MAX_PAGE_SIZE", "api.max_page_size"),
    ("API_DEFAULT_PAGE_SIZE", "api.default_page_size"),
];

pub const AGENTS: &[(&str, &str)] = &[
    ("AGENT_MAX_ITERATIONS", "agents.max_iterations"),
    ("AGENT_MAX_TOOL_CALLS", "agents.max_tool_calls"),
    ("AGENT_TASK_TIMEOUT_SECONDS", "agents.task_timeout_seconds"),
    ("AGENT_MAX_RETRIES", "agents.max_retries"),
    (
        "AGENT_TOOL_CALL_TIMEOUT_SECONDS",
        "agents.tools.call_timeout_seconds",
    ),
    (
        "AGENT_MAX_TOOL_CALLS_PER_TASK",
        "agents.tools.max_calls_per_task",
    ),
    ("AGENT_COMMUNICATION_ENABLED", "agents.communication"),
    ("AGENT_COMPANY_RESEARCH_ENABLED", "agents.company_research"),
];

pub const SECURITY: &[(&str, &str)] = &[
    ("EMAIL_MODE", "security.email_mode"),
    ("EMAIL_AUTO_SEND", "security.outbound.auto_send"),
    (
        "EMAIL_MAX_SENDS_PER_HOUR",
        "security.outbound.max_sends_per_hour",
    ),
    (
        "EMAIL_MAX_SENDS_PER_LEAD_PER_HOUR",
        "security.outbound.max_sends_per_lead_per_hour",
    ),
    (
        "EMAIL_MIN_INTERVAL_SECONDS",
        "security.outbound.min_interval_seconds",
    ),
    (
        "EMAIL_MAX_CONSECUTIVE_REPLIES",
        "security.outbound.max_consecutive_replies",
    ),
    ("EMAIL_MAX_BODY_CHARS", "security.outbound.max_body_chars"),
    (
        "EMAIL_ALLOW_OUTBOUND_ATTACHMENTS",
        "security.attachments.allow_outbound_attachments",
    ),
    ("MAX_MESSAGE_BYTES", "security.inbound.max_message_bytes"),
    (
        "MAX_ATTACHMENT_BYTES",
        "security.inbound.max_attachment_bytes",
    ),
    ("MAX_ATTACHMENTS", "security.inbound.max_attachments"),
    ("MAX_LLM_BODY_CHARS", "security.inbound.max_llm_body_chars"),
    ("PROMPTS_DIR", "security.prompts_dir"),
    (
        "MAX_COST_MICROS_PER_TASK",
        "security.max_cost_micros_per_task",
    ),
    (
        "MAX_COST_MICROS_PER_HOUR",
        "security.max_cost_micros_per_hour",
    ),
    (
        "MAX_COST_MICROS_PER_DAY",
        "security.max_cost_micros_per_day",
    ),
];
