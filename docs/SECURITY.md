# Security Model

## Email Mode

- **dry_run** (default): Analyse only, never send or change mailbox
- **review**: Prepare drafts and labels, require manual approval
- **auto**: Automatic actions within configured limits

## Prompt Injection Protection

- Email content is always treated as data, never as instructions
- System prompt is immutable from email content
- No arbitrary SQL, shell, file system, or code execution via LLM
- No credentials, internal instructions, or other client data exposed

## Outbound Controls

- Configurable hourly send limit per mailbox and per lead
- Minimum interval between sends to same address
- Maximum consecutive replies without customer response
- Body character limit enforced
- No outbound attachments in production mode

## Authentication

- API key required (`X-API-Key` header)
- Keys stored as a SHA-256 hash (the raw value is shown once at creation),
  never in plaintext; lookup is by hash on an indexed column
- Role-based access: admin, manager, operator, viewer
- `/health` and `/ready` are deliberately public; they expose uptime, version
  and whether the database answers — no mailbox, lead or message data

## Network Exposure

- Compose publishes `8080` and `5432` on `127.0.0.1` only. Neither the API nor
  PostgreSQL terminates TLS, so neither is meant to be reached off-host
- Reach the API over an SSH tunnel or behind a reverse proxy with
  certificates; keep PostgreSQL on loopback even then
- The service listens on `APP_HOST` (default `0.0.0.0`) *inside* the container
  — that is what the port mapping needs, and it is not the same as publishing
  the port on the host
- `GET /api/events/stream` and the `/events` viewer carry no authentication:
  they are a local operations view, which is another reason not to widen the
  `8080` mapping without a proxy in front

## Database Backups

PostgreSQL dumps contain personal and commercial information. The backup
directory is created with owner-only permissions on Unix, and Docker stores it
in a separate persistent volume. Keep host and volume access restricted. Dumps
are currently unencrypted; before copying them off the VPS, encrypt them with a
managed key and verify the decryption/restore procedure. The same-VPS volume is
not protection against VPS loss. Backup contents are never sent to an LLM.