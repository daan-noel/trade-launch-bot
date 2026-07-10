# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the launch-platform LIVE bin (`forge-live`): ingest +
# launcher + trading + thin HTTP API. Mirrors deploy/hunter-live/api.Dockerfile.
#
# This is the ONLY launch-platform image shipped to EC2. The analysis bin
# (`forge-lab`, with duckdb/arrow/parquet) runs on the workstation and is never
# containerised — see the forge CLAUDE.md "Dep partition".
#
# cargo-chef caches the (huge) dependency tree as its own layer, so day-to-day
# updates only recompile YOUR code instead of all deps.
#
# Build context = MONOREPO root (the single cargo workspace + shared/ crates
# live there). See deploy/forge.compose.yml.
# No DATABASE_URL needed at build time (runtime sqlx::query + embedded migrations).
# ---------------------------------------------------------------------------

# Pin to the toolchain the workspace builds on (matches hunter-live).
FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile just the forge-live binary -------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# NOTE: `-p forge-live` is REQUIRED. The root workspace sets default-members to the
# hunter bins only, so a bare `--bin forge-live` resolves against those and errors
# "no bin target named forge-live in default-run packages". Scope by package.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json -p forge-live --bin forge-live
# Now bring in the real source and build only the forge-live bin.
COPY . .
# target/ is a cache mount (not persisted into the layer) — copy the finished
# binary out within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p forge-live --bin forge-live \
    && cp /app/target/release/forge-live /usr/local/bin/forge-live

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/forge-live /usr/local/bin/forge-live
# HOST/PORT are set in docker-compose.yml (bind 0.0.0.0 inside the network).
# EXPOSE is documentation-only; the real bind is the injected PORT (LIVE_API_PORT).
EXPOSE 8230
CMD ["forge-live"]
