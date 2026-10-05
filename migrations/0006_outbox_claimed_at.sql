-- When a send was claimed.
--
-- `status = 'sending'` is written before the network call, so a process that
-- dies mid-delivery leaves rows nobody will ever pick up again. A claim
-- timestamp is what lets the worker tell "in flight" from "abandoned" without
-- guessing.

ALTER TABLE mailbox_outbox ADD COLUMN claimed_at TIMESTAMPTZ;

CREATE INDEX idx_outbox_stale_sending
    ON mailbox_outbox (claimed_at)
    WHERE action = 'send' AND status = 'sending';
