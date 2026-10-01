# MCA Mail — Implementation Status

_Last updated: 2026-10-01_

## Completed

### Phase 1 — Foundation ✅
- Rust workspace (mca-mail + mca-mail-testkit)
- Layered config: defaults → TOML → flat env, with `#[serde(default)]` merge
- Config validation with field-name hints, no secret echo
- tracing + JSON logs
- SQLx PostgreSQL pool + migrations (0001_core, 0002_seed)
- Health endpoints (`/health`, `/ready`)

### Phase 2 — Mail ✅
- `MailProvider` trait + `MockMailProvider` + `ImapMailProvider` + SMTP
- RFC 5322 parsing, HTML→text, attachment extraction bounds
- Idempotent ingestion (provider UID / Message-ID / content hash)
- Poll worker with configurable interval
- Dedup confirmed end-to-end (re-poll skips stored messages)

### Phase 3 — AI Core ✅
- `LlmProvider` trait: OpenAI-compatible client (retries, backoff, cost) + Mock
- Real integration verified against Polza AI (`deepseek-v4-flash`)
- Tool Registry: typed args, agent permissions, timeouts
- Orchestrator: stage recording, iteration/tool guards, retry + review fallback

### Phase 4 — Specialised Agents ✅
- Spam, Classification, Lead Qualification, Logistics Expert,
  Company Research, Email Communication, Handoff
- Structured JSON outputs with strict parsing + markdown/prose tolerance
- UTF-8-safe error truncation

### Phase 5 — Leads & Correspondence ✅
- Idempotent lead creation (conversation key)
- Lead scopes (transport/customs/procurement/full_import) from classification
- Handoff with automation lock
- Draft planning; `dry_run` default

### Phase 6 — API & Security ✅
- ✅ Axum router: health, ready, emails, leads (stubs), agents (stub)
- ✅ Prompt injection: email content treated as data
- ✅ Outbound policy guard rails (mode, rate, body limits)
- ✅ Real API-key verification: `X-API-Key` extractor checks `api_keys`
  (hashed lookup, expiry, `last_used_at`), role gates per handler
  (viewer reads, operator reprocess/review, manager approve/reject)
- ✅ Admin CLI: `mca-mail api-key create|list|revoke`, `mca-mail healthcheck`
  (the Docker HEALTHCHECK), `mca-mail help` — raw key printed once

### Phase 7 — Testing ✅ (unit) / ✅ (integration/e2e)
- 107 tests passing (83 unit + 15 persistence + 6 API auth + 3 e2e);
  clippy clean; fmt clean
- `persistence_contract`, `api_contract`, `e2e_contract` all embed and apply
  the migrations (need `MCA_TEST_DATABASE_URL`)
- Manual E2E against mock corpus + real LLM: 10 fixtures processed,
  categories/spam-verdicts/leads correctly persisted

### Phase 8 — Docs ✅
- README, ARCHITECTURE, SETUP, API, AGENTS, SECURITY, DEPLOYMENT,
  EMAIL_INTEGRATION, IMPLEMENTATION_PLAN
- `.env.example`, Dockerfile, docker-compose(.dev), CI/CD workflows

## Verified End-to-End (2026-10-01)

Fresh `mca_mail_dev`, mock mail provider (10 fixtures) + real Polza AI LLM:

- **10/10 emails terminal**: 6 `processed`, 2 `needs_review` (phishing), 2
  `quarantined` (spam/automated), **0 failed runs**, no manual intervention
- Statuses are no longer overwritten: the pipeline returns its final status
  instead of unconditionally stamping `processed` over quarantine/review
- Lead attached to the source email (`attach_lead`) — 6 emails carry `lead_id`
- Categories/verdicts: spam, phishing_suspected, transport_request,
  full_import_request, customs_request, new_lead, automated_notification
- 6 leads, 5 drafts, 4 handoffs persisted
- Parallel queue worker (`claim_batch` + 4 concurrent pipelines)
- Restart recovery: orphaned `running` runs → `failed`, stuck `processing`
  emails → `pending`, pipeline resumes
- Duplicate draft per lead no longer errors (returns the existing live draft)

## CI/CD (2026-10-01)

- `.github/workflows/ci.yml`: fmt → clippy (-D warnings) → unit tests →
  release build; integration job with a PostgreSQL service runs
  `persistence_contract` + `e2e_contract` + `api_contract` (all embed and
  apply the migrations — no sqlx-cli install); separate Docker image build
  with GHA layer cache; 100-line limit report (non-blocking)
- `.github/workflows/release.yml`: tests gate → linux/windows binaries +
  GHCR image (GHA cache) → GitHub Release with assets on `v*` tags
- `SQLX_OFFLINE=true` in CI so any future `sqlx::query!` fails fast instead
  of reaching for a live database
- Repo published at github.com/kotyara12345678/MCA-Mail — full CI green
  (check, integration, docker, line-limit)

## Verified End-to-End (2026-09-30)

With mock mail provider + real Polza AI LLM:

| Fixture | Expected | Actual |
|---|---|---|
| Spam offer | spam | `spam_verdict=spam` |
| Transport request | transport_request | `category=transport_request`, lead created |
| Full import | full_import_request | `category=full_import_request`, lead created |
| "How much?" | new_lead | `category=new_lead`, lead created |
| Phishing | phishing_suspected | `spam_verdict=phishing_suspected` |
| Prompt injection | treated as data | processed, no leak |

## Remaining

1. API: real email/lead listing queries, draft approve/reject wiring
2. Real IMAP/SMTP pilot after sysadmin provides credentials
3. Company research provider (approved registry integration)
4. Graceful-shutdown test, retention job e2e check

## Decisions Log

- Config env merge fixed: `Env::raw()` overwrote whole nested structs; replaced
  with JSON-tree construction + `Serialized::from` + `#[serde(default)]`.
- `OpenAiProvider` used `Secret::to_string()` (→`<redacted>`); fixed with
  `expose_owned()`.
- Agents with non-JSON model output: tolerant extraction + `NeedsReview`
  fallback instead of `Dead`.
- Communication agent tool hints removed: model tried tool-calls the loop does
  not execute yet.
- Poll loop now only stores messages; a separate `queue_loop` claims pending
  emails via `FOR UPDATE SKIP LOCKED` and processes 4 concurrently — a slow LLM
  can no longer stall fetching or serialize the corpus.
- `draft_repo::create` treats an existing live draft for the lead as success
  (business rule: one pending reply per lead), instead of dying on
  `uq_drafts_live_per_lead`.