# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the Rust backend (actix-web + sqlx + solana-sdk).
#
# cargo-chef caches the (huge) dependency tree as its own layer, so day-to-day
# updates only recompile YOUR code (~1 min) instead of all ~400 crates (~10 min).
#
# Build context = repo root (the cargo workspace lives there). See docker-compose.yml.
# No DATABASE_URL needed at build time: all queries are runtime sqlx::query(),
# and migrations are embedded into the binary via sqlx::migrate!("./migrations").
# ---------------------------------------------------------------------------

# Pin to the toolchain the project already builds on locally (rustc 1.95).
FROM rust:1.95-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile the backend binary -------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Cook the dependency tree. The cargo-chef layer caches at the Docker level;
# the BuildKit cache mounts add a second safety net so that even when this
# layer DOES re-run (e.g. a Cargo.toml/lock change), already-downloaded crates
# and already-compiled deps are reused instead of fetched/built from scratch.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
# Now bring in the real source and build just our crate.
COPY . .
# target/ is a cache mount, so it is NOT persisted into the image layer —
# copy the finished binary out within the same RUN. NOTE: must NOT copy to
# /app/backend — that path already exists as the `backend/` crate source dir
# (from COPY . .), so `cp file /app/backend` drops the binary INSIDE it as
# /app/backend/backend. Copy to /usr/local/bin/backend instead.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin backend \
    && cp /app/target/release/backend /usr/local/bin/backend

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates is required for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/backend /usr/local/bin/backend
# HOST/PORT are set in docker-compose.yml (must bind 0.0.0.0 inside the network).
EXPOSE 8081
CMD ["backend"]
