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
- Keys stored hashed (bcrypt-style), never in plaintext
- Role-based access: admin, manager, operator, viewer