//! OpenAI-compatible model access variables.

pub const LLM: &[(&str, &str)] = &[
    ("LLM_PROVIDER", "llm.provider"),
    ("LLM_BASE_URL", "llm.base_url"),
    ("LLM_API_KEY", "llm.api_key"),
    ("LLM_MODEL", "llm.model"),
    ("LLM_MODEL_CHEAP", "llm.routing.cheap"),
    ("LLM_MODEL_STANDARD", "llm.routing.standard"),
    ("LLM_MODEL_CAPABLE", "llm.routing.capable"),
    ("LLM_MAX_TOKENS", "llm.max_tokens"),
    ("LLM_TEMPERATURE", "llm.temperature"),
    ("LLM_TIMEOUT_SECONDS", "llm.timeout_seconds"),
    ("LLM_CONNECT_TIMEOUT_SECONDS", "llm.connect_timeout_seconds"),
    ("LLM_MAX_RETRIES", "llm.max_retries"),
    ("LLM_RETRY_BACKOFF_MS", "llm.retry_backoff_ms"),
    ("LLM_JSON_MODE", "llm.json_mode"),
    (
        "LLM_CIRCUIT_BREAKER_THRESHOLD",
        "llm.circuit_breaker_threshold",
    ),
    (
        "LLM_PRICE_PROMPT_PER_MILLION_USD",
        "llm.price.prompt_per_million_usd",
    ),
    (
        "LLM_PRICE_COMPLETION_PER_MILLION_USD",
        "llm.price.completion_per_million_usd",
    ),
    ("LLM_DISABLED", "llm.disabled"),
];
