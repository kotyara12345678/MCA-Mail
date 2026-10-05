# MCA Mail — Observability

Structured event stream for the full email lifecycle: terminal logs, a live
SSE feed and an HTML dashboard — no external stack required (no Prometheus,
Grafana, Kafka or Redis).

## Where the events go

1. **Terminal** — pretty format by default, JSON with `LOG_FORMAT=json`.
2. **SSE** — every event is mirrored to an in-memory broadcast bus that powers
   `GET /api/events/stream`.
3. **Persistence** (unchanged) — stage progress in `email_processing_runs`,
   statuses in `emails`, artifacts in `leads`/`drafts`/`handoffs`.

Events are emitted through `tracing` with the dedicated target `mca::obs`
(`observability::EVENT_TARGET`). The broadcast layer filters on that target
only and skips JSON serialisation entirely while no SSE client is connected.

## Environment

| Variable | Values | Meaning |
|---|---|---|
| `LOG_LEVEL` | `info` (default), `debug`, … | Level when `RUST_LOG` is unset |
| `RUST_LOG` | any filter | Wins over `LOG_LEVEL` when set |
| `LOG_FORMAT` | `pretty` (default) / `json` | Terminal output format |

Pretty: `HH:MM:SS LEVEL message field=value …` (level padded, event name
right after it). JSON: one object per line with `ts`, `level`, `event` and
the fields — the same shape as an SSE frame.

## Correlation

Every pipeline event carries:

- `email_id` — the email being processed;
- `processing_id` — `proc_{run_id}` of the current run.

One `processing_id` ties together log lines, reprocess API calls, stage rows
in `email_processing_runs` and audit entries of a single run.

## Event catalogue

| Group | Events (level) | Key fields |
|---|---|---|
| Email lifecycle | `email_received` (info), `processing_started`, `processing_completed`, `processing_failed` (error) | `email_id`, `processing_id`, `status`, `duration_ms`, `error_type`, `retry_count` |
| Agents | `agent_started`, `agent_completed`, `agent_failed` (error) | `agent`, `result`, `duration_ms` |
| LLM | `llm_request_started`, `llm_request_completed`, `llm_retry` (warn), `llm_failed` (error) | `provider`, `model`, `duration_ms`, `prompt_tokens`, `completion_tokens`, `retry_count` |
| Tools | `tool_started`, `tool_completed`, `tool_failed` (error), `tool_timeout` (warn) | `agent`, `tool`, `result`, `duration_ms` |
| Artifacts | `draft_created`, `handoff_created`, `email_moved`, `email_labeled` | `draft_id`/`handoff_id`, `lead_id`, `disposition`/`reason` |
| Queue | `poll_started`, `poll_completed`, `emails_fetched`, `emails_inserted`, `emails_skipped_duplicate`, `batch_claimed`, `batch_completed`, `worker_started`, `worker_completed`, `worker_failed` (error) | `mailbox`, `count`/`size`/`ok`/`failed`, `duration_ms` |
| IMAP IDLE | `idle_new_mail` (info), `idle_reissued` (debug), `idle_retry` (error), `idle_unsupported` (error) | `seconds`, `error`, `delay_seconds`, `attempt` |
| System | `migration_started`/`completed`, `database_connection_failed` (error), `repository_error` (error), `transaction_failed` (error) | `operation`, `error_type`, `duration_ms` |
| Recovery | `recovery_started`, `stuck_emails_found`, `processing_recovered`, `recovery_completed` | `runs`, `emails`, `duration_ms` |
| HTTP | `http_request` (info), `http_request_failure` (warn/error) | `method`, `path` (route pattern), `status`, `duration_ms`, `request_id`, `error_type` |

Levels: `http_request_failure` is `warn` for 4xx and `error` for 5xx/timeout.

## Live stream

- `GET /api/events/stream` — Server-Sent Events, one JSON message per frame
  (`{ts, level, event, …fields}`). No auth (local operations view; put it
  behind your reverse proxy in production). The stream is exempt from the
  30s request timeout by design and auto-reconnects in the browser.
- `GET /events` — dark HTML dashboard tailing the same stream (last 500
  events, newest first, warn/error highlighted).

While nobody listens, emission costs a counter check only.

## HTTP middleware

`api/http_log` wraps every matched route (outermost layer):

- keeps an inbound `x-request-id` or mints a UUID; echoes it on the response;
- enforces the 30s request timeout (SSE routes are merged after the layer,
  so streams are never cut);
- emits `http_request` / `http_request_failure` with the **matched route
  pattern** (`MatchedPath`, e.g. `/api/emails/:id`), not the raw URL.

## What is never logged

Prompts, LLM responses, API keys, passwords and full email bodies.
`agent_completed.result` carries only a short structured verdict
(category, disposition, scope); error strings are clipped to 300 chars;
`x-request-id` is marked sensitive.

## Tests

- Unit: `observability::{format,bus,events,system_events,tools_events,http_events}_test`
  — formatting, bus filtering, correlation, no-prompt leaks, queue/system/tool/HTTP events.
- Contract: `tests/sse_contract.rs` (HTML page, SSE framing + delivery),
  `tests/http_log_contract.rs` (request id, `http_request` emission).
  Both run without a database.
