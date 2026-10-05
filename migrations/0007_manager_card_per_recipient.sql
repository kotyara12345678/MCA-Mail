-- One manager card per lead *per recipient*.
--
-- The card may go to the whole team: MANAGER_CARD_RECIPIENT takes a comma
-- separated list and the orchestrator queues one row per address, so each
-- manager has their own hourly ceiling, their own retry and their own place
-- in the audit trail. What must never happen is a second card to the *same*
-- address for one lead, which is what this index still forbids.
--
-- Widening a unique index cannot create a violation, so the rows already
-- queued keep their old `manager_card:<lead>` key and stay covered: their
-- (lead_id, message_type, recipient) still collides with any new attempt.

DROP INDEX uq_outbox_manager_card_per_lead;

CREATE UNIQUE INDEX uq_outbox_manager_card_per_lead
    ON mailbox_outbox (lead_id, message_type, recipient)
    WHERE message_type = 'manager_card' AND lead_id IS NOT NULL;
