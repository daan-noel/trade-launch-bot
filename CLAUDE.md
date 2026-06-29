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

Six Rust workspace crates + `frontend-react` SPA. The old single `backend` crate was split into two bins over a shared core, then renamed to the `live`/`lab` topology (see [@plans/modes/crate-split.md](@plans/modes/crate-split.md) and [live-lab-remake-plan.md](live-lab-remake-plan.md)):

| Crate | Kind | Role |
| --- | --- | --- |
| `trading_core` | lib | config, models, storage, core services/state (`CoreState`), api framework + auth + SSE bridge, core handlers, strategy domain (`tpsl_rules_core`), **ingest contract** (`trading_core::ingest`) |
| `pump-trader` | lib | buy/sell executor; **standalone drop-in library** (no workspace deps). Three tiers: `protocol` (Tier-1 `const Pubkey` invariants), `config` (Tier-2 `TraderConfig` + 7 `Default` sub-structs), per-call args. Signs via `Arc<dyn Signer>` (HSM/remote-ready); typed `error::TradeError` (no `anyhow`). `probe`/`claim` are off-by-default cargo features; `constants.rs` is a thin back-compat shim |
| `ingest-laserstream` | lib | Helius LaserStream gRPC transport (client→pipeline→db_writer) + watchdog; exposes `spawn(…) -> IngestHandles` |
| `ingest-websocket` | lib | **empty scaffold** — `spawn(…) -> IngestHandles` stub mirroring laserstream so `live` can swap transports later (not yet implemented) |
| `live` | **bin** | LIVE box: strategies, trader, deploy services/state (`DeployState`), live/trading handlers, `probe`. Ships to EC2 |
| `lab` | **bin** | ANALYSIS box: sweep/backtest, swing analyzer, local state (`LocalState`), rule-authoring + sweep handlers. Runs with NO keys / NO gRPC; never depends on `pump-trader` |

The transport-agnostic ingest contract (`IngestHandles`, `TraderHook`, re-exports of `StrategyPing`/`TradeSignals`) lives in `trading_core::ingest`; both ingest crates depend on `trading_core` and expose the same `spawn(…)`. `ingest-laserstream` re-exports the contract types for back-compat.

Each bin is its own composition root (`tokio::select!` over long-lived tasks). `live/main.rs` calls `ingest_laserstream::spawn(…)` + trading + strategy + HTTP; `lab/main.rs` is thin (SOL-price poller + token-cache seed + HTTP, no ingest/trader). Helius LaserStream (gRPC) is the **sole** live transport; the `trades` table *is* that feed. Both serve `configure_core_routes` plus their own route config. The frontend split is build-time, so there is no runtime capability advertisement — each bin builds into its own SPA with a static nav. **The frontend uses `live`/`lab` vocabulary throughout** (`@live`/`@lab` aliases, `src/live`/`src/lab` trees, `liveApi`/`labApi`).

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
cargo check -p live                    # typecheck the live bin
cargo check -p lab                     # typecheck the analysis bin
cargo check -p trading_core            # typecheck the shared lib
cargo test  -p live                    # live unit tests (strategies, trader edge)
cargo test  -p lab                     # lab unit tests (sweep, swing)
cargo test  -p live -- --ignored       # integration; needs DATABASE_URL + HELIUS_RPC_URL
cargo test  -p pump-trader             # trader crate tests
cargo run   -p live                    # live box: loads .env; needs Postgres + Helius gRPC
cargo run   -p lab                     # analysis box: needs Postgres; NO keys / NO gRPC
cargo run   -p lab -- lake-export       # batch: export sealed days local-PG -> Parquet lake ($SWEEP_LAKE_DIR)
cargo run   -p live -- probe <ladder|fanout|simulate-sell|holdings> [args]
cd frontend-react; npm run dev         # both apps concurrently: live :5173, lab :5174 (separate dev servers)
npm run dev:live                       # live app only (:5173, proxies /api -> live bin :8081)
npm run dev:lab                        # lab app only  (:5174, proxies /api -> lab bin :8082) — pair with `PORT=8082 cargo run -p lab`
npm run build                          # tsc (checks BOTH trees) && vite build (live config) → LIVE-ONLY dist/index.html
```

**Frontend is two apps over a shared core** (mirrors the backend two-bin split): `src/shared` ·
`src/live` (`@live/*`) · `src/lab` (`@lab/*`), two Vite entries + **two dev servers**
(`index.html`→live `vite.live.config.ts` :5173, `lab.html`→lab `vite.lab.config.ts` :5174;
`lab.html` is dev-only — never built for prod). Each app runs independently (`dev:live`/`dev:lab`)
or both at once (`dev`). Mode is build-time, not runtime — no `useCapabilities` gating. Ship the
**live** build to EC2 (`npm run build` emits lab-free `dist/index.html`). One split `createApi`:
`baseApi` shell + per-mode `injectEndpoints`; import mode hooks from `@live|@lab/store/*Endpoints`,
never the shared `store/apiSlice` barrel, so a mode's side effect never leaks across builds. See
[@arch/frontend.md](@arch/frontend.md).

Stay in the owning crate (`trading_core` / `pump-trader` / `ingest-laserstream` / `ingest-websocket` / `live` / `lab`). Use `--target-dir target-check` if a bin `.exe` is running (locks `target/`). Clippy `too_many_arguments` is `#[allow]`-ed on trade-path fns by design.

## Performance budgets (hot path — violation = bug)

- **Sell-confirm:** no new RPC call — confirm via the `trades` gRPC feed. An RPC poll reintroduces latency + double-sell risk.
- **Ingest pipeline:** no blocking I/O, `.await`-on-lock, or unbounded per-event alloc. DB/SSE writes through channels only.
- **Strategy eval:** read from `runtime_cache.rs` (in-memory), never DB-per-event.

## Data-scale guardrails

- Bound every query — paginate/time-window/stream. Never `SELECT *` the full `trades`/`raw_txs`.
- New high-volume tables are **TimescaleDB hypertables** with declarative `add_compression_policy` + `add_retention_policy` (defined in `0001_init.sql`); continuous aggregates are set up at boot by `trading_core::storage::timescale`. The old hand-rolled `maintenance.rs` partition loop is gone.

## Deployed server (EC2: 2vCPU / 4GB RAM — IO-bound, RAM-constrained)

- **Ship `live` + `ingest-laserstream` to EC2 only.** `lab` (sweep/arrow/parquet/rayon + bundled `duckdb`, and the `lab/src/lake/` Parquet-lake pipeline) stays on the workstation — never deploy it.
- Sweeps/backtests: **local only** (server = 7-day rolling ingest buffer)
- Analysis: server→local DB sync (`scripts/db-incremental-sync.ps1` — incremental DB→DB over an SSH tunnel)
- No new infra spend (box stays fixed)
- Every new write path must justify IO cost; follow partition+retention pattern
- Connection counts are load-bearing; new pools require shrinking something else
- Don't raise `MAX_TRADES_RETAINED`, `SEED_TOKEN_LIMIT`, or cache TTLs on server

## Definition of done

- **Backend:** `cargo check -p live` + `cargo check -p lab` clean; clippy on touched code; test when logic changed
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
- `tpsl_sniper_1`/`tpsl_sniper_2` **decision** modules (`entry`/`exit`/`cohort`, in `trading_core`) are intentional clones — a fix in one usually belongs in both. (The live *orchestration* is no longer cloned: Phase 3 unified it into one registry-dispatched `live/src/strategies/{service,runner,execution}`.)
- `.env` required (see `.env.example`); secrets/keys there only, never in code.
