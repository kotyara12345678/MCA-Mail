-- Two additional classification categories: `business_inquiry` for genuine
-- commercial mail that does not match a service shape, and `other` for
-- messages that fit no business shape at all.
--
-- The CHECK constraint on `emails.category` was created inline by 0001, so it
-- carries Postgres' default name and has to be replaced rather than extended.
-- Rows already written are untouched: the new list is a superset of the old.
ALTER TABLE emails DROP CONSTRAINT IF EXISTS emails_category_check;
ALTER TABLE emails ADD CONSTRAINT emails_category_check
    CHECK (category IS NULL OR category IN
           ('new_lead', 'existing_client', 'partner', 'transport_request',
            'full_import_request', 'customs_request', 'procurement_request',
            'document_request', 'complaint', 'internal', 'advertisement',
            'spam', 'uncertain', 'business_inquiry', 'other'));
