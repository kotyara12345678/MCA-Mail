# PostgreSQL Backups

## Configuration

Backups cover MCA Mail's PostgreSQL database only; IMAP mailboxes are never
backed up. They run in both `MAIL_MODE=read_only` and `MAIL_MODE=read_write`.

| Variable | Default in `.env.example` | Meaning |
|---|---:|---|
| `BACKUP_ENABLED` | `true` | Start the scheduled worker |
| `BACKUP_DIR` | `/app/backups` | Absolute directory for backup files |
| `BACKUP_INTERVAL_HOURS` | `6` | Interval between dumps |
| `BACKUP_RETENTION_DAYS` | `7` | Keep the newest dump per UTC date |
| `BACKUP_RETENTION_WEEKS` | `4` | Keep up to one point per ISO week beyond daily retention |
| `BACKUP_MAX_SIZE_MB` | `1024` | Maximum accepted dump size |

The production Compose file mounts `BACKUP_DIR` to a separate persistent
`backups` volume. On Unix the directory is restricted to its owner. The worker
uses `pg_dump` custom format, a temporary owner-only `.pgpass`, a command
timeout, `pg_restore --list` validation, and an atomic rename from `.partial`
to a completed UTC timestamp-plus-UUID name. Failed or empty dumps are not published and do not
trigger rotation. Only canonical `mca-backup-*.dump` files are rotation
candidates; symlinks and unrelated files are left alone. Temporary files are
not counted as successful backups.

When `BACKUP_ENABLED=false`, startup does not spawn the worker. Configuration
is validated before the application starts when backups are enabled.

## Verify Before Recovery

First list a dump's table of contents. This does not connect to or modify a
database:

```bash
pg_restore --list mca-backup-YYYYMMDDTHHMMSSZ-UUID.dump
```

For a stronger restore test, use a disposable PostgreSQL instance/database,
never the production database. Put credentials in a protected `.pgpass` file
and point `PGPASSFILE` at it; do not put passwords in command arguments:

```bash
createdb --host DB_HOST --username DB_USER mca_restore_check
pg_restore --exit-on-error --no-owner --no-acl \
  --host DB_HOST --username DB_USER --dbname mca_restore_check \
   mca-backup-YYYYMMDDTHHMMSSZ-UUID.dump
dropdb --host DB_HOST --username DB_USER mca_restore_check
```

The test database must be isolated from production and disposable. Do not run
`dropdb` against any database other than the test database you just created.

## Production Restore

Restore is deliberately manual; the application never restores automatically.
Before starting, confirm the selected dump passes `pg_restore --list`, record
the current database state with a separate backup, and schedule downtime.

1. Stop MCA Mail so it cannot write during recovery: `docker compose stop mca-mail`.
2. Copy the selected dump out of the `backups` volume to a protected recovery
   host or make it readable to the operator running `pg_restore`.
3. Configure a protected `.pgpass` file (`0600` on Unix) and export
   `PGPASSFILE` in the restore environment.
4. Restore into the intended database. `--clean --if-exists` is destructive:

   ```bash
   pg_restore --exit-on-error --clean --if-exists --no-owner --no-acl \
     --host DB_HOST --username DB_USER --dbname mca_mail \
   mca-backup-YYYYMMDDTHHMMSSZ-UUID.dump
   ```

5. Start the application and verify `/health`, `/ready`, recent leads, drafts,
   and processing events before resuming normal operations.

Do not test the production restore command on the production database. Test it
on an isolated instance first. Retain the pre-restore database copy until the
recovered service has been checked.

## Limits and Next Step

Backups on the same VPS do not protect against VPS loss, account compromise or
volume deletion. The Compose named volume survives container replacement but
is removed by `docker compose down -v`. Backups are not encrypted at rest by
this subsystem; restrict volume access and do not send dumps to an LLM. Before
off-server upload, add client-side encryption with a separately managed key,
then test decryption and restore. No external storage service is configured.