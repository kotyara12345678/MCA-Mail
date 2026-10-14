-- Admin panel foundation: cookie sessions for the single login/password the
-- panel shares, and LLM profiles the panel can switch between without a
-- restart. Both are additive; nothing existing changes shape.

CREATE TABLE admin_sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash   TEXT NOT NULL UNIQUE,
    login        TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_admin_sessions_expires ON admin_sessions (expires_at);

CREATE TABLE llm_profiles (
    id                               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                             TEXT NOT NULL UNIQUE,
    base_url                         TEXT NOT NULL,
    api_key                          TEXT NOT NULL,
    model                            TEXT NOT NULL,
    price_prompt_per_million_usd     DOUBLE PRECISION NOT NULL DEFAULT 0,
    price_completion_per_million_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_active                        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at                       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- At most one active profile: every call resolves exactly one model.
CREATE UNIQUE INDEX uq_llm_profiles_single_active
    ON llm_profiles (is_active) WHERE is_active;
