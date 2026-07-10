# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the ANALYSIS bin (`lab`): sweep/backtest engine +
# swing analyzer + Parquet-lake pipeline + HTTP API (actix-web + sqlx).
#
# IMPORTANT: this image deploys to its OWN lab server, NOT the live EC2 box —
# the analysis bin pulls arrow/parquet/rayon + a bundled libduckdb compiled from
# C++ source, so it is heavier than the 2vCPU/4GB live host should carry. See
# CLAUDE.md "Deployed server". The live backend (deploy/hunter-live/api.Dockerfile)
# is the only image on the EC2 live box.
#
# Mirrors hunter-live/api.Dockerfile's cargo-chef caching so day-to-day edits recompile
# only YOUR code. Build context = repo root (the cargo workspace lives there).
# No DATABASE_URL needed at build time: queries are runtime sqlx::query() and
# migrations are embedded via sqlx::migrate!("./migrations").
# ---------------------------------------------------------------------------

# Pin to the toolchain the project already builds on locally (rustc 1.95).
FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile the lab binary -----------------
# The rust:bookworm base ships g++/make (buildpack-deps), which duckdb's
# `bundled` feature needs to compile libduckdb in-tree.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin lab
COPY . .
# target/ is a cache mount, so it is NOT persisted into the image layer —
# copy the finished binary out within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin lab \
    && cp /app/target/release/lab /usr/local/bin/lab

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates: TLS to the SOL-price source. libstdc++6: bundled libduckdb
# static-links libduckdb itself but dynamically links the C++ standard library.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/lab /usr/local/bin/lab
# HOST/PORT/SWEEP_LAKE_DIR are set in deploy/hunter-lab/compose.yml.
EXPOSE 8082
CMD ["lab"]
