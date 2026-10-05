# Deployment & Sysadmin Guide

## Deployment Overview

MCA Mail is a single Rust service plus PostgreSQL. It polls corporate mail,
runs AI agents, and exposes a REST API.

## Docker Deployment (VPS)

```bash
# 1. Copy environment
cp .env.example .env
# 2. Edit .env with real values (never commit .env)
nano .env
# 3. Start the stack
docker compose up -d --build
# 4. Verify
curl http://localhost:8080/ready
```

### On the VPS

1. Install Docker Engine with the compose plugin. Nothing else is required:
   the image builds `mca-mail` itself and ships `pg_dump` for backups.
2. `cp .env.example .env`, then fill in the secrets: `MAIL_USERNAME`,
   `MAIL_PASSWORD`, `MAIL_FROM_ADDRESS`, `LLM_BASE_URL`, `LLM_API_KEY`,
   `LLM_MODEL`, `MANAGER_CARD_RECIPIENT`. Set `LOG_FORMAT=json`.
3. `docker compose up -d --build`. Migrations run automatically on first start
   (`DATABASE_AUTO_MIGRATE=true`); a failed migration stops the container with
   a non-zero exit rather than half-starting the schema.
4. Create the first API key — there is no bootstrap key, so without this step
   every `/api/v1` call returns 401:
   `docker compose run --rm mca-mail api-key create --name ops --role admin`
   The raw key is printed once. `docker compose run` starts a one-off container
   against the same database, so the key is valid for the running service.
5. `docker compose ps` must show `postgres` healthy and `mca-mail` healthy.
   The healthcheck runs `mca-mail healthcheck` → `GET /ready`, so it goes
   unhealthy (and stays visible in `ps`) once the database stops answering.
6. Watch the first real message end to end:
   `docker compose logs -f mca-mail`.

Both published ports bind to `127.0.0.1`, because neither PostgreSQL nor the
API speaks TLS. Reach the API from a workstation with an SSH tunnel
(`ssh -L 8080:127.0.0.1:8080 <vps>`), or put a reverse proxy with certificates
in front of it. Only widen a mapping (`"8080:8080"`) after one of those is in
place. PostgreSQL never needs to be reachable from outside the host.

Updates: `git pull && docker compose up -d --build`. Restart and stop are
graceful — the container gets 30s to drain open requests and finish the current
outbox delivery or backup dump before Docker sends SIGKILL.

## Modes

- `EMAIL_MODE=dry_run` — analyse only; nothing is sent or moved.
- `EMAIL_MODE=review` — drafts and labels prepared for manual approval.
- `EMAIL_MODE=auto` — approved actions execute within configured limits.
  Default is `dry_run`. Do not switch to `auto` until policies are agreed.

## Data Required from the System Administrator

To connect the real corporate mailbox the following must be provided:

| Setting | Description | Example |
|---|---|---|
| `MAIL_IMAP_HOST` | IMAP server hostname | `imap.example.ru` |
| `MAIL_IMAP_PORT` | IMAP port | `993` |
| `MAIL_IMAP_TLS` | TLS mode | `implicit` |
| `MAIL_USERNAME` | Mailbox login | `sales@mca-logistics.ru` |
| `MAIL_PASSWORD` | Mailbox password/app password | *(secret)* |
| `MAIL_INBOX` | Folder to poll | `INBOX` |
| `MAIL_SMTP_HOST` | SMTP hostname | `smtp.example.ru` |
| `MAIL_SMTP_PORT` | SMTP port | `587` |
| `MAIL_SMTP_TLS` | TLS mode (`implicit` or `starttls`) | `starttls` |
| `MAIL_FROM_ADDRESS` | Envelope sender | `sales@mca-logistics.ru` |

### Following the mailbox (IMAP IDLE)

Mail normally arrives on the `MAIL_POLL_INTERVAL_SECONDS` tick. With
`MAIL_PROVIDER=imap` the service can instead ask the server to announce new
mail the moment it lands, and keep a slower poll running behind it:

| Setting | Default | Description |
|---|---|---|
| `MAIL_IDLE` | `false` | Use IDLE when the server advertises it |
| `MAIL_IDLE_WAIT_SECONDS` | `1740` | End and re-issue IDLE this often (RFC 2177: every 29 min) |
| `MAIL_IDLE_BACKOFF_MIN_SECONDS` | `1` | First delay after a failed cycle |
| `MAIL_IDLE_BACKOFF_MAX_SECONDS` | `300` | Cap on that delay, however many attempts fail |
| `MAIL_IDLE_JITTER_PERCENT` | `20` | Random spread on each delay, so replicas do not retry as one |
| `MAIL_IDLE_FALLBACK_POLL_SECONDS` | `90` | Safety poll that runs only while IDLE is on |

