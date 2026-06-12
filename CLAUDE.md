# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Priorities (read first)

This is a meme-coin trading bot handling **massive token + trade volume**. Performance, low latency, and effectiveness outrank everything else. Apply these on every change:

- **Performance & latency first (backend).** Hot paths (ingest pipeline, strategy eval, sell-confirm loop) are throughput-critical. Avoid blocking the tokio runtime, redundant RPC/DB round-trips, per-event allocations, and lock contention. Prefer streaming/notify over polling. If a change trades latency for convenience, flag it.
- **Modular & extensible structure (both stacks).** New strategies, endpoints, pages, and analyses must drop in without touching unrelated code. Keep the layering: backend = handler → service → repo; frontend = page (thin) → reusable component + hook. One responsibility per module; small correct diffs.
- **Efficient data/state on the frontend.** Fetch via `services/api.ts` (REST) or `services/sse.ts` (SSE) and cache deliberately — reuse RTK Query / context / localStorage to **avoid re-fetching and re-rendering**. SSE-driven updates over polling; memoize so high-frequency ticks (SOL/USD rate, live trades) update only the affected cells, never whole tables. See the frontend perf patterns below.
- **Reusable UI.** Build from `components/ui/`, `components/table/DataTable`, and shared hooks. Don't reimplement a button, modal, table, or formatter that already exists.
- **Concise, low-context communication.** Keep answers short and simple, with a small example only when it helps. Avoid re-reading/re-deriving known facts. For any non-trivial plan, write it to a `*-plan.md` file instead of dumping it in chat.

## What this is

A Pump.fun meme-coin trading bot. It ingests Solana on-chain activity in real time, runs TP/SL sniper strategies against it, executes buys/sells on the bonding curve and the migrated AMM, and exposes a React dashboard. Three deployable pieces: a Rust `backend` (ingest + strategies + HTTP API), the `pump-trader` Rust crate (trade execution), and a `frontend-react` SPA.

## Commands

Cargo workspace (`backend` + `pump-trader`); frontend is a separate npm project.

```powershell
# Build / typecheck the backend (the binary). backend has NO lib target.
cargo check -p backend
cargo build -p backend --release

# Tests. backend is binary-only — its unit tests run under --bin, NOT --lib.
cargo test --bin backend
cargo test --bin backend -- --ignored  # integration tests; need DATABASE_URL + HELIUS_RPC_URL
cargo test -p pump-trader              # the trader crate has a lib + real unit tests
cargo test -p pump-trader jito_tip     # single module
cargo test -p pump-trader fan_out_returns_success_despite_a_failing_endpoint  # single test

# Run the backend (loads .env). Requires Postgres reachable at DATABASE_URL.
cargo run -p backend                   # logging via RUST_LOG (default backend=info,sqlx=error)

# Live-infra probes — one-shot, no-/low-SOL validation of the trade path, then exit
# (runs before DB/ingest/HTTP startup). See run_probe in backend/src/main.rs.
cargo run -p backend -- probe ladder [levels]                 # Jito tip escalation ladder (read-only)
cargo run -p backend -- probe fanout [lamports] [--tip] [--confirm]   # self-transfer to all sender endpoints
cargo run -p backend -- probe simulate-sell <mint> [amount] [--cashback]  # simulate a curve sell (zero SOL)
cargo run -p backend -- probe holdings                        # list wallet token accounts

# Frontend (dev server on :5173, proxies /api -> :8081)
cd frontend-react; npm install; npm run dev
npm run build                          # tsc && vite build

# Full stack via Docker (postgres + backend + nginx-served frontend)
docker compose up -d --build           # also: run.bat (down -v then up --build)
docker compose logs -f backend
```

Notes
- **Avoid `cargo build`/`test` against the default `target/` while a `backend.exe` is running** — it locks the output. Use `--target-dir target-check` for throwaway builds (gitignored), or just `cargo check`.
- `cargo test --lib -p backend` fails with "no library targets" — backend has no `lib.rs`. Always `--bin backend`.
- No linter config beyond `cargo clippy`; clippy `too_many_arguments` is `#[allow]`-ed on the trade-path fns by design.

## Architecture

