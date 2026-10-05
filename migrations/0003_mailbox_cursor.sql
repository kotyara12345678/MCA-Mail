-- Fresh-mail cursor: UIDVALIDITY + high-water mark, one row per mailbox.
--
-- The poll worker consults this row before every fetch. On the first cycle it
-- records the current boundary (UIDNEXT-1) and processes nothing older, so a
-- deployment against a mailbox with years of history starts clean. Every cycle
-- that stores a batch moves the mark forward; a UIDVALIDITY change rewrites the
-- boundary instead of replaying history.
CREATE TABLE mailbox_cursors (
    mailbox         TEXT        PRIMARY KEY,
    uid_validity    BIGINT      NOT NULL,
    high_water_uid  BIGINT      NOT NULL,
    initialized_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
