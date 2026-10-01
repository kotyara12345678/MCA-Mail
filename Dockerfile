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
RUN apt-get update -qq && apt-get install -y -qq ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
RUN groupadd -r mca && useradd -r -g mca -m -d /app mca
USER mca
WORKDIR /app
COPY --from=builder /app/target/release/mca-mail /app/mca-mail
COPY migrations/ /app/migrations/
COPY prompts/ /app/prompts/
COPY fixtures/ /app/fixtures/
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["/app/mca-mail", "healthcheck"]
ENTRYPOINT ["/app/mca-mail"]