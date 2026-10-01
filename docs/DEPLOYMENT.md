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
docker compose up -d
# 4. Verify
curl http://localhost:8080/health
```

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
| `MAIL_QUARANTINE_FOLDER` | Folder for spam | `MCA/Quarantine` |
| `MAIL_SMTP_HOST` | SMTP hostname | `smtp.example.ru` |
| `MAIL_SMTP_PORT` | SMTP port | `587` |
| `MAIL_SMTP_FROM_ADDRESS` | Envelope sender | `sales@mca-logistics.ru` |

Plus for AI:
| Setting | Description |
|---|---|
| `LLM_BASE_URL` | OpenAI-compatible endpoint |
| `LLM_API_KEY` | API key (secret) |
| `LLM_MODEL` | Default model id |

## Pre-launch Checklist

1. IMAP/SMTP credentials verified with a test mailbox.
2. Folder permissions confirmed (create/label/move).
3. Spam quarantine folder exists (or label-only configured).
4. `EMAIL_MODE=dry_run` with real mailbox — verify classification accuracy.
5. Approve auto-send policy and set `EMAIL_AUTO_SEND=false` initially.
6. Confirm retention policy (defaults: 365d emails, 180d events, 1095d audit).
7. Verify `LLM_API_KEY` budget and model prices.

## Backup & Restore

PostgreSQL volume backup:

```bash
docker compose exec postgres pg_dump -U mca mca_mail > backup_$(date +%F).sql
# restore
cat backup.sql | docker compose exec -T postgres psql -U mca mca_mail
```

## Diagnostics

- `GET /health` — service + DB liveness
- `GET /ready` — readiness probe
- Logs: `docker compose logs -f mca-mail`
- Email stuck in `processing`: check `email_processing_runs` for `running` runs
- LLM cost: metrics `llm_cost_estimate`, `llm_tokens_total`

## Security Notes

- Never commit `.env` or secrets.
- API admin endpoints require an API key.
- Email bodies are never written to logs.
- Outbound mail is bounded by rate limits in all modes.