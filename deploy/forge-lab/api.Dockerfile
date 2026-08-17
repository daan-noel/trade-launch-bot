# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the forge ANALYSIS bin (package + bin `forge-lab`):
# lake export + sweeps + backtests + analytics + thin HTTP API.
#
# LOCAL DEV ONLY (deploy/forge.compose.yml). The analysis bin is
# workstation-only and must NEVER ship to EC2 — the lake tier pulls
# duckdb/arrow/parquet/rayon when filled. See forge/CLAUDE.md "Dep partition".
# Mirrors deploy/hunter-lab/api.Dockerfile.
#
# Build context = MONOREPO root (the single cargo workspace + shared/ crates).
# No DATABASE_URL at build time (runtime sqlx::query + embedded migrations).
#
# CACHE CONTRACT: the per-image `id=` on the target/ mount, the one shared
# (unlocked) CARGO_HOME mount, and why `docker builder prune -a` must never run
# after a build are all explained in deploy/hunter-live/api.Dockerfile — read that header
# before editing a --mount line here.
# ---------------------------------------------------------------------------

FROM rust:1.95-bookworm AS chef
# CARGO_HOME lives in ONE cache mount so cargo's package-cache lock is shared
# with the registry it guards (CACHE CONTRACT above). `--root /usr/local` keeps
# the cargo-chef binary in the image layer, outside that cache mount.
ENV CARGO_HOME=/cargo
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    cargo install cargo-chef --locked --root /usr/local
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# The rust:bookworm base ships g++/make (buildpack-deps), which duckdb's
# `bundled` feature needs to compile libduckdb in-tree when the lake fills.
FROM chef AS builder
# Raise on a big workstation via the compose build arg; the low default bounds
# both rustc and the cc crate's parallel .cpp compiles for libduckdb.
ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
COPY --from=planner /app/recipe.json recipe.json
# `-p forge-lab` scopes by package (default-members are the hunter bins only).
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    --mount=type=cache,target=/app/target,id=forge-lab-target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json -p forge-lab --bin forge-lab
COPY . .
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    --mount=type=cache,target=/app/target,id=forge-lab-target,sharing=locked \
    cargo build --release -p forge-lab --bin forge-lab \
    && cp /app/target/release/forge-lab /usr/local/bin/forge-lab

FROM debian:bookworm-slim AS runtime
# ca-certificates: TLS to the SOL-price source. libstdc++6: bundled libduckdb
# static-links libduckdb but dynamically links the C++ standard library.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/forge-lab /usr/local/bin/forge-lab
# HOST/PORT/SWEEP_LAKE_DIR are set in deploy/forge.compose.yml.
# EXPOSE is documentation-only; the real bind is the injected PORT (LAB_API_PORT).
EXPOSE 8240
CMD ["forge-lab"]
