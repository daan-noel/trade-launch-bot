# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repo.

## Priorities (read first)

Meme-coin trading bot handling **massive token + trade volume**. Performance and low latency outrank everything. On every change:

- **Backend latency first.** Hot paths (ingest pipeline, strategy eval, sell-confirm loop) are throughput-critical. Avoid blocking the tokio runtime, redundant RPC/DB round-trips, per-event allocations, lock contention. Notify over poll. Flag any latency-for-convenience trade.
- **Modular & extensible.** New strategies/endpoints/pages drop in without touching unrelated code. Layering: backend = handler → service → repo; frontend = page (thin) → component + hook. One responsibility per module.
- **Efficient frontend state.** Fetch via `services/api.ts` (REST) / `services/sse.ts` (SSE); cache via RTK Query / context / localStorage to avoid re-fetch and re-render. Memoize so high-frequency ticks (SOL/USD, live trades) update only affected cells, never whole tables.
- **Reusable UI.** Build from `components/ui/`, `components/table/DataTable`, shared hooks. Don't reimplement an existing button/modal/table/formatter.
- **Concise communication.** Short answers. Write non-trivial plans to a `*-plan.md` file, not chat.
- **Docs stay in sync.** Any change that touches logic/flow MUST update the matching `@docs/*.md` map in the same task — read it before editing, update it after. No logic change ships with a stale map.

## Architecture

Rust `backend` (ingest + strategies + HTTP API) + `pump-trader` crate (trade execution) + `frontend-react` SPA. Trades on the bonding curve and the migrated AMM; exposes a React dashboard. `backend` is the composition root in [main.rs](backend/src/main.rs): long-lived tokio tasks — ingest producer → pipeline → DbWriter, StrategyRunner, SOL price poller, partition maintenance, optional HTTP server — joined by one `tokio::select!`. Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed.

**File-level maps live in `@docs/` — read the relevant one instead of re-exploring source.** (The *why* lives in `@project_plans/`.) When code moves, update the matching map (file-level, no line numbers — cheap to keep current).

| Doc | Covers |
| --- | --- |
| [@docs/architecture.md](@docs/architecture.md) | backend skeleton: `main.rs` wiring, module layout, `api/`, `state/`, `services/`, `config/`, two-crate split |
| [@docs/ingest.md](@docs/ingest.md) | `ingest_laserstream/`: client → pipeline → db_writer flow, decoder, committed gRPC codegen, partition maintenance |
| [@docs/strategies.md](@docs/strategies.md) | `strategies/`: StrategyRunner, tpsl_sniper_{1,2} clones, entry gating, exit ladder, real/paper execution, invariants |
| [@docs/trade-execution.md](@docs/trade-execution.md) | `pump-trader/`: buy/sell/amm, durable-nonce + Jito tip escalation, Sender fan-out, caches/probes |
| [@docs/database.md](@docs/database.md) | Postgres schema, migrations, partitioning, every repository → table → fns |
| [@docs/frontend.md](@docs/frontend.md) | `frontend-react/src/`: pages, `components/ui` + DataTable, hooks, RTK Query/SSE services, perf patterns |
| [@docs/sweep.md](@docs/sweep.md) | `sweep/`: **strategy-agnostic** param-sweep engine + **grouped** sweeps — corpus → fingerprint grouping → per-group sweep → best-combo by expectancy → per-strategy `*_grouped_sweep_*` tables (generic table-name-driven repo) → `/api/strategies/sweeps` → Grouped Sweep page. The TPSL2 `Strategy` impl is `sweep/strategies/tpsl2.rs`. A new strategy = `Strategy`/`ParamSpace`/`AxesSpec` + registry arm + migration |

## Commands

Cargo workspace (`backend` + `pump-trader`); frontend is a separate npm project.

```powershell
cargo check -p backend                 # typecheck the binary (backend has NO lib target)
cargo test --bin backend               # unit tests run under --bin, NOT --lib
cargo test --bin backend -- --ignored  # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test -p pump-trader              # trader crate has a lib + real unit tests
cargo run  -p backend                  # loads .env; needs Postgres; RUST_LOG=backend=info,sqlx=error
cargo run  -p backend -- probe <ladder|fanout|simulate-sell|holdings> [args]  # live trade-path probes; see run_probe in main.rs
cd frontend-react; npm run dev         # dev server :5173, proxies /api -> :8081
npm run build                          # tsc && vite build
```

Notes