### Two-crate split
- `pump-trader/` — standalone trade-execution crate (`pump_trader`), reusable across projects. Owns nonce/seed-account pools, blockhash + Jito tip-floor caches, transaction build/send/confirm, and the bonding-curve + AMM swap logic. Has a real `lib.rs` and unit tests. `backend` re-exports it via `backend/src/trader/mod.rs` (`pub use pump_trader::{PumpFunTrader, TraderConfig, WalletHolding}`).
- `backend/` — everything else: ingest, strategies, storage, HTTP API. Binary-only.

### Startup & wiring (`backend/src/main.rs`)
`main` is the composition root. It builds the `TraderConfig`, initializes the `PumpFunTrader` (warms nonce slots + tip cache), connects Postgres and runs migrations, then spins up long-lived tokio tasks joined by a single `tokio::select!` (if any exits, the process logs and stops):
- **ingest producer** → **ingest pipeline** → **DbWriter** (the live data path)
- **StrategyRunner** (consumes strategy events)
- **SOL price poller**, **partition maintenance**, optional **HTTP server**

Shared state flows through `Arc`s and tokio channels created here: a `broadcast` SSE channel, `watch` channels for `live` mode and the persisted settings doc, the `TokenCache`, and `TradeSignals` (a (wallet,mint) wakeup hub — the DbWriter signals it when a trade is persisted so the buy/sell confirm loops wait on a notify instead of polling).

### Ingest — LaserStream is the SOLE transport (`backend/src/ingest_laserstream/`)
Helius LaserStream (Yellowstone gRPC) is the only live ingest path (the old WebSocket transport was removed). Flow: `client.rs` (gRPC producer) → mpsc `serde_json::Value` → `pipeline.rs` (`IngestPipeline`: decodes pump.fun events, updates `TokenCache`, fans out to SSE + strategy channel + DB channel) → `db_writer.rs` (`DbWriter` inserts `DbWriteOp::Trade` etc. into Postgres).
- gRPC bindings are **committed codegen** under `ingest_laserstream/generated/` (no build-time `protoc`); `.proto` is in `proto/`.
- **The `trades` table is this feed.** The TPSL exit loop confirms fills by polling `trades` — i.e. it confirms via the gRPC feed, not a separate RPC poll. When changing sell-confirm logic, account for the feed's index lag (see below).

### Strategies (`backend/src/strategies/`)
`StrategyRunner` (`runner.rs`) consumes the strategy channel and dispatches to two near-identical strategies, **`tpsl_sniper_1`** and **`tpsl_sniper_2`** (clones kept separate so their params/rules evolve independently). Each has the same shape:

- `handler.rs` (event entry), `entry/` (entry-signal gating — a rule fires only when **all** configured criteria pass), `exit/` (TP/SL evaluation), `execution/real.rs` (the live `sell_until_balance_cleared` retry loop) + paper equivalents, `runtime_cache.rs` (in-memory rule/position state loaded from DB at boot), `lifecycle.rs`, `backtest.rs`, `paper_run.rs`.
- **Exit ladder priority:** LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop. Evaluated trade-driven (each new trade) plus a 1s clock sweep for deadline exits (Stall/TimeStop) that come due in silence.
- Adding a strategy/criterion: extend `entry/` or `exit/` modules — don't fork the runner. New strategies plug into `StrategyRunner` dispatch.
- Live vs paper is the `live` watch toggle + persisted settings; rules and positions persist via `tpsl{1,2}_*_repo.rs`.

### Trade path (`pump-trader/src/trader/`)
- `buy.rs` — `buy_token` (confirmed) and `buy_token_snipe` (skips ATA-check RPC + skips RPC confirm; caller confirms via the trade feed). Curve buys use a recent blockhash (size) and a level-0 Jito tip (single shot).
- `sell.rs` / `amm.rs` — curve and post-migration AMM swaps. Sells use **durable nonce** txs and a retry loop that **escalates the Jito tip per attempt** (`tip_level`) and can run with `confirm` on or off.
- `tx.rs` — `send_transaction` **fans out** the identical signed tx to all configured Helius Sender endpoints concurrently (first success wins; on-chain signature dedup means it lands once and the tip transfer is paid at most once). `confirm_transaction`/`signature_state` poll RPC.
- `jito_tip.rs` — background-refreshed Jito tip-floor cache. `tip_lamports_for_level(level)` climbs the auction (configured percentile → p95 → p99 → ×`JITO_TIP_ESCALATION_TAIL_MULT` per extra level), always clamped to `[MIN_JITO_TIP_SOL, MAX_JITO_TIP_SOL]`. A tx that never lands costs nothing, so escalation only ever costs more once it wins.
- `init.rs`, `nonce.rs`, `pool.rs`, `blockhash.rs` — pool warming + cache refresh background tasks. `probe.rs` backs the `probe` subcommands.

