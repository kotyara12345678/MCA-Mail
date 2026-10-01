# Email Integration

## Provider Abstraction

All mailbox access goes through the [`MailProvider`] trait:

- `fetch_new()` — pull messages not yet handled
- `send()` — deliver an outbound message (idempotent by key)
- `health()` — liveness without side effects
- `init()` — optional one-time setup (mock corpus load)

Two implementations ship:

1. **MockMailProvider** — reads `.eml`/`.txt` fixtures from `fixtures/emails`,
   serves each message once, records sends in memory. Safe for dev/CI.
2. **ImapMailProvider** — real mailbox via IMAP (read) + SMTP (send).

## Mock Corpus Format

Files are plain text, UTF-8, with an optional header block separated by a blank
line:

```text
From: Ivan Petrov <ivan@example.com>
To: sales@mca-logistics.ru
Subject: Freight quote
Message-ID: <abc@example.com>
Date: Mon, 5 Jan 2026 10:00:00 +0000

Hello, please quote the delivery.
```

`Message-ID` and `Date` are used for threading and dedup. A file without the
blank line is treated as body-only.

## Idempotency

- **Provider UID** (`pmid:` prefix) — strongest dedup signal.
- **RFC 5322 Message-ID** (`mid:` prefix) — stable across rescans.
- **Content hash** (`sha:` prefix) — last resort.

Unique indexes on these keys make re-polling safe: a message is stored once and
processing is never duplicated.

## Polling

- Configurable interval (`MAIL_POLL_INTERVAL_SECONDS`, default 300).
- Each cycle fetches, dedupes, stores, then hands new messages to the
  orchestrator.
- A message already stored is skipped (`InsertOutcome::Duplicate`).

## Connecting the Real Mailbox

Requires values from the system administrator — see `DEPLOYMENT.md`. Set
`MAIL_PROVIDER=imap` and the IMAP/SMTP settings. Secrets come from environment
variables only; never commit them.

[`MailProvider`]: ../crates/mca-mail/src/mail/mod.rs