- Don't build/test against the default `target/` while `backend.exe` runs — it locks output. Use `--target-dir target-check` (gitignored), or just `cargo check`.
- `cargo test --lib -p backend` → "no library targets". Always `--bin backend`.
- clippy `too_many_arguments` is `#[allow]`-ed on the trade-path fns by design.

## Performance budgets (hot path — a violation is a bug)

- **Sell-confirm loop:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll reintroduces latency + double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event allocation. DB/SSE writes go through existing channels, never inline.
- **Strategy eval:** read rule/position state from `runtime_cache.rs` (in-memory), never DB-per-event.
- Prefer notify over poll (the `TradeSignals` wakeup hub is the pattern).

## Data-scale guardrails

`tokens`/`trades` are large and grow continuously:

- **Bound every query** — paginate / time-window / stream. Never `SELECT *` the full `trades`/`raw_transactions` into memory (backend or frontend).
- Backend list endpoints: filter/sort/paginate server-side in the repo; don't fetch-all-then-slice.
- Frontend: request only the visible page; rely on RTK Query/SSE cache, not re-fetch loops.
- New high-volume tables follow the `raw_transactions` partition + retention pattern (`maintenance.rs`).

## Deployed server constraints (EC2: 2vCPU / 4GB RAM)

**Read this before any change that touches ingest, DB writes, connections, table size, or retention.**
The deployed box is IO-bound and RAM-constrained. Postgres and the backend share 4 GB; shared_buffers is 256 MB. Every change that increases write IO, index surface, connection count, or in-memory cache size on the server is a regression until proven otherwise.

Hard rules:

- **Sweeps and backtests run LOCAL only.** The server's batch pool does no work. Never add work to the deployed batch path.
- **Analysis is dump→local.** The deployed DB is a thin rolling ingest buffer (`KEEP_DAYS = 7`, daily partitions). All historical analysis happens on the local DB after a `db-snapshot-dump.sh` + `db-snapshot-restore.ps1` refresh.
- **No infra spend.** The box stays 2vCPU/4GB. "Use more RAM/disk/connections" is not a valid solution.
- **Every new write path must justify its IO cost.** New tables or columns on high-volume paths (`trades`, `tokens`, `tokens_info`) must follow the partition+retention pattern in `maintenance.rs` and must not grow the index set without dropping an equivalent one.
- **Connection counts are load-bearing.** Each open PG connection is ~26 MB competing with the 256 MB buffer pool. New pools or raised limits require shrinking something else.
- **In-memory caches trade RAM against page cache.** Any increase to `MAX_TRADES_RETAINED`, `SEED_TOKEN_LIMIT`, or cache TTLs on the server directly shrinks the Postgres buffer pool. Default to the tuned values in `tuning.rs`; raise them only on local.

See [postgres-perf-plan.md](postgres-perf-plan.md) for the full diagnosis and all tuned values.

## Definition of done

- **Backend:** `cargo check --bin backend` clean; `cargo clippy` on touched code; add/adjust a `--bin backend` (or `pump-trader`) test when logic changed.
- **Frontend:** `npm run build` clean; no extra re-render on the SOL/USD tick or live-trade stream (reuse existing memo/context patterns).
- **Docs:** any logic change updates its `@docs/` map and every referencing `@project_plans/` / `*-plan.md` file — after the task is finished and validated.
- **Temp plan files:** when a step in a root `*-plan.md` is done, remove that step from the file; when every step is done, delete the file.
- Stayed in the owning crate, no new warnings, no secrets in code.

## .env management

`.env` is gitignored and must always stay in sync with `.env.example`. When `.env.example` is updated:

1. **Back up first:** copy `.env` → `.env.backup` so you can diff what changed.
2. **Update `.env`:** apply every new key from `.env.example`, filling in real values (not placeholders).
3. Never leave a key in `.env.example` that has no counterpart in `.env`.

```powershell
Copy-Item .env .env.backup -Force   # step 1 — always do this first
# then edit .env to add/update keys
```

`.env.backup` is gitignored. It exists only for local diff/review — delete it once you've confirmed the update.

## Gotchas

- **Sell-confirm timing:** the exit loop now polls the **full** window (poll-then-sleep, tracking remaining balance) before retrying — this buffers the gRPC feed's index lag that the old RPC confirm poll incidentally provided. Without it, duplicate sells fire. Preserve when editing `execution/real.rs` or the sell retry path.
- `tpsl_sniper_1`/`tpsl_sniper_2` are intentional clones — a fix in one usually belongs in both.
- `.env` required (see `.env.example`); secrets/keys there only, never in code.
