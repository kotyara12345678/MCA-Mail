# MCA Mail — Implementation Plan

## Goal

Production-ready AI email agent for MCA Logistics: connects to corporate email,
filters spam, qualifies leads, drafts replies, and hands off qualified leads to
human managers.

## Architecture Summary

Modular monolith in Rust:

- `domain` — pure types, wire enums, invariants (no I/O)
- `config` — layered config (defaults → TOML → env), validation
- `error` — typed errors with stable codes + HTTP mapping
- `mail` — `MailProvider` trait: Mock + IMAP/SMTP
- `llm` — `LlmProvider` trait: OpenAI-compatible + Mock
- `tools` — typed Tool Registry with permission checks
- `agents` — 7 specialised agents
- `orchestration` — agent pipeline with stage tracking
- `persistence` — PostgreSQL repos via sqlx
- `api` — Axum REST API
- `application` — bootstrap + background workers

## Phases

### Phase 0 — Research
- Existing codebase reviewed and preserved.
- Structure, migrations, domain, config, mail, persistence already present.

### Phase 1 — Foundation
- ✅ Cargo workspace, config, logging, health endpoints
- ✅ PostgreSQL migrations (core + seed)
- ✅ Docker Compose, CI

### Phase 2 — Mail
- ✅ `MailProvider` trait, `MockMailProvider`, `ImapMailProvider`, SMTP
- ✅ Inbound parsing (RFC 5322, HTML→text, attachments)
- ✅ Dedup via provider UID / Message-ID / content hash
- ✅ Poll worker with periodic fetch

### Phase 3 — AI Core
- ✅ `LlmProvider` trait + OpenAI-compatible client + Mock
- ✅ Tool Registry with typed args, permissions, timeouts
- ✅ Orchestrator with stage recording
- ✅ Iteration/tool-call guards, retry handling

### Phase 4 — Specialised Agents
- ✅ Spam, Classification, Lead Qualification, Logistics Expert,
  Company Research, Email Communication, Handoff

### Phase 5 — Leads & Correspondence
- ✅ Lead creation (idempotent by conversation key)
- ✅ Lead status management, handoff with automation lock
- ✅ Draft/reply planning (dry_run default)

### Phase 6 — API & Security
- ✅ Axum REST API (health, emails, leads, agents, settings, audit stubs)
- ✅ Prompt injection safeguards (email content = data, never instructions)
- ✅ Outbound policy guard rails (mode, rate, body limits)
- ⚠️ API auth placeholder (X-API-Key parse), full key DB integration pending

### Phase 7 — Testing
- ✅ Unit tests (78)
- ✅ Integration test (persistence_contract, requires `MCA_TEST_DATABASE_URL`)
- ⚠️ E2E scenarios: runnable manually against mock corpus; automated harness pending

### Phase 8 — Docs & Launch Prep
- ✅ README, ARCHITECTURE, SETUP, API, AGENTS, SECURITY
- ✅ .env.example
- ✅ Docker multi-stage, Compose, CI/CD
- ⚠️ Sysadmin instruction checklist (see DEPLOYMENT.md additions)

## Key Decisions

1. **wire_enum! macro** — enums stored as TEXT with CHECK constraints; adding a
   value is an ordinary migration.
2. **Dedup keys** — `pmid:` > `mid:` > `sha:`; unique indexes guarantee
   at-most-once ingestion.
3. **One live run per email** — partial unique index prevents double processing.
4. **One open handoff per lead** — partial unique index; repeated escalations
   update the existing handoff.
5. **Draft idempotency** — SHA-256 of (run, lead, body) as unique key.
6. **Mock everything** — offline dev/test without real mail or paid LLM.
7. **Config via flat env** — documented variables mapped onto nested tree;
   `#[serde(default)]` on every settings struct so partial env overrides merge
   with defaults.

## Remaining Work

- Wire API auth to `api_keys` table (real key verification)
- Add full email/lead listing queries to API handlers
- Automated e2e harness in `tests/e2e`
- Real IMAP/SMTP integration testing once credentials are provided
- Company research provider (needs approved registry source)