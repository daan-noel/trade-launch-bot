# CLAUDE.md

Guidance for Claude Code working in this repo.

## Priorities

Meme-coin trading bot — **massive token + trade volume**. Performance outranks everything. On every change:

- **Backend latency first.** Hot paths (ingest, strategy eval, sell-confirm): no blocking runtime, redundant RPC/DB round-trips, per-event alloc, or lock contention. Notify over poll.
- **Modular.** handler→service→repo; page→component+hook. One responsibility per module.
- **Efficient frontend state.** RTK Query / SSE cache; memoize high-freq ticks (SOL/USD, live trades).
- **Reusable UI.** Build from `components/ui/`, `components/table/DataTable`, shared hooks.
- **Concise.** Short answers; non-trivial plans to a `*-plan.md` file.

## Architecture

Rust `backend` (ingest + strategies + HTTP API) + `pump-trader` crate (trade execution) + `frontend-react` SPA. `backend` is the composition root in [main.rs](backend/src/main.rs): long-lived tokio tasks joined in one `tokio::select!`. Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed.

**Read `@arch/` docs instead of re-exploring source. Deep-dive detail lives in `@plans/`.**

| Doc | Covers |
| --- | --- |
| [@arch/architecture.md](@arch/architecture.md) | backend skeleton: `main.rs` wiring, module layout, `api/`, `state/` |
| [@arch/ingest.md](@arch/ingest.md) | `ingest_laserstream/`: client→pipeline→db_writer, file map |
| [@arch/strategies.md](@arch/strategies.md) | `strategies/`: StrategyRunner, tpsl1/tpsl2 module map, exit ladder |
| [@arch/trade-execution.md](@arch/trade-execution.md) | `pump-trader/`: module map, key behaviors |
| [@arch/database.md](@arch/database.md) | Postgres schema, pools, every repo→table→fns |
| [@arch/frontend.md](@arch/frontend.md) | `frontend-react/src/`: pages, components, hooks, RTK Query/SSE |
| [@arch/sweep.md](@arch/sweep.md) | `sweep/`: param-sweep engine, grouping, persistence, API |

## Commands

```powershell
cargo check -p backend                 # typecheck (backend has NO lib target)
cargo test --bin backend               # unit tests — NOT --lib
cargo test --bin backend -- --ignored  # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test -p pump-trader              # trader crate tests
cargo run  -p backend                  # loads .env; needs Postgres
cargo run  -p backend -- probe <ladder|fanout|simulate-sell|holdings> [args]
cd frontend-react; npm run dev         # dev server :5173, proxies /api -> :8081
npm run build                          # tsc && vite build
```

Use `--target-dir target-check` if `backend.exe` is running (locks `target/`). Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll reintroduces latency + double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc. DB/SSE writes through channels only.
- **Strategy eval:** read from `runtime_cache.rs` (in-memory), never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream. Never `SELECT *` the full `trades`/`raw_transactions`.
- New high-volume tables follow the `raw_transactions` partition+retention pattern (`maintenance.rs`).

## Deployed server (EC2: 2vCPU / 4GB RAM — IO-bound, RAM-constrained)

- Sweeps/backtests: **local only** (server = 7-day rolling ingest buffer)
- Analysis: dump→local (`db-snapshot-dump.sh` + `db-snapshot-restore.ps1`)
- No new infra spend (box stays fixed)
- Every new write path must justify IO cost; follow partition+retention pattern
- Connection counts are load-bearing; new pools require shrinking something else
- Don't raise `MAX_TRADES_RETAINED`, `SEED_TOKEN_LIMIT`, or cache TTLs on server

## Definition of done

- **Backend:** `cargo check --bin backend` clean; clippy on touched code; test when logic changed
- **Frontend:** `npm run build` clean; no extra re-render on SOL/USD tick or live-trade stream
- **Docs — update ALL affected tiers:**
  - Rules/commands/constraints changed → **CLAUDE.md**
  - Module structure/data flow/behavior changed → **@arch/[subsystem].md**
  - Implementation detail/algorithm/decision changed → **@plans/[subsystem]/[topic].md**
- Stayed in the owning crate, no new warnings, no secrets in code

## .env management

`.env` is gitignored; keep in sync with `.env.example`. When `.env.example` updates: backup first, then apply every new key with real values.

```powershell
Copy-Item .env .env.backup -Force   # always do this first
```

## Gotchas

- **Sell-confirm timing:** the exit loop polls the **full** window before retrying — buffers the gRPC feed's index lag. Without it, duplicate sells fire. Preserve when editing `execution/real.rs` or the sell retry path.
- `tpsl_sniper_1`/`tpsl_sniper_2` are intentional clones — a fix in one usually belongs in both.
- `.env` required (see `.env.example`); secrets/keys there only, never in code.
