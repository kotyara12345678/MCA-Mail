-- Outbound email intents.
--
-- The queue gains the `send` action so that "we would like to email someone"
-- is a durable row, not a network call buried in a pipeline: nothing talks to
-- SMTP directly, and every send is preceded by a server-side policy check.
--
-- `email_id` becomes nullable because two of the intents have no inbound
-- message behind them (a manager card is generated from the lead alone).

ALTER TABLE mailbox_outbox DROP CONSTRAINT IF EXISTS mailbox_outbox_action_check;
ALTER TABLE mailbox_outbox ALTER COLUMN email_id DROP NOT NULL;

ALTER TABLE mailbox_outbox
    ADD COLUMN message_type TEXT
        CHECK (message_type IS NULL OR message_type IN ('customer_reply', 'manager_card')),
    ADD COLUMN lead_id      UUID REFERENCES leads (id) ON DELETE CASCADE,
    ADD COLUMN run_id       UUID,
    ADD COLUMN recipient    TEXT,
    ADD COLUMN sender       TEXT,
    ADD COLUMN in_reply_to  TEXT,
    ADD COLUMN ref_headers  TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN subject      TEXT NOT NULL DEFAULT '',
    ADD COLUMN body_text    TEXT NOT NULL DEFAULT '',
    ADD COLUMN body_html    TEXT NOT NULL DEFAULT '',
    ADD COLUMN idempotency_key TEXT,
    ADD COLUMN status       TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'sending', 'sent', 'failed', 'held', 'cancelled')),
    ADD COLUMN next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN max_attempts INT NOT NULL DEFAULT 5,
    ADD COLUMN correlation_id TEXT,
    ADD COLUMN policy_denial TEXT,
    ADD COLUMN sent_at      TIMESTAMPTZ,
    ADD COLUMN provider_message_id TEXT;

ALTER TABLE mailbox_outbox
    ADD CONSTRAINT mailbox_outbox_action_check
        CHECK (action IN ('move', 'label', 'mark_read', 'archive', 'flag', 'send'));

-- One logical message per key: a retried pipeline, a duplicate event or a
-- restarted worker all resolve to the same row.
CREATE UNIQUE INDEX uq_outbox_idempotency
    ON mailbox_outbox (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- At most one manager card per lead, whatever the key derivation says.
CREATE UNIQUE INDEX uq_outbox_manager_card_per_lead
    ON mailbox_outbox (lead_id, message_type)
    WHERE message_type = 'manager_card' AND lead_id IS NOT NULL;

CREATE INDEX idx_outbox_send_due
    ON mailbox_outbox (next_attempt_at)
    WHERE action = 'send' AND applied = FALSE;
