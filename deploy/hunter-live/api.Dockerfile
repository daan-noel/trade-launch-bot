# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Multi-stage build for the LIVE backend bin (`hunter-live`): ingest +
# strategies + trade execution + HTTP API (actix-web + sqlx + solana-sdk).
#
# This is the ONLY backend image on the EC2 live box. The analysis bin
# (`hunter-lab`, with arrow/parquet/rayon) is a SEPARATE image
# (deploy/hunter-lab/api.Dockerfile) that deploys to its own, heavier lab
# server — never the live box. See CLAUDE.md "Deployed server".
#
# cargo-chef caches the (huge) dependency tree as its own layer, so day-to-day
# updates only recompile YOUR code (~1 min) instead of all ~400 crates (~10 min).
#
# Build context = repo root (the cargo workspace lives there). See compose.yml.
# No DATABASE_URL needed at build time: all queries are runtime sqlx::query(),
# and migrations are embedded into the binary via sqlx::migrate!("./migrations").
#
# CACHE CONTRACT (read before editing a --mount line). Locked by
# `config::deploy_guard` in hunter-core, which runs on every `cargo test`:
#   * target/ carries a per-image `id=`. Every Rust image in this repo mounts
#     /app/target, and BuildKit defaults a cache's id to its TARGET PATH — so
#     without an explicit id all four (hunter live/lab, forge live/lab) share ONE
#     target dir. They resolve different feature sets over the same shared deps,
#     so each build invalidates the previous one's artifacts: a permanent
#     recompile ping-pong that looks exactly like "the cache isn't working".
#   * The crate download cache is ONE mount covering the whole CARGO_HOME
#     (`/cargo`, id=cargo-home), DELIBERATELY shared across images: same crates,
#     one download. It has to be CARGO_HOME and not registry/ + git/, because
#     cargo's package-cache lock lives at $CARGO_HOME/.package-cache — one level
#     ABOVE those two. Mounting only registry/ and git/ shares the unpack dir
#     without the lock guarding it, so concurrent image builds unpack the same
#     crate on top of each other and one dies with `failed to unpack package ...
#     failed to open .cargo-ok ... File exists (os error 17)`. Keep the default
#     sharing=shared: cargo's lock serialises only the download/unpack, whereas
#     `sharing=locked` holds the mount for the WHOLE RUN and serialises every
#     concurrent image build behind whichever started first. Since the mount is
#     CARGO_HOME, the chef stage installs cargo-chef with `--root /usr/local` to
#     keep the binary in an image layer; never mount /usr/local/cargo itself,
#     which holds the rustup proxies PATH resolves `cargo` through.
#   * `docker builder prune -a` DELETES cache mounts. Since target/ lives only in
#     a cache mount, that wipes every cooked dependency and the next build is a
#     full cold compile. Cap the prune by size instead — see EC2-DISK-HOUSEKEEPING.md.
# ---------------------------------------------------------------------------

# Pin to the toolchain the project already builds on locally (rustc 1.95).
FROM rust:1.95-bookworm AS chef
# CARGO_HOME lives in ONE cache mount so cargo's package-cache lock is shared
# with the registry it guards (CACHE CONTRACT above). `--root /usr/local` keeps
# the cargo-chef binary in the image layer, outside that cache mount.
ENV CARGO_HOME=/cargo
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    cargo install cargo-chef --locked --root /usr/local
WORKDIR /app

# --- Plan: compute the dependency recipe from the full source ---------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- Build: cook deps (cached), then compile the live binary -------
FROM chef AS builder
# Cap build parallelism so the Rust release compile does not spawn one rustc per
# core and blow past a memory-constrained Docker VM (~8GB WSL2 default) or the
# 2vCPU/4GB EC2 box. Raise it on the workstation via the compose build arg
# (CARGO_BUILD_JOBS=8) — the default stays low so a server build can't OOM.
ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
COPY --from=planner /app/recipe.json recipe.json
# Cook the dependency tree. The cargo-chef layer caches at the Docker level;
# the BuildKit cache mounts add a second safety net so that even when this
# layer DOES re-run (e.g. a Cargo.toml/lock change), already-downloaded crates
# and already-compiled deps are reused instead of fetched/built from scratch.
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    --mount=type=cache,target=/app/target,id=hunter-live-target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin hunter-live
# Now bring in the real source and build just the live bin.
COPY . .
# target/ is a cache mount, so it is NOT persisted into the image layer —
# copy the finished binary out within the same RUN.
RUN --mount=type=cache,target=/cargo,id=cargo-home \
    --mount=type=cache,target=/app/target,id=hunter-live-target,sharing=locked \
    cargo build --release --bin hunter-live \
    && cp /app/target/release/hunter-live /usr/local/bin/hunter-live

# --- Runtime: slim image with just the binary -------------------------------
FROM debian:bookworm-slim AS runtime
# ca-certificates is required for TLS to Helius / Solana RPC (rustls trust store).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/hunter-live /usr/local/bin/hunter-live
# HOST/PORT are set in compose.yml (must bind 0.0.0.0 inside the network).
# EXPOSE is documentation-only; the real bind is the injected PORT (LIVE_API_PORT).
EXPOSE 8130
CMD ["hunter-live"]
