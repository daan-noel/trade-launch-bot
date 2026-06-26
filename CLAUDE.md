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

Five Rust workspace crates + `frontend-react` SPA. The old single `backend` crate was split into two bins over a shared core (see [@plans/modes/crate-split.md](@plans/modes/crate-split.md)):

| Crate | Kind | Role |
| --- | --- | --- |
| `pump-constants` | lib | zero-dep literal constants (program IDs, CU tunables); `pump-trader` re-exports as `pump_trader::constants` |
| `backend-core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, core handlers, strategy domain (`tpsl_rules_core`) |
| `ingest-laserstream` | lib | Helius LaserStream gRPC transport (client→pipeline→db_writer) + watchdog; exposes `spawn(…) -> IngestHandles` |
| `backend-deploy` | **bin** | LIVE box: strategies, trader, deploy services/state (`DeployState`), live/trading handlers, `probe`. Ships to EC2 |
| `backend-local` | **bin** | ANALYSIS box: sweep/backtest, swing analyzer, local state (`LocalState`), rule-authoring + sweep handlers. Runs with NO keys / NO gRPC |

Each bin is its own composition root (`tokio::select!` over long-lived tasks). `backend-deploy/main.rs` calls `ingest_laserstream::spawn(…)` + trading + strategy + HTTP; `backend-local/main.rs` is thin (SOL-price poller + token-cache seed + HTTP, no ingest/trader). Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed. Both serve `configure_core_routes` plus their own route config; `GET /api/system/capabilities` advertises which bin (frontend gates nav/routes on it).

**Read `@arch/` docs instead of re-exploring source. Deep-dive detail lives in `@plans/`.**

| Doc | Covers |
| --- | --- |
| [@arch/architecture.md](@arch/architecture.md) | crate map, two bins' `main.rs` wiring, three state structs, ingest interface |
| [@arch/ingest.md](@arch/ingest.md) | `ingest_laserstream/`: client→pipeline→db_writer, file map |
| [@arch/strategies.md](@arch/strategies.md) | `strategies/`: StrategyRunner, tpsl1/tpsl2 module map, exit ladder |
| [@arch/trade-execution.md](@arch/trade-execution.md) | `pump-trader/`: module map, key behaviors |
| [@arch/database.md](@arch/database.md) | Postgres schema, pools, every repo→table→fns |
| [@arch/frontend.md](@arch/frontend.md) | `frontend-react/src/`: pages, components, hooks, RTK Query/SSE |
| [@arch/sweep.md](@arch/sweep.md) | `sweep/`: param-sweep engine, grouping, persistence, API |

## Commands

```powershell
cargo check -p backend-deploy          # typecheck the live bin
cargo check -p backend-local           # typecheck the analysis bin
cargo check -p backend-core            # typecheck the shared lib
cargo test  -p backend-deploy          # deploy unit tests (strategies, trader edge)
cargo test  -p backend-local           # local unit tests (sweep, swing)
cargo test  -p backend-deploy -- --ignored  # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test  -p pump-trader             # trader crate tests
cargo run   -p backend-deploy          # live box: loads .env; needs Postgres + Helius gRPC
cargo run   -p backend-local           # analysis box: needs Postgres; NO keys / NO gRPC
cargo run   -p backend-deploy -- probe <ladder|fanout|simulate-sell|holdings> [args]
cd frontend-react; npm run dev         # dev server :5173, proxies /api -> :8081
npm run build                          # tsc && vite build
```

Stay in the owning crate (`pump-constants` / `backend-core` / `ingest-laserstream` / `backend-deploy` / `backend-local`). Use `--target-dir target-check` if a bin `.exe` is running (locks `target/`). Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll reintroduces latency + double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc. DB/SSE writes through channels only.
- **Strategy eval:** read from `runtime_cache.rs` (in-memory), never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream. Never `SELECT *` the full `trades`/`raw_transactions`.
- New high-volume tables follow the `raw_transactions` partition+retention pattern (`maintenance.rs`).

## Deployed server (EC2: 2vCPU / 4GB RAM — IO-bound, RAM-constrained)

- **Ship `backend-deploy` + `ingest-laserstream` to EC2 only.** `backend-local` (sweep/arrow/parquet/rayon) stays on the workstation — never deploy it.
- Sweeps/backtests: **local only** (server = 7-day rolling ingest buffer)
- Analysis: dump→local (`db-snapshot-dump.sh` + `db-snapshot-restore.ps1`)
- No new infra spend (box stays fixed)
- Every new write path must justify IO cost; follow partition+retention pattern
- Connection counts are load-bearing; new pools require shrinking something else
- Don't raise `MAX_TRADES_RETAINED`, `SEED_TOKEN_LIMIT`, or cache TTLs on server

## Definition of done

- **Backend:** `cargo check -p backend-deploy` + `cargo check -p backend-local` clean; clippy on touched code; test when logic changed
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
