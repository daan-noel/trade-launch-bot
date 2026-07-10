# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the forge ANALYSIS bin (package `forge-lab`, bin
# `slp-lab`): lake export + sweeps + backtests + analytics + thin HTTP API.
#
# LOCAL DEV ONLY (deploy/forge-lab/compose.yml). The analysis bin is
# workstation-only and must NEVER ship to EC2 — the lake tier pulls
# duckdb/arrow/parquet/rayon when filled. See forge/CLAUDE.md "Dep partition".
# Mirrors deploy/hunter-lab/api.Dockerfile.
#
# Build context = MONOREPO root (the single cargo workspace + shared/ crates).
# No DATABASE_URL at build time (runtime sqlx::query + embedded migrations).
# ---------------------------------------------------------------------------

FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# The rust:bookworm base ships g++/make (buildpack-deps), which duckdb's
# `bundled` feature needs to compile libduckdb in-tree when the lake fills.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# `-p forge-lab` scopes by package (default-members are the hunter bins only);
# the bin target is still named `slp-lab`.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json -p forge-lab --bin slp-lab
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p forge-lab --bin slp-lab \
    && cp /app/target/release/slp-lab /usr/local/bin/slp-lab

FROM debian:bookworm-slim AS runtime
# ca-certificates: TLS to the SOL-price source. libstdc++6: bundled libduckdb
# static-links libduckdb but dynamically links the C++ standard library.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/slp-lab /usr/local/bin/slp-lab
# HOST/PORT/SWEEP_LAKE_DIR are set in deploy/forge-lab/compose.yml.
EXPOSE 8092
CMD ["slp-lab"]
