# syntax=docker/dockerfile:1
FROM rust:1.96-slim-bookworm AS chef
RUN apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --package mca-mail

FROM debian:bookworm-slim AS runtime
# postgresql-client supplies pg_dump and pg_restore, which the backup worker
# shells out to. Without them BACKUP_ENABLED=true would fail every run, so the
# dependency is part of the image rather than a prerequisite the operator has
# to discover after losing a disk.
RUN apt-get update -qq && apt-get install -y -qq ca-certificates curl gnupg libssl3
RUN install -d /usr/share/postgresql-common/pgdg && \
		curl --fail --silent --show-error https://www.postgresql.org/media/keys/ACCC4CF8.asc | \
			gpg --dearmor -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg && \
		echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" \
			> /etc/apt/sources.list.d/pgdg.list && \
		apt-get update -qq && apt-get install -y -qq postgresql-client-16 && \
		rm -rf /var/lib/apt/lists/*
RUN groupadd -r mca && useradd -r -g mca -m -d /app mca
# Created up front and owned by the runtime user: pg_dump runs as `mca`, so a
# root-owned mount would fail every backup.
RUN mkdir -p /app/backups && chown mca:mca /app/backups
USER mca
WORKDIR /app
COPY --from=builder /app/target/release/mca-mail /app/mca-mail
COPY migrations/ /app/migrations/
COPY prompts/ /app/prompts/
COPY fixtures/ /app/fixtures/
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["/app/mca-mail", "healthcheck"]
ENTRYPOINT ["/app/mca-mail"]