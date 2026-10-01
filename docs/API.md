# REST API Reference

Base URL: `http://localhost:8080`

## Health

`GET /health` — Service health including database status
`GET /ready` — Readiness probe

## Emails

`GET /api/v1/emails` — List emails (paginated, filterable)
`GET /api/v1/emails/{id}` — Get email detail
`GET /api/v1/emails/{id}/thread` — Get thread context
`POST /api/v1/emails/{id}/reprocess` — Re-run agent pipeline
`POST /api/v1/emails/{id}/review` — Flag for human review
`POST /api/v1/emails/{id}/approve` — Approve pending action
`POST /api/v1/emails/{id}/reject` — Reject pending action

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

## Authentication

All endpoints except `/health` and `/ready` require `X-API-Key` header.