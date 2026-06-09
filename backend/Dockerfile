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
# This layer is cached and only re-runs when dependencies change.
RUN cargo chef cook --release --recipe-path recipe.json
# Now bring in the real source and build just our crate.
COPY . .
RUN cargo build --release --bin backend

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates is required for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/backend /usr/local/bin/backend
# HOST/PORT are set in docker-compose.yml (must bind 0.0.0.0 inside the network).
EXPOSE 8081
CMD ["backend"]