Leave `MAIL_IDLE=false` until the mailbox provider confirms IDLE support: if it
is missing the service logs `idle_unsupported` and stops the watcher rather
than retrying for ever, and mail simply keeps arriving on the normal tick. Do
not raise `MAIL_IDLE_FALLBACK_POLL_SECONDS` to match the main poll — it is the
only thing that notices a connection which died without reporting an error.

Plus for AI:
| Setting | Description |
|---|---|
| `LLM_BASE_URL` | OpenAI-compatible endpoint |
| `LLM_API_KEY` | API key (secret) |
| `LLM_MODEL` | Default model id |

## API Keys

REST endpoints (except `/health`, `/ready`) need an `X-API-Key` header. Keys
are managed with the built-in CLI — the raw value is printed once at creation
and only its SHA-256 is stored.

```bash
# bare binary
mca-mail api-key create --name ops-admin --role admin --expires-days 365
mca-mail api-key list
mca-mail api-key revoke <id|prefix>

# in Docker
docker compose run --rm mca-mail api-key create --name ops-admin --role admin
docker compose run --rm mca-mail api-key list
```

Roles: `viewer` (read) → `operator` → `manager` → `admin`. Revoke by the
8-char `PREFIX` column from `api-key list`. The same CLI powers the Docker
`HEALTHCHECK` (`mca-mail healthcheck` → GET `/ready` on loopback).

## Pre-launch Checklist

1. IMAP/SMTP credentials verified with a test mailbox.
2. Folder permissions confirmed (create/label/move).
3. Spam quarantine folder exists and its exact name is set in
   `MAIL_SPAM_FOLDER`. Without it the move falls back to the SPECIAL-USE
   `\Junk` role, which servers that do not advertise SPECIAL-USE (many do not)
   do not have — and the message then stays in the inbox.
4. Sent folder exists and its exact name is set in `MAIL_SENT_FOLDER`. A
   delivered reply is `APPEND`ed there after SMTP hands it over, which is the
   only way it shows up under «Отправленные»; without the name the copy falls
   back to the SPECIAL-USE `\Sent` role and, on a server that does not
   advertise SPECIAL-USE, is skipped with a warning while the reply still
   goes out.
5. `EMAIL_MODE=dry_run` with real mailbox — verify classification accuracy.
6. Auto-send switched on for the pilot: `EMAIL_MODE=auto` **and**
   `EMAIL_AUTO_SEND=true`. Both gates must be open — with `EMAIL_MODE=review`
   the same reply is prepared but held as a draft for a human.
7. Manager card switched on: `SEND_MANAGER_CARD=true` with
   `MANAGER_CARD_RECIPIENT=<manager address>`. An empty recipient silently
   skips the card, so an unconfigured address looks like a silent failure.
8. Confirm retention policy (defaults: 365d emails, 180d events, 1095d audit).
9. Verify `LLM_API_KEY` budget and model prices.
10. First API key created (`api-key create`) and stored in the operator's
    secret manager.
11. `MAIL_IDLE` left off until the mailbox provider confirms IDLE support;
    verified with a test mailbox before switching it on.
12. `POSTGRES_PASSWORD` changed from the `mca` default when the database port
    is reachable from anywhere but loopback. It is read by both the database
    service and `DATABASE_URL`, so changing it in `.env` moves them together.
13. Published ports confirmed bound to `127.0.0.1`, or a TLS reverse proxy in
    front of `8080`. `LOG_FORMAT=json` set for log collection.

## Backup & Restore

Automatic PostgreSQL backups are enabled by default in `.env.example` and the
production Compose service. They run independently of `MAIL_MODE`; the Compose
stack stores them in the persistent `backups` volume at `/app/backups`.

Set `BACKUP_ENABLED=false` to disable scheduling. The default policy runs every
6 hours, retains one backup per UTC date for 7 days and up to 4 weekly points,
and rejects dumps larger than 1024 MB. See [BACKUPS.md](BACKUPS.md) for the
complete configuration, verification and recovery procedure.

Do not use `docker compose down -v` unless deleting the backup volume is
intentional. A volume on the same VPS does not protect against loss of that
VPS; arrange encrypted off-server storage as a separate recovery step.

## Diagnostics

- `GET /health` — liveness: always 200 while the process serves HTTP; the
  `database` field is a live probe of the pool
- `GET /ready` — readiness: 503 while the database is unreachable
- Logs: `docker compose logs -f mca-mail`
- Email stuck in `processing`: check `email_processing_runs` for `running` runs
- LLM cost: metrics `llm_cost_estimate`, `llm_tokens_total`

## Security Notes

- Never commit `.env` or secrets.
- API admin endpoints require an API key.
- Email bodies are never written to logs.
- Outbound mail is bounded by rate limits in all modes.