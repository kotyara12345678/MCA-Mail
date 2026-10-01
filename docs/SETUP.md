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
- `MAIL_PROVIDER` — `mock` (default) or `imap`
- `LLM_PROVIDER` — `mock` (default) or `openai_compatible`
- `EMAIL_MODE` — `dry_run`, `review`, or `auto`

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