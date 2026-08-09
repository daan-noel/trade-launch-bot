# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the ANALYSIS bin (`hunter-lab`): sweep/backtest engine +
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
#
# CACHE CONTRACT: the per-image `id=` on the target/ mount, the shared (unlocked)
# registry/git mounts, and why `docker builder prune -a` must never run after a
# build are all explained in deploy/hunter-live/api.Dockerfile — read that header
# before editing a --mount line here. It matters MOST for this image: an lab build
# sharing hunter-live's target dir is what forces libduckdb to recompile.
# ---------------------------------------------------------------------------

# Pin to the toolchain the project already builds on locally (rustc 1.95).
FROM rust:1.95-bookworm AS chef
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile the lab binary -----------------
# The rust:bookworm base ships g++/make (buildpack-deps), which duckdb's
# `bundled` feature needs to compile libduckdb in-tree.
FROM chef AS builder
# Cap build parallelism so the cold libduckdb (C++) + Rust release compile does
# not spawn one rustc/cc per core and blow past a memory-constrained Docker VM
# (~8GB WSL2 default) — an OOM here kills buildkitd mid-build. Lower = lower peak
# RAM, slightly slower. Also bounds the cc crate's parallel .cpp compiles. Raise
# it on a big workstation via the compose build arg; the default stays safe.
ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target,id=hunter-lab-target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin hunter-lab
COPY . .
# target/ is a cache mount, so it is NOT persisted into the image layer —
# copy the finished binary out within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target,id=hunter-lab-target,sharing=locked \
    cargo build --release --bin hunter-lab \
    && cp /app/target/release/hunter-lab /usr/local/bin/hunter-lab

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates: TLS to the SOL-price source. libstdc++6: bundled libduckdb
# static-links libduckdb itself but dynamically links the C++ standard library.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/hunter-lab /usr/local/bin/hunter-lab
# HOST/PORT/SWEEP_LAKE_DIR are set in deploy/hunter.compose.yml.
# EXPOSE is documentation-only; the real bind is the injected PORT (LAB_API_PORT).
EXPOSE 8140
CMD ["hunter-lab"]
