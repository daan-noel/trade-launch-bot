# Plan: Split backend into `deploy` and `local` runtime modes

## Context

The project runs in two very different ways:

- **EC2 (deploy):** realtime LaserStream ingest + persist + real trading + tx tracking. Low latency is the only thing that matters.
- **Local:** offline analysis, simulation, group-sweeping over Postgres dumps pulled from EC2. No ingest, no real trading.

Today it's a **single `backend` binary with no mode awareness** — `cargo run -p backend` always starts ingest + live strategy execution + trading + the full HTTP surface (live *and* sweep/analysis endpoints). The two concerns are already **loosely coupled** in code (ingest/trading are long-lived tasks on the `hot` pool; sweep/backtest/analysis are HTTP-triggered jobs on the `batch` pool that read `trades` read-only and never feed the live loop), but nothing enforces the separation. Consequences: the 4GB EC2 box can be made to run a heavy sweep, and locally you can't run analysis without also firing up ingest/trading.

**Goal of this deliverable:** introduce a single runtime mode switch so each environment starts only the subsystems it needs. ClickHouse and Redis are explicitly **deferred** — this is the foundational seam they'd later hang off.

**Chosen approach (per decisions):** runtime env flag (not Cargo features, not separate crates). One binary everywhere; gate task-spawns and route registration at startup. Trade-off accepted: the EC2 binary still *contains* the analysis code, but it never *runs* or *exposes* it.

## Design

Add `APP_MODE` env var with two values:

- `deploy` (default if unset) — ingest + strategies + trading + live API. Current EC2 behavior, unchanged.
- `local` — analysis/sweep/backtest API only. No ingest, no strategy runner, no real trading.

Represent as an enum in config, resolved once at startup, threaded through `AppState`, and read at the three seams: **task spawning** (`main.rs`), **route registration** (`api/mod.rs`), and **live-only AppState handles**. This mode is coarser than the existing `HTTP_ENABLED` / `LIVE_MODE` toggles and composes with them (e.g. `LIVE_MODE` pause still works within `deploy`).

## Changes by area

### 1. Config — `backend/src/config/settings.rs`
- Add `AppMode { Deploy, Local }` enum + a `mode` field on the settings struct, parsed from `APP_MODE` (default `Deploy`; unknown value = hard error at startup, fail fast).
- Add helpers `is_deploy()` / `is_local()`.
- On boot, log the resolved mode loudly — especially `>>> APP_MODE=deploy: LIVE INGEST + REAL TRADING ENABLED <<<` so it's unmistakable on EC2.

### 2. Task gating — `backend/src/main.rs` (`tokio::select!`, ~lines 550–936)
Wrap spawns by mode. The probe subcommand path is unaffected (it already exits before this block).

- **`deploy` only:** ingest tasks (producer, pipeline, db_writer, pool-subscription refresh, queue-depth logger), partition maintenance (`ingest_laserstream/maintenance.rs`), `strategy_task` (StrategyRunner), wallet reconciliation, SOL balance cache refresh.
- **Both modes:** HTTP server (routes themselves gated in step 3), token/token-list cache seeding (analysis reads need it), SOL price poller (keep; cheap, used by UI/analysis — can revisit).
- **`local` only:** nothing new to spawn — sweep/backtest are HTTP-triggered, not tasks. Local just *omits* the deploy tasks.

### 3. Route gating — `backend/src/api/mod.rs` (`configure()`, lines 6–389)
Read mode from `AppState` and register routes by concern. Principle: **live mutation/trading = deploy; sweep/backtest/analysis = local; read-only metadata = both.**