**Helius Sender** already dual-routes each submission to Jito + SWQOS internally across regions (0 credits). Client-side multi-endpoint fan-out therefore adds *geographic* redundancy, not extra Jito exposure. Endpoints come from `HELIUS_FAST_SENDER_URLS` (comma-separated) or the singular `HELIUS_FAST_SENDER_URL` fallback; `TraderConfig.helius_sender_urls` is a `Vec`.

### HTTP API (`backend/src/api/`)
`api::configure` registers all `/api/*` routes (`api/mod.rs`); handlers grouped under `handlers/{tokens,trading,strategies,system}/`. Handlers are thin — they take `web::Data<Arc<AppState>>` and delegate to services/repos. SSE stream at `/api/stream`. Mutating requests (POST/PUT/DELETE/PATCH) require a bearer token **only if** `API_AUTH_TOKEN` is set (otherwise open); GET/OPTIONS always pass.

### Storage (`backend/src/storage/`)
sqlx + Postgres. `repositories/*` are the only place raw SQL lives. Migrations in `backend/migrations/` (`0001_init.sql` is the consolidated baseline; add `00NN_*.sql` for new schema). `raw_transactions` is weekly-partitioned with ~2-month retention via `ingest_laserstream/maintenance.rs`.

## Performance budgets

Hard rules on the hot path (treat a violation as a bug, not a style nit):

- **Sell-confirm loop:** no new RPC call — confirm via the `trades` feed (gRPC), as today. Adding an RPC poll reintroduces latency and double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await` on a lock, or unbounded allocation per event. DB and SSE writes go through the existing channels, never inline.
- **Strategy eval:** read rule/position state from `runtime_cache.rs` (in-memory), never query the DB per trade event.
- **Prefer notify over poll** everywhere (the `TradeSignals` wakeup hub is the pattern).
- *Numeric SLAs (e.g. max ingest→strategy latency, max trades/sec) — TODO: fill in once measured.*

## Data-scale guardrails

The `tokens`/`trades` tables are large and grow continuously:

- **Always bound queries** — paginate, time-window, or stream. Never `SELECT *` the full `trades`/`raw_transactions` table into memory (backend or frontend).
- Backend list endpoints: server-side filter/sort/paginate in the repo; don't fetch-all-then-slice.
- Frontend: request only the visible page; rely on RTK Query/SSE cache, not re-fetch loops.
- New high-volume tables follow the `raw_transactions` pattern — partition + retention via `maintenance.rs`.

## Definition of done

Before calling a change complete:

- **Backend:** `cargo check --bin backend` clean; run `cargo clippy` on touched code; add/adjust a unit test under `--bin backend` (or `pump-trader`) when logic changed.
- **Frontend:** `npm run build` (tsc + vite) clean; verify the change adds **no extra re-render** on the SOL/USD tick or live-trade stream (reuse existing memo/context patterns).
- **Both:** smallest correct diff, stayed in the owning crate, no new lint warnings, no secrets in code.

## Gotchas

- **Sell-confirm timing:** the exit loop historically relied on an RPC confirm poll that incidentally buffered the gRPC feed's index lag. The loop now polls the **full** window (poll-then-sleep, tracking remaining balance) before concluding a retry is needed — naively flipping confirm off without that buffer fires duplicate sells. Preserve that when editing `execution/real.rs` or the sell retry path.
- `tpsl_sniper_1` and `tpsl_sniper_2` are intentional clones — a fix in one usually belongs in both.
- `.env` is required (see `.env.example`). Secrets/keys live there only, never in code.
