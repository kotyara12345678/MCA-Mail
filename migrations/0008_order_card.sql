-- Customer-facing order card.
--
-- The card a caller receives after a phone call is queued through the same
-- outbox as every other outbound email, so the send worker's policy check
-- (mode, auto_send, automation lock, quotas) stays the single gate deciding
-- whether it may leave the building. This adds a message_type value, not a
-- second sending mechanism.

-- Drop whatever CHECK guards message_type (auto-named
-- `mailbox_outbox_message_type_check` on Postgres, but found by definition so
-- a differently named constraint cannot silently survive and keep rejecting
-- the new value).
DO $$
DECLARE
    con TEXT;
BEGIN
    FOR con IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'mailbox_outbox'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%message_type%'
    LOOP
        EXECUTE format('ALTER TABLE mailbox_outbox DROP CONSTRAINT %I', con);
    END LOOP;
END $$;

ALTER TABLE mailbox_outbox
    ADD CONSTRAINT mailbox_outbox_message_type_check
        CHECK (message_type IS NULL OR message_type IN
               ('customer_reply', 'manager_card', 'order_card'));

-- One order card per lead per recipient: a replayed finish_call collapses
-- onto the row that already carries it, keyed by the same
-- (lead_id, message_type, recipient) rule the manager card uses.
CREATE UNIQUE INDEX uq_outbox_order_card_per_lead
    ON mailbox_outbox (lead_id, message_type, recipient)
    WHERE message_type = 'order_card' AND lead_id IS NOT NULL;
