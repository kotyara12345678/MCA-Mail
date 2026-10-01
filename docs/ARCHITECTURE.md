# MCA Mail — Architecture

## Overview

MCA Mail is an AI-powered email processing system built for MCA Logistics. It connects to corporate email, analyses incoming messages, filters spam, qualifies leads, and hands off serious opportunities to human managers.

## Principles

- **Modular monolith**: all code runs in one process, with clear module boundaries
- **Trait-based dependency injection**: every external system (mail, LLM, database) is behind a trait
- **Idempotency first**: all write operations are idempotent via unique keys
- **Security by default**: `dry_run` mode, no auto-send, no prompt injection leaks
- **Cost-aware**: cheap model for spam/classification, capable model for complex conversations

## Module Map

```
┌─────────────────────────────────────────────────────┐
│  API (axum)                                         │
│  /health /ready /api/v1/emails /api/v1/leads ...    │
├─────────────────────────────────────────────────────┤
│  Orchestrator                                       │
│  Pipeline: Spam→Classify→Qualify→Communicate→Handoff│
├─────────────────────────────────────────────────────┤
│  Agents                        │  Tool Registry     │
│  SpamAgent                     │  CRM tools         │
│  ClassificationAgent           │  Mail tools        │
│  LeadQualificationAgent        │  Research tools    │
│  LogisticsExpertAgent          │  Support tools     │
│  CompanyResearchAgent          │                    │
│  CommunicationAgent            │                    │
│  HandoffAgent                  │                    │
├─────────────────────────────────────────────────────┤
│  LLM Provider (trait + OpenAI + Mock)               │
├─────────────────────────────────────────────────────┤
│  Mail Gateway (IMAP + SMTP + Mock)                  │
├─────────────────────────────────────────────────────┤
│  Persistence (PostgreSQL via sqlx)                  │
├─────────────────────────────────────────────────────┤
│  Domain (pure types, enums, wire_enum! macro)       │
└─────────────────────────────────────────────────────┘
```

## Agent Pipeline

1. **Spam Agent** - classifies email as spam/not_spam/uncertain
2. **Classification Agent** - categorises (new_lead, transport_request, etc.)
3. **Lead Qualification Agent** - extracts commercial parameters
4. **Company Research Agent** - checks company against registries (optional)
5. **Logistics Expert Agent** - provides professional advice (optional)
6. **Communication Agent** - drafts/polishes reply
7. **Handoff Agent** - prepares structured handoff to human manager

## Data Flow

```
Email arrives → MailProvider.fetch_new() → stored in emails table
  → ProcessingRun created → Spam Agent runs
  → Classification Agent runs → Lead created (if commercial)
  → LeadQualification Agent → Communication Agent
  → Draft created → (auto-send or manual approve)
  → If handoff: HumanHandoff created → lead locked
```

## Key Design Decisions

- **wire_enum! macro**: string-backed enums stored as TEXT — no ALTER TYPE migrations
- **Dedup keys**: provider UID > Message-ID > content hash — guarantees idempotency
- **Processing stages**: recorded per-run so crashed runs can be resumed
- **One open handoff per lead**: partial unique index prevents duplicate escalations
- **Mock everything**: mock mail provider + mock LLM = fully functional offline

## Configuration

All settings via environment variables or TOML files. See `.env.example` and `config/` modules.