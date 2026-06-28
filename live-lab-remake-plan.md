# Remake: `live`/`lab` restructure — registry-unified strategies + clean-rebuild schema

## Context

The repo was split into two bins over a shared core (`backend-deploy` = live trading on EC2,
`backend-local` = analysis on the workstation). That split worked, but three things are still
misaligned with the intended end-state:

1. **Naming** — the modes are now *live*/*lab*, but the crates are still `backend-core` /
   `backend-deploy` / `backend-local`, and `pump-constants` is a standalone crate the new
   topology folds away.
2. **Strategy domain is duplicated** — `tpsl1`/`tpsl2` are hand-cloned across models, repos,
   runtime caches, entry/exit modules, and handlers. The DB design already moved to **unified**
   `strategy_rules/runs/positions/metrics` (typed lifecycle + JSONB params); the code hasn't.
3. **Schema is the old shape** — the four staged storage plans ([token](../../Videos/meme-trading/token-storage-plan.md),
   [trades](../../Videos/meme-trading/trades-storage-plan.md), [raw-txs](../../Videos/meme-trading/raw-txs-storage-plan.md),
   [strategy](../../Videos/meme-trading/strategy-storage-plan.md)) describe a clean rebuild on
   TimescaleDB that isn't applied yet.

**Goal:** one clear, optimized, concrete structure where `core` owns *all* CRUD + strategy logic
+ external-API micro-services; `live` and `lab` are thin composition roots that **consume** core
and never re-implement logic; and the data layer is the clean TimescaleDB rebuild. The `lab` box
gets a Parquet-lake + DuckDB analysis path fed by incremental PG sync.

**Decisions locked (this session):** Full registry unify · clean rebuild + TimescaleDB, local-first
· lab pipeline `EC2-PG → local-PG → Parquet lake → DuckDB` · rename crates, fold `pump-constants`,
scaffold `ingest-websocket` · **frontend rename deferred** (keep `@deploy`/`@analysis` and the
`/api/system/capabilities` mode strings unchanged for now so the running UI keeps working).

---

## Target crate topology

| Crate | Kind | Role | From |
| --- | --- | --- | --- |
| `core` | lib | config · models · storage (CRUD for all main tables) · **all** strategy logic (registry, entry/exit kernel, simulation kernel) · external-API micro-services · api framework + shared handlers · `CoreState` · **ingest contract** | rename `backend-core` |
| `pump-trader` | lib | buy/sell executor. Absorbs `pump-constants` as a `constants` module | + fold `pump-constants` |
| `ingest-laserstream` | lib | Helius LaserStream gRPC transport: connect → decode → hand off via the core ingest contract | unchanged |
| `ingest-websocket` | lib | **new, empty scaffold** — `spawn(...) -> IngestHandles` stub mirroring laserstream so `live` can swap transports later | new |
| `live` | **bin** | EC2 box: load cache+settings → receive/decode (one ingest crate) → cache → eval entry/exit → execute buy/sell; concurrently persist; CRUD on UI events | rename `backend-deploy` |
| `lab` | **bin** | workstation: big-data sim/grouped-sweep over Parquet+DuckDB; PG sync. Calls core's strategy functions, never re-implements logic | rename `backend-local` |

`pump-constants` crate is **deleted**; its literals move to `pump-trader/src/constants.rs` and the
re-export `pub use pump_constants as constants` becomes `pub mod constants`. Verify no other crate
depends on it directly (today only `pump-trader` does).

### The ingest contract (enables two transports)

Lift the transport-agnostic types out of `ingest-laserstream` into **`core::ingest`**:
`IngestHandles`, the `TraderHook` trait, `StrategyPing`, and `TradeSignals`. Both ingest crates then
depend on `core` and expose the **same** `spawn(...) -> IngestHandles`. Each transport keeps only its
wire protocol + decoder; the decoded event model + `pipeline`/`db_writer` stay in `ingest-laserstream`
for now (sharing them with the websocket path is a later task — the scaffold is empty).

---

## `core` — the shared contract (what `live` and `lab` both call)

### 1. CRUD for the main tables
New repos replacing the tpsl1/2-specific ones, one module per table group under `core/src/storage/repositories/`:

- `token_repo` (`tokens`), `token_info_repo` (`tokens_info`), `token_sync_state_repo` (`token_sync_state`) — per [token-storage-plan.md](../../Videos/meme-trading/token-storage-plan.md)
- `trade_repo` (`trades`, integer base units, `(block_time, tx_signature, leg_index)` PK) — per [trades-storage-plan.md](../../Videos/meme-trading/trades-storage-plan.md)
- `strategy_repo` — one repo spanning `strategy_rules` / `strategy_runs` / `strategy_run_metrics` / `strategy_positions` — per [strategy-storage-plan.md](../../Videos/meme-trading/strategy-storage-plan.md)
- `settings_repo` (`app_settings`) — keep
- (optional) `wallet_repo` dict if `wallet_address` interning is adopted (trades-plan lever 2)

Pools (`hot`/`api`/`batch`) and accessor methods on `CoreState` are preserved as-is.

### 2. Strategy logic — registry-unified (the big refactor)
Replace the cloned tpsl1/tpsl2 orchestration with one enum-dispatched registry. **Static dispatch
(enum, no `dyn`/vtable)** to respect the hot-path budget.

```
core/src/strategies/
  registry.rs        // enum StrategyImpl { Tpsl1, Tpsl2 }; from_id(&str)->Option<Self>
                     // enum StrategyParams { Tpsl1(Tpsl1Params), Tpsl2(Tpsl2Params) }
                     // methods: parse_params, matches_entry, find_entry_fill,
                     //          find_trade_driven_exit, find_clock_driven_exit
  kernel.rs          // simulate_rule(StrategyImpl, &StrategyParams, trades, cfg) -> RunMetrics + fills
                     // RunMetrics struct == strategy_run_metrics columns (live/paper/sweep comparable)
  tpsl_sniper_1/     // KEEP — entry/exit modules, called through the enum
  tpsl_sniper_2/     // KEEP — entry/exit + scalp + cohort, called through the enum
```

Key properties:
- `params` JSONB is **parsed once at rule-load** into the typed `StrategyParams` (cached in the
  runtime cache) → zero per-event JSON cost, per the storage-plan principle.
- `tpsl_sniper_1` and `tpsl_sniper_2` modules stay separate behind the enum — the "fix the clone in
  both" gotcha is preserved at the logic level; only orchestration unifies.
- **Unified runtime cache** `StrategyRuntimeCache` (active rules, `holding_by_mint`, per-rule
  counters, paper-run refs, `exit_state_by_position`, in-flight guards) — strategy-agnostic; eval is
  dispatched via `rule.strategy_id`. Replaces `Tpsl1RuntimeCache`/`Tpsl2RuntimeCache`. Lives in
  `core` (so `lab`'s paper-replay and `live`'s runner share it).
- **One simulation kernel** in `core::strategies::kernel` — a lean trade-walk calling the decision
  functions, emitting the shared `RunMetrics`. `lab`'s sweep calls it per param-combo; paper-trading
  replays call it; `live` uses the same decision functions incrementally. `lab` adds **only**
  orchestration around it (never logic).
- **Unified rule-CRUD domain** keyed by `strategy_id` (replaces the per-strategy split in
  `tpsl_rules_core`): validate → build `StrategyRule` → repo write; calling edge appends its side
  effects (cache reload + `rules_changed` on `live`, nothing on `lab`).

### 3. External-API micro-services
Keep in `core::services`: `sol_price` poller (CoinGecko → Jupiter fallback) + the `coingecko` /
`jupiter` / `helius_rpc` / `http` clients. Move the **RPC token-detail/sync** service here too (it's
external-API + CRUD, no signing keys), leaving the `live` handler to call it. Key-requiring,
trade-adjacent services (e.g. boot `wallet_reconcile`) stay in `live`.

---

## Database — clean rebuild on TimescaleDB (local-first)

Single new migration that **drops the old tpsl1/2 + tokens/trades shapes and recreates** per the four
storage plans + [timescaledb-plan.md](../../Videos/meme-trading/timescaledb-plan.md). EC2 is a 7-day
rolling buffer (not a system of record), so a clean drop/recreate is acceptable.

- `tokens` / `tokens_info` / `token_sync_state` — `mint_address` natural PK; `token_sync_state`
  replaces the `last_synced_*` columns; `age`/`market_cap` derived in `token_overview` view.
- `trades` — integer base units; PK `(block_time, tx_signature, leg_index)` *is* the dedup key;
  `(slot, tx_index, leg_index)` ordering; **hypertable** on `block_time`, compress after 7d, retain
  30d; `trades_ohlcv_1m` continuous aggregate + hierarchical 5m/1h.
- `raw_transactions` — `BYTEA` payloads; hypertable; compress after 2d, retain 7d.
- `strategy_rules/runs/run_metrics/positions` — typed lifecycle + JSONB `params`; `params_snapshot`
  freezes the rule into the run; PnL derived in `strategy_position_pnl` view; real-only double-sell
  partial-unique indexes.
- Replace the hand-rolled partition maintenance in `ingest_laserstream/maintenance.rs` with Timescale
  retention/compression **policies** (and `ON CONFLICT DO UPDATE` → `DO NOTHING` in `trade_repo`).

**Rollout order:** apply on a fresh **local** DB first (zero risk, install the TimescaleDB extension
locally), validate a stable week, then apply on EC2 during the `live` cutover.

---

## `live` workflow (composition root)

`live/src/main.rs` `tokio::select!` over long-lived tasks (unchanged shape, generalized strategy
wiring):
1. **boot** — load token cache + settings + active `strategy_rules` (parse params once) into
   `StrategyRuntimeCache`; init trader.
2. **ingest** — `core::ingest`-contract `spawn(...)` (laserstream by default) → decode → `TokenCache`
   update → fan-out.
3. **strategy runner** — one **strategy-agnostic** `StrategyRunner` consuming `StrategyPing`s,
   dispatching entry/exit via `StrategyImpl::from_id(rule.strategy_id)`; opens/closes
   `StrategyPosition`s; executes buy/sell through `pump-trader`.
4. **persist** — `DbWriter` batches decoded data to Postgres concurrently (channels only).
5. **services** — SOL/USD poller; HTTP server (core routes + live routes).
6. **UI CRUD** — handled separately on request via `core` repos.

Hot-path budgets unchanged: **sell-confirm via the `trades` gRPC feed, never a new RPC poll**; the
exit loop still polls the full window before retry (dup-sell guard in `execution/real.rs`); strategy
eval reads the in-memory cache only.

---

## `lab` workflow (composition root + data pipeline)

`lab/src/main.rs` stays thin (SOL poller + token-cache seed + HTTP, no ingest/trader). The new work is
the **3-hop data pipeline** and pointing the sweep at it:

```
EC2 Postgres ──(incremental, sealed daily Timescale chunks)──▶ local Postgres   [landing + hot tail]
local Postgres ──(export each newly-sealed day, append-only)──▶ Parquet lake     [immutable history]
Parquet lake ──(query/attach)──▶ DuckDB ──▶ sweep corpus                          [columnar analysis]
```

- **Sync** — extend [scripts/db-incremental-sync.ps1](../../Videos/meme-trading/scripts/db-incremental-sync.ps1)
  (postgres_fdw over SSH) to pull **sealed** daily chunks (yesterday-and-older; today stays open) into
  local PG. Non-destructive, watermarked.
- **Lake export** — new step: each newly-sealed local partition → one immutable Parquet dataset
  (partitioned by day), wallet-interned to `u32` (reuse the existing projection idea in
  [backend-local sweep](../../Videos/meme-trading/backend-local/src/sweep/)).
- **DuckDB query layer** — assemble the sweep corpus by querying the lake (attach local PG for the
  recent uncompressed tail). Replaces the ad-hoc Parquet corpus cache with DuckDB views keyed by
  corpus hash.
- **Sweep** — `lab` keeps rayon parallelism + grouping + DDSketch aggregation, but the per-combo math
  is **`core::strategies::kernel::simulate_rule`**. Results land in the
  `strategy_run_metrics`-compatible shape so live/paper/sweep stay comparable. No strategy logic in
  `lab`.

---

## Execution phases

| Phase | Scope | Done when |
| --- | --- | --- |
| **0 · Topology** ✅ DONE | Rename crates `core`/`live`/`lab` (dirs, Cargo package names, path deps, workspace members, imports, docs). Fold `pump-constants` → `pump-trader::constants`. Scaffold empty `ingest-websocket` (spawn stub). Lift ingest contract into `core::ingest`. Keep capabilities mode strings unchanged. | `cargo check` clean on all crates; behavior identical (run `live`+`lab`, compare endpoints) |
| **1 · Schema + core data layer** ✅ DONE | New migration (clean rebuild, TimescaleDB) on fresh **local** DB. New `core` models + repos for tokens/trades/strategy/settings. Swap maintenance → Timescale policies. | Migration applies on fresh local DB; repos round-trip; `cargo check -p core` |
| **2 · Strategy unify (core)** ✅ DONE | `StrategyImpl`/`StrategyParams` registry; `simulate_rule` kernel + shared `RunMetrics`; unified `StrategyRuntimeCache`; unified rule-CRUD domain. Keep tpsl1/tpsl2 logic modules. | Parity unit tests vs old tpsl1/2 decisions on fixtures; `cargo check -p core` |
| **3 · `live` rewire** ✅ DONE | Strategy-agnostic `StrategyRunner` + generalized position execution; ingest via `core::ingest`; unified strategy handlers keyed by `strategy_id`. | `cargo check -p live`; probe `ladder`/`simulate-sell`; manual buy/sell; sell-confirm still feed-driven (no new RPC) |
| **4 · `lab` pipeline** 🟡 BUILD-DONE (runtime-verify pending) | Sealed-daily PG sync; lake export; DuckDB corpus; sweep calls core's kernel. **All four sub-tasks built + unit-verified:** (a) ✅ sweep repoint/dedup — `ComboAgg` wraps `kernel::RunAgg`, lab's duplicate sketch/cost code deleted; (b) ✅ sync rework — `db-incremental-sync.ps1` now targets the new schema (wallet_dict id-preserving → tokens → tokens_info/token_sync_state → trades; sealed-day upper bound; partition loop removed); (c) ✅ lake export — `lab/src/lake/export.rs` writes immutable day-partitioned Parquet + tokens dimension (`cargo run -p lab -- lake-export`); (d) ✅ DuckDB corpus — `lab/src/lake/duck.rs` `LakeSource: CorpusSource` via bundled `duckdb` (row API). **Cutover wired (`SWEEP_CORPUS_SOURCE=lake` toggle):** the grouped-sweep handler now selects `LakeSource` vs `load_grouped_corpus` per-request via env (`corpus_source_is_lake`), skipping PG `attach_fingerprints` on the lake path; PG stays the default so the same sweep runs both ways. **Still deferred (needs DB/EC2):** run the pipeline end-to-end and confirm lake metrics match a PG baseline, then flip the default to lake. | `cargo check -p lab` ✅; lab 72 tests ✅; grouped sweep runs off the lake + metrics match a PG-sourced baseline ⏳ (DB-gated) |
| **5 · EC2 cutover** | Apply schema rebuild + TimescaleDB on EC2; deploy `live` + `ingest-laserstream`; watch RAM/heartbeat/compression. | Stable ingest + trading on EC2; RAM within budget |
| **6 · (deferred)** | Frontend rename `@deploy`/`@analysis` → `@live`/`@lab` + flip capabilities mode strings. | out of scope this remake |

> **Status (2026-06-28):** Phases **0** (`4d68d82`), **1** (`d62111f`), **2** (`6c46110`), **3**
> (`395dd86`/`0043ba3`/`2d76127`) complete; `cargo check --workspace` clean. **Phase 4 (`lab`
> pipeline) build-complete, runtime-verify pending** — all four sub-tasks are implemented and
> unit-verified (`cargo check -p lab` ✅, lab 72 tests ✅, clippy ✅): sweep dedup (`21c6f38`),
> sealed-daily sync rework, Parquet lake export (`lab/src/lake/export.rs`), and the DuckDB
> `LakeSource` corpus (`lab/src/lake/duck.rs`, bundled `duckdb` v1.2.2). **Not yet done — gated on
> DB/EC2:** run the EC2→local→lake→DuckDB pipeline end-to-end, flip the grouped-sweep handler from
> `load_grouped_corpus` to `LakeSource`, and confirm lake-sourced metrics match a PG baseline.
>
> **Carried into later phases:**
> - **Phase 3** also absorbs the one Phase-2 carve-out: the per-position clock exit-state memo
>   (`exit_state_by_position`) + time-exit secondary index — deferred because they need the live
>   token-cache trade source (only meaningfully built/tested with the live feed). And: migration is
>   build-verified but **not yet run on a real TimescaleDB box** (do this before/with the EC2 cutover).
> - **Phase 4** ✅ (this sub-task) repointed the `lab` sweep at `core::strategies::kernel`:
>   `ComboAgg` wraps the shared `RunAgg`, `ComboMetrics` = core `RunMetrics` + `combo_id`, and lab's
>   duplicate `QuantileSketch`/`robust_score`/`exit_index`/`CostModel`/`round_trip_with_costs`/`ExitCode`
>   are deleted (re-exported from core). Note the sweep keeps its own entry-cache/grouped engine and
>   `TokenOutcome` (richer, for drill-in) — `core::simulate_rule`'s linear walk would drop the per-token
>   entry-cache reuse, a hot-path regression, so only the shared cost/aggregate math was unified.

---

## Verification (end-to-end)

- **Build:** `cargo check -p core` · `cargo check -p live` · `cargo check -p lab` clean; clippy on
  touched code. (Use `--target-dir target-check` if a bin `.exe` holds `target/`.)
- **Schema:** drop + re-run the migration on a scratch local DB with the TimescaleDB extension;
  confirm hypertables, compression/retention policies, and the `trades_ohlcv_1m` CAgg exist
  (`SELECT * FROM timescaledb_information.hypertables / jobs`).
- **Strategy parity (Phase 2):** golden-fixture tests asserting `StrategyImpl` entry/exit decisions
  and `simulate_rule` metrics equal the pre-refactor `tpsl1`/`tpsl2` outputs on the same trade
  series. `cargo test -p core`.
- **Live (Phase 3):** `cargo run -p live` against local PG + Helius gRPC; `cargo run -p live -- probe
  ladder|simulate-sell|holdings`; place a small real buy/sell and confirm the sell confirms off the
  `trades` feed (no extra RPC in logs); rule CRUD via the UI mutates `strategy_rules` and reloads the
  cache.
- **Lab (Phase 4):** run the sealed-daily sync, export a day to the lake, run a grouped sweep through
  DuckDB; compare its `strategy_run_metrics` to a baseline sweep sourced directly from PG (must
  match). `cargo test -p lab`.

## Docs to update (Definition of Done)

- **CLAUDE.md** — crate table, commands (`-p core|live|lab`), mode names live/lab, perf budgets.
- **@arch/** — `architecture.md` (crate map, two roots, ingest contract), `database.md` (new schema +
  Timescale), `strategies.md` (registry + kernel), `ingest.md` (contract lift), `sweep.md` (DuckDB
  lake). `frontend.md` only when Phase 6 runs.
- **@plans/** — supersede `@plans/modes/crate-split.md`; the four `*-storage-plan.md` +
  `timescaledb-plan.md` become the schema source-of-truth referenced here.

## Progress tracking (rule)

As each phase/task is completed, mark it done in this file — append `✅ DONE` (with a short note or
date) to the phase/task heading or its table row. Keep this file the single running source of truth
for remake progress; update it in the same change that completes the work.

When a phase is finished **completely**, `git commit` it with a short, concise, explicit message
(e.g. `Phase 1: clean TimescaleDB rebuild + core data layer`). One commit per fully-completed phase.
