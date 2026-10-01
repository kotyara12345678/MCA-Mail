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

### Configuration Parameters Required from System Admin

1. Corporate email server (IMAP host, port, TLS mode)
2. Email account credentials
3. SMTP server for outgoing mail
4. LLM provider URL and API key
5. Approved company registry source (if company research needed)