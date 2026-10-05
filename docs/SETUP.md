# Setup

## Prerequisites

- Rust 1.85+ (stable)
- PostgreSQL 16+
- Docker & Docker Compose (optional)

## Quick Start (Development)

```bash
# 1. Start PostgreSQL
docker compose -f docker-compose.dev.yml up -d postgres

# 2. Copy and edit config
cp .env.example .env

# 3. Run migrations
cargo install sqlx-cli
sqlx migrate run

# 4. Run the service
cargo run
```

## Configuration

See `.env.example` for all available settings. Key variables:

- `DATABASE_URL` — PostgreSQL connection string
- `MAIL_MODE` — `read_only` (default) or `read_write` (MOVE/COPY/safe flags only)
- `MAIL_PROVIDER` — `mock` (default) or `imap`
- `LLM_PROVIDER` — `mock` (default) or `openai_compatible`
- `EMAIL_MODE` — `dry_run`, `review`, or `auto`
- `BACKUP_ENABLED` — enables scheduled PostgreSQL backups (sample: `true`)
- `BACKUP_DIR` — absolute backup directory, mounted persistently in Docker
- `BACKUP_INTERVAL_HOURS`, `BACKUP_RETENTION_DAYS`,
  `BACKUP_RETENTION_WEEKS`, `BACKUP_MAX_SIZE_MB` — backup schedule and limits

`read_only` still permits processing and writes to MCA Mail's own PostgreSQL,
including internal drafts. It refuses SMTP send, IMAP APPEND, MOVE, COPY,
DELETE, `\\Deleted`, APPEND, SMTP send and EXPUNGE are always refused by the
mailbox mutation gate. `read_write` permits role-based MOVE/COPY and `\\Seen`/
`$Junk` flags only; legacy `work` is parsed as `read_write`. SMTP is a separate
policy and is not enabled by this mode. An unknown mode is a configuration error.

Backups contain customer and commercial data. Restrict access to the mounted
volume; backups are not encrypted yet and must not be sent to an LLM. See
[BACKUPS.md](BACKUPS.md) for restore and isolated validation steps.

## Testing

```bash
cargo test                    # unit tests
cargo test --test persistence_contract  # integration tests (requires PG)
cargo test --lib              # library tests only
```

## Docker

```bash
docker compose up -d          # production stack
docker compose -f docker-compose.dev.yml up -d  # dev stack
```