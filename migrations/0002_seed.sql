-- Seed data required for a working installation.
--
-- The mailbox labels MCA uses are created here so the gateway has stable
-- identifiers, and the system settings table gets its documented keys with
-- defaults. Nothing here contains credentials: operator API keys are created
-- by the bootstrap step at first start and stored hashed.

INSERT INTO email_labels (mailbox, name, color, is_system)
VALUES
    ('mca', 'MCA:NewLead',    '#2563eb', TRUE),
    ('mca', 'MCA:Qualified',  '#16a34a', TRUE),
    ('mca', 'MCA:Handoff',    '#ea580c', TRUE),
    ('mca', 'MCA:NeedsReview','#dc2626', TRUE),
    ('mca', 'MCA:Spam',       '#6b7280', TRUE),
    ('mca', 'MCA:Waiting',    '#ca8a04', TRUE)
ON CONFLICT (mailbox, name) DO NOTHING;

INSERT INTO system_settings (key, value, description)
VALUES
    ('retention.enabled', 'true'::jsonb, 'Enable the retention cleanup job'),
    ('retention.email_days', '365'::jsonb, 'Days before inbound email bodies are anonymized'),
    ('retention.attachment_days', '365'::jsonb, 'Days before attachment extracts are removed'),
    ('retention.event_days', '180'::jsonb, 'Days before processing events are purged'),
    ('retention.audit_days', '1095'::jsonb, 'Days before audit rows are purged'),
    ('retention.anonymize', 'true'::jsonb, 'Anonymize instead of deleting message bodies'),
    ('automation.global_enabled', 'true'::jsonb, 'Master switch for autonomous outbound mail'),
    ('outbound.max_per_hour', '20'::jsonb, 'Ceiling of outbound messages per rolling hour'),
    ('company_research.configured', 'false'::jsonb, 'True once a registry source is approved')
ON CONFLICT (key) DO NOTHING;
