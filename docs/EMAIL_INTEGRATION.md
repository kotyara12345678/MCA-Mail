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

## How New Mail Arrives

There is exactly one reader: `sync_loop`. Everything else only decides *when*
it should run.

```text
  IDLE watcher ──┐
                 ├──> one-slot trigger ──> sync_loop ──> fetch / dedupe / store
  fallback poll ─┘              ▲              │
                                 └── interval ──┘
```

| Source | Default | Fires |
|---|---|---|
| Interval | `MAIL_POLL_INTERVAL_SECONDS=300` | on a fixed tick |
| IDLE watcher | `MAIL_IDLE=false` | when the server says the mailbox changed |
| Fallback poll | `MAIL_IDLE_FALLBACK_POLL_SECONDS=90` | only while IDLE is on |

- **The trigger is one slot wide.** Several notifications arriving during a
  fetch collapse into the next single read; a notifier is never blocked behind
  a slow one.
- **IDLE is read-only.** The watcher opens its own connection with `EXAMINE`, so
  the server itself refuses any flag change — it never shares the fetch
  session, which would otherwise park `FETCH` behind an open `IDLE`.
- **IDLE is checked first.** `MAIL_IDLE` is the operator's switch and
  `MAIL_PROVIDER=imap` is what makes it meaningful; the capability is then
  verified against the server before it is used (RFC 3501: a client must not
  send `IDLE` unless the server advertised it).
- **Failures back off.** A dropped connection waits
  `MAIL_IDLE_BACKOFF_MIN_SECONDS`, doubling up to `MAIL_IDLE_BACKOFF_MAX_SECONDS`
  with `MAIL_IDLE_JITTER_PERCENT` of spread, and resets on the next healthy
  cycle. A server without the capability, or a rejected password, stops the
  watcher instead of being retried for ever.
- **The fallback poller is not optional in spirit.** IDLE is a *fast* path, not
  the only path: a connection that dies without an error would otherwise stop
  mail while every status check still reads "connected".

## Connecting the Real Mailbox

Requires values from the system administrator — see `DEPLOYMENT.md`. Set
`MAIL_PROVIDER=imap` and the IMAP/SMTP settings. Secrets come from environment
variables only; never commit them.

[`MailProvider`]: ../crates/mca-mail/src/mail/mod.rs