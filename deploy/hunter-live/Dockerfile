# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the LIVE backend bin (`live`): ingest +
# strategies + trade execution + HTTP API (actix-web + sqlx + solana-sdk).
#
# This is the ONLY backend image shipped to EC2. The analysis bin
# (`lab`, with arrow/parquet/rayon) runs on the workstation and is
# never containerised — see CLAUDE.md "Deployed server".
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

# --- Build: cook deps (cached), then compile the live binary -------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Cook the dependency tree. The cargo-chef layer caches at the Docker level;
# the BuildKit cache mounts add a second safety net so that even when this
# layer DOES re-run (e.g. a Cargo.toml/lock change), already-downloaded crates
# and already-compiled deps are reused instead of fetched/built from scratch.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin live
# Now bring in the real source and build just the live bin.
COPY . .
# target/ is a cache mount, so it is NOT persisted into the image layer —
# copy the finished binary out within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin live \
    && cp /app/target/release/live /usr/local/bin/live

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates is required for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/live /usr/local/bin/live
# HOST/PORT are set in docker-compose.yml (must bind 0.0.0.0 inside the network).
EXPOSE 8081
CMD ["live"]
