# MCA Mail — README

## AI Email Agent for MCA Logistics

MCA Mail is an autonomous AI system that connects to corporate email, analyses incoming messages, filters spam, identifies potential clients, conducts initial correspondence, and transfers qualified leads to human managers.

### Features

- **Spam filtering** — detects spam, ads, phishing (never deletes, only quarantines)
- **Smart classification** — categorises emails by type and commercial value
- **Lead qualification** — extracts cargo parameters, company details, and service needs
- **Logistics expertise** — provides professional advice on international shipping
- **Company research** — checks company details from public registries (configurable)
- **Professional communication** — drafts replies in the client's language
- **Manager handoff** — structured lead packages with full conversation context
- **Multi-level safety** — `dry_run` → `review` → `auto` modes
- **Read-only mailbox mode** — `MAIL_MODE=read_only` (the default) reads mail and
  writes our own records while refusing every mailbox mutation
- **Automatic backups** — scheduled `pg_dump` with validation and retention
- **Instant new-mail detection** — optional IMAP IDLE with a jittered reconnect
  backoff and a fallback poller (`MAIL_IDLE=false` by default)

### Architecture

- **Language**: Rust (stable, async via Tokio)
- **Database**: PostgreSQL with sqlx
- **AI**: OpenAI-compatible API, swappable via trait
- **Mail**: IMAP + SMTP, with mock provider for development
- **API**: Axum REST API with OpenAPI documentation
- **Docker**: Multi-stage build, Docker Compose

### Quick Start

```bash
cp .env.example .env
docker compose up -d
curl http://localhost:8080/health
```

See [docs/SETUP.md](docs/SETUP.md) for detailed instructions.

### Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Setup](docs/SETUP.md)
- [API](docs/API.md)
- [Agents](docs/AGENTS.md)
- [Security](docs/SECURITY.md)
- [Deployment](docs/DEPLOYMENT.md)

### CI/CD

GitHub Actions (`.github/workflows/`):

- **ci.yml** — on push/PR: `cargo fmt` check, `cargo clippy -D warnings`,
  unit tests, release build; integration + e2e tests against a PostgreSQL
  service container (both suites embed and apply the migrations themselves);
  Docker image build with layer caching. The 100-line file limit runs as a
  non-blocking report.
- **release.yml** — on `v*` tags: full test gate, linux/windows release
  binaries, Docker image pushed to GHCR (`:latest` + tag), GitHub Release
  with the binaries attached.

Local equivalents:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
MCA_TEST_DATABASE_URL=postgres://user:pass@host:5432/db cargo test
```

### Mailbox access mode

`MAIL_MODE` controls what the agent may do to the **customer's mailbox**. It is
independent of `EMAIL_MODE`, which controls outbound sending.

| `MAIL_MODE` | Reads mail | Writes our database | Mutates the mailbox |
|-------------|-----------|--------------------|---------------------|
| `read_only` (default) | yes | yes | never |
| `read_write` | yes | yes | safe MOVE/COPY/flags only |

`read_only` allows everything that touches our own PostgreSQL — leads, drafts,
classification, history — and refuses every server-side change: SMTP send,
IMAP APPEND, MOVE, COPY, DELETE, archive, flag and label changes. Refusals
happen before any network call and are logged with the mode and operation only;
message content and credentials never appear. Because nothing is flagged,
messages are re-read on the next poll and the database dedup key keeps
processing idempotent.

An unrecognised `MAIL_MODE` stops startup. Legacy `work` parses as
`read_write`; neither mode enables SMTP sending, APPEND, `\\Deleted`, or EXPUNGE.
SMTP remains separately gated by `EMAIL_MODE`/`EMAIL_AUTO_SEND`.

### Backups

Backups run regardless of `MAIL_MODE`. Read-only protects the customer's
mailbox; it does not protect our records, which are the only account of the work.

```bash
BACKUP_ENABLED=true
BACKUP_DIR=/app/backups
BACKUP_INTERVAL_HOURS=6
BACKUP_RETENTION_DAYS=7
BACKUP_RETENTION_WEEKS=4
BACKUP_MAX_SIZE_MB=1024
```

Each run writes `pg_dump --format=custom` to a `.partial` file, validates it
with `pg_restore --list`, and only then renames it into place, so a partial
file is never mistaken for a backup. The database password is passed through a
temporary `.pgpass` rather than the command line, and each run has a timeout.

Rotating keeps the newest backup no matter what, and only deletes files it
recognises: one newest copy for each of the last seven UTC dates, then up to
four weekly copies. Symlinks and unrelated files are never followed or removed;
temporary files are not counted as completed backups.

For safe validation and step-by-step restore instructions, see
[docs/BACKUPS.md](docs/BACKUPS.md). Production restore is manual and must be
performed only after stopping the application and taking a separate copy of
the current database.

The Docker `backups` volume survives container replacement, but not loss of the
VPS or `docker compose down -v`. It is not encrypted; restrict host/volume
access and plan an encrypted off-server copy before relying on disaster
recovery.

### Configuration Parameters Required from System Admin

1. Corporate email server (IMAP host, port, TLS mode)
2. Email account credentials
3. SMTP server for outgoing mail
4. LLM provider URL and API key
5. Approved company registry source (if company research needed)