- **`deploy` only:** `/strategies/tpsl{1,2}/rules` lifecycle (activate/pause/stop), `/strategies/tpsl{1,2}/positions`, `/solana/wallet/*`, `/cashback/*`, `/system/live`, `/token/sync`.
- **`local` only:** `/strategies/sweeps*`, `/tokens/{mint}/swings` + `/tokens/swings/batch`, `/analysis`, `/jobs/swings/*`, `/jobs/simulations/*`, and the per-rule **`/strategies/tpsl{1,2}/rules/{id}/simulate`** backtest (it's analysis even though it sits under the rules path — register it separately from the lifecycle routes).
- **Both:** `/tokens` (+ `/tokens/{mint}`), `/stream` (SSE — in `local` it simply carries no live trade/position frames since ingest isn't running, no code change needed), `/system/settings`, `/system/health`, `/jobs/status`, read-only `GET` of rules.

### 4. Live-only AppState handles — `backend/src/state/app_state.rs`
Most fields are cheap to construct in `local` and can stay as-is (empty `token_cache`, `tpsl{1,2}_cache`, `pool_index` DashMap, `trade_signals` Notify hub — just never updated live). The one field that needs real keys + RPC to construct is `trader: Arc<PumpFunTrader>`.

- **Recommended:** make it `Option<Arc<PumpFunTrader>>`, constructed only in `deploy`. Its consumers (wallet/trading handlers, StrategyRunner) are all deploy-gated already, so they become `.expect("trader present in deploy mode")` / `if let Some`.
- **VERIFY FIRST (key risk):** confirm the sweep/backtest curve simulation (`simulate_curve_buy/sell` in `pump-trader`) are free/associated fns — **not** methods on a constructed `PumpFunTrader` instance. If they need an instance, `local` must build an offline trader (no keypair/RPC) instead of `None`; in that case prefer constructing a minimal offline `PumpFunTrader` over `Option`-wrapping. Resolve this before touching the struct.

### 5. Frontend capability gating (lighter half) — `frontend-react/`
- Add `GET /api/system/capabilities` → `{ mode, has_live_trading, has_analysis }` (always-on route; reads `AppState.mode`).
- Frontend fetches it once at boot and conditionally renders nav + lazy routes in `src/App.tsx`: hide live pages (`Tpsl{1,2}Page`, `MyWalletPage`, trading bits of `SettingsPage`) in `local`; hide sweep/analysis pages (`AnalysisPage`, `SwingDetectionPage`, `Tpsl{1,2}GroupedSweepPage`) in `deploy`.
- Backend selection is unchanged — `VITE_DEV_PROXY_TARGET` in `vite.config.ts` already points the local frontend at either the local backend or EC2.

### 6. `.env` / `.env.example`
- Add `APP_MODE=deploy` to `.env.example` with a comment (`deploy` on EC2, `local` for analysis).
- **Per the env-backup rule:** `Copy-Item .env .env.backup -Force` **before** editing `.env`. Set `APP_MODE=local` in the local `.env`; EC2 `.env` gets `APP_MODE=deploy` (or leave unset → default deploy).

## Docs to update (Definition of Done)
- **CLAUDE.md** — document `APP_MODE`, the two modes, and which subsystems each runs (rules/commands/constraints change). Add to the Commands and Deployed-server sections.
- **@arch/architecture.md** — note that `main.rs` task wiring + `api/mod.rs` route registration are now mode-gated.
- **@plans/** — add `@plans/modes/mode-split.md` capturing the design + the trader/curve-math resolution.

## Verification
1. `cargo check --bin backend` clean; clippy on touched files.
2. **Local:** `APP_MODE=local cargo run -p backend` → logs show *no* ingest/strategy/trader spawns; HTTP up; `POST /api/strategies/sweeps` works; `POST /api/solana/wallet/buy` returns 404/disabled; `GET /api/system/capabilities` → `has_analysis:true, has_live_trading:false`.
3. **Deploy:** `cargo run -p backend` (unset) and `APP_MODE=deploy` → ingest + trading + live routes behave exactly as today; `POST /api/strategies/sweeps` returns 404/disabled; capabilities → `has_live_trading:true, has_analysis:false`.
4. Confirm `local` mode boots with no trading keys / no HELIUS gRPC reachable (proves the deploy-only deps aren't constructed).
5. **Frontend:** `npm run build` clean; nav hides the other mode's pages against each backend; no extra re-render on SOL/USD tick or live-trade stream.

## Out of scope (deferred)
- ClickHouse (decide later / measure first — confirm corpus-load + creation-stats SQL is actually the bottleneck at scale before adding an additive `trades`/`tokens_info` mirror behind the `local` path).
- Redis (deferred; in-memory + Parquet caches already cover single-process local mode).
- Cargo feature flags / crate split (chose runtime flag; can layer features later on this same seam if EC2 binary leanness becomes a priority).
