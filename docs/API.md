# REST API Reference

Base URL: `http://localhost:8080`

## Health

`GET /health` — Service health including database status
`GET /ready` — Readiness probe

## Emails

`GET /api/v1/emails` — List emails (paginated, filterable)
`GET /api/v1/emails/{id}` — Get email detail
`GET /api/v1/emails/{id}/thread` — Get thread context
`POST /api/v1/emails/{id}/reprocess` — Re-run agent pipeline *(deferred)*
`POST /api/v1/emails/{id}/review` — Flag for human review *(deferred)*
`POST /api/v1/emails/{id}/approve` — Approve pending action *(deferred)*
`POST /api/v1/emails/{id}/reject` — Reject pending action *(deferred)*

### Deferred endpoints

The four `POST` handlers above currently enforce authentication and the role
gate (`reprocess`/`review` need `operator`, `approve`/`reject` need `manager`)
and then acknowledge the call without performing an action. They are **not**
part of the production pilot:

- `reprocess` needs the orchestrator on `ApiState`, which is not wired into the
  router yet.
- `review` writes an email status, but the contract deliberately accepts an
  arbitrary id, so a real handler would answer 404 and break that contract.
- `approve`/`reject` are email-scoped while the pending-action model is
  draft-scoped (`email_drafts.status = pending_approval`); the draft
  approve/reject wiring is tracked separately in
  [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).

Until then, treat them as authorization-only probes. Real actions happen in the
pipeline (`queue_loop` → `Orchestrator`) and at send time through the outbound
policy guard.

## Leads

`GET /api/v1/leads` — List leads
`GET /api/v1/leads/{id}` — Get lead detail
`POST /api/v1/leads` — Create lead manually
`PATCH /api/v1/leads/{id}` — Update lead
`GET /api/v1/leads/{id}/conversation` — Get conversation history
`POST /api/v1/leads/{id}/handoff` — Hand off to manager

## Agents

`GET /api/v1/agents/status` — Agent status overview
`GET /api/v1/agents/runs` — Recent agent runs
`GET /api/v1/agents/runs/{id}` — Run detail

## Settings

`GET /api/v1/settings` — Get current settings (redacted)
`PATCH /api/v1/settings` — Update settings

## Audit

`GET /api/v1/audit` — Audit log (paginated)

## Observability

`GET /api/events/stream` — Live event stream (SSE; one JSON frame per event)
`GET /events` — HTML dashboard tailing the same stream

Both are public local-operations views (no `X-API-Key`); put them behind your
reverse proxy in production. Every response carries `x-request-id`.
See [OBSERVABILITY.md](OBSERVABILITY.md) for the event catalogue.

## Authentication

All endpoints except `/health`, `/ready`, `/events` and
`/api/events/stream` require `X-API-Key` header.

Create and manage keys with the CLI (raw key is shown once):

```bash
mca-mail api-key create --name ops --role manager
mca-mail api-key list
mca-mail api-key revoke <id|prefix>
```

See [DEPLOYMENT.md](DEPLOYMENT.md#api-keys) for Docker usage and role meanings.