# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the launch-platform LIVE bin (`slp-live`): ingest +
# launcher + trading + thin HTTP API. Mirrors meme-trading/live/Dockerfile.
#
# This is the ONLY launch-platform image shipped to EC2. The analysis bin
# (`slp-lab`, with duckdb/arrow/parquet) runs on the workstation and is never
# containerised — see the SLP CLAUDE.md "Dep partition".
#
# cargo-chef caches the (huge) dependency tree as its own layer, so day-to-day
# updates only recompile YOUR code instead of all deps.
#
# Build context = MONOREPO root (the single cargo workspace + shared/ crates
# live there). See solana-launch-platform/docker-compose.yml.
# No DATABASE_URL needed at build time (runtime sqlx::query + embedded migrations).
# ---------------------------------------------------------------------------

# Pin to the toolchain the workspace builds on (matches meme-trading/live).
FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile just the slp-live binary --------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# NOTE: `-p slp-live` is REQUIRED. The root workspace sets default-members to the
# meme-trading bins only, so a bare `--bin slp-live` resolves against those and
# errors "no bin target named slp-live in default-run packages". Scope by package.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json -p slp-live --bin slp-live
# Now bring in the real source and build only the slp-live bin.
COPY . .
# target/ is a cache mount (not persisted into the layer) — copy the finished
# binary out within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p slp-live --bin slp-live \
    && cp /app/target/release/slp-live /usr/local/bin/slp-live

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/slp-live /usr/local/bin/slp-live
# HOST/PORT are set in docker-compose.yml (bind 0.0.0.0 inside the network).
EXPOSE 8081
CMD ["slp-live"]
