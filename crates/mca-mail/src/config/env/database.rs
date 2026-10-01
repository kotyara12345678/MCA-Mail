//! PostgreSQL connection and migration variables.

pub const DATABASE: &[(&str, &str)] = &[
    ("DATABASE_URL", "database.url"),
    ("DATABASE_MAX_CONNECTIONS", "database.max_connections"),
    ("DATABASE_MIN_CONNECTIONS", "database.min_connections"),
    (
        "DATABASE_CONNECT_TIMEOUT_SECONDS",
        "database.connect_timeout_seconds",
    ),
    (
        "DATABASE_ACQUIRE_TIMEOUT_SECONDS",
        "database.acquire_timeout_seconds",
    ),
    ("DATABASE_AUTO_MIGRATE", "database.auto_migrate"),
    (
        "DATABASE_STATEMENT_TIMEOUT_SECONDS",
        "database.statement_timeout_seconds",
    ),
];
