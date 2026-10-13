-- Red flag for mail that may be an order.
--
-- A manager scanning the inbox needs the commercial traffic to jump out:
-- transport requests, price questions, follow-ups on a live shipment,
-- invoices tied to an order. The pipeline already produces every signal
-- that answers "is this an order?" — the category, the spam verdict, the
-- classifier's confidence and the thread the message landed in — so the
-- flag is one boolean column derived from those signals, never a second
-- classification of its own.
--
-- FALSE by default: an email nobody has run the pipeline over is not order
-- traffic by definition, and the flag must stay trustworthy enough to be
-- worth looking at.

ALTER TABLE emails ADD COLUMN order_flag BOOLEAN NOT NULL DEFAULT FALSE;
