# Sweep — strategy-agnostic param-sweep engine

File-level map of `backend/src/sweep/`. The generic param-sweep & backtest stack.
Related: [@arch/strategies.md](@arch/strategies.md) (pure fns the sweep reuses), [@arch/database.md](@arch/database.md) (sweep tables), [@arch/frontend.md](@arch/frontend.md) (sweep page).
Deep-dive detail: `@plans/sweep/sweep-engine-detail.md`, `@plans/sweep/sweep-metrics-explained.md`.

## Core idea

A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in the loop), folds per-(combo, token) outcomes into one ranked metrics row per combo. Engine/aggregate/grouping layers are strategy-blind — a new strategy adds only a `Strategy`/`ParamSpace` impl.

Two sweep shapes:

- **Flat sweep** (`run_sweep`) — one global ranked table. No longer exposed standalone; it is the per-group primitive the grouped sweep calls.
- **Grouped sweep** (`run_grouped_sweep`) — the primary entry point. Partition corpus by exact-value **fingerprint key**, run flat sweep per group, surface each group's best combo.

Decision parity: a strategy's `simulate` calls the same pure fns the live path uses.

## `backend/src/sweep/` (generic)

| File | Owns |
| --- | --- |
| `mod.rs` | Module map |
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `SweepMethod` (Grid/Random/LHS/Refine); entry-cache reuse (`entry_key`, `prepare_token`, `resolve_entry`, `resolve_exit`). **Re-exports** `CostModel`/`round_trip_with_costs`/`ExitCode` from `trading_core::strategies::kernel` — the sweep keeps no second copy of the cost/exit math |
| `projection.rs` | `SweepTrade` — slim per-trade row (wallet interned to `u32`); `WalletInterner`; `project_trades` |
| `corpus.rs` | `CorpusSource` trait + `DbSource` (chunked batch query); `Selection` (cap + time window + curve_only); Parquet corpus cache; `attach_fingerprints` |
| `engine.rs` | `run_sweep` — rayon over tokens; entry-cache reuse per token; `SweepObserver` cancel; buffer recycling |
| `progress.rs` | `SweepObserver` trait; `SweepProgress` (phase-tagged SSE); `NoopObserver` |
| `aggregate.rs` | `ComboAgg` (a thin wrapper over the core kernel's `RunAgg`) → `ComboMetrics` (= core `RunMetrics` + `combo_id`, via `from_run`). O(1) per combo via the core `QuantileSketch` (~0.6 KB, ~15% rel. error for median/p90) — the sketch/robust-score/exit-index math lives once in `trading_core::strategies::kernel` |
| `retention.rs` | `retained_combo_ids` — keeps per-metric-extreme combos + best_combo (~660 rows/group max); used write-time AND at compaction |
| `grouping.rs` | `TokenFingerprint`, `GroupField`, `GroupKey`; `normalize_label_vec` (shared with corpus filter) |
| `grouped_engine.rs` | `run_grouped_sweep`; two-phase driver (large groups serial, small groups parallel); `make_group_result`; coarse→refine (`run_grouped_with_refine`); partial persistence via `GroupSink` |
| `registry.rs` | `tables_for(strategy_id)`, `strategy_ids()`, `run_grouped(...)`; `MAX_COMBOS`; `sweep_base_rule_tpsl{1,2}` |
| `strategies/tpsl2.rs` | TPSL2 `Strategy`/`ParamSpace` — sweeps all 15 knobs; entry-cache by 8 scalp-gate knobs; `prepare_token` returns launch cohort once per token |
| `strategies/tpsl1.rs` | TPSL1 `Strategy`/`ParamSpace` — sweeps exit ladder only (6 knobs); param-free entry resolves once per token |

## Persistence + API

Tables per strategy: `<strategy>_grouped_sweep_{runs,groups,combos,results}`. Generic `grouped_sweep_repo.rs` (table-name-driven). Incremental writes: run header up front (`status='running'`), groups appended one at a time, finalized on completion. Crash-recovery: `reconcile_orphaned_runs` at boot. Retention filter applied write-time so only ~660 rows/group are ever inserted.

API: `POST /api/strategies/sweeps` (start, detached → 202 with `run_id`), `POST .../cancel`, `DELETE .../sweeps/{run_id}`, `PATCH .../sweeps/{run_id}` (rename), `DELETE .../sweeps?before=` (prune), `GET` for runs/groups/results.

## Parquet lake + DuckDB corpus (Phase 4 — `lab/src/lake/`)

The 3-hop analysis pipeline that feeds the sweep on the workstation:
`EC2-PG → local-PG → Parquet lake → DuckDB`.

| Hop | What | Where |
| --- | --- | --- |
| 1 · sync | Sealed daily Timescale chunks (yesterday-and-older) pulled EC2→local PG over postgres_fdw/SSH; `wallet_dict` id-preserving, no partition loop (Timescale auto-chunks) | `scripts/db-incremental-sync.ps1` |
| 2 · lake export | Each newly-sealed local day → **immutable** `trades/dt=YYYY-MM-DD/data.parquet` (write-once, temp+rename); `tokens/tokens.parquet` dimension (fingerprint cols) rewritten each run. Streamed + row-group-flushed. Units mirror `trade_repo`'s `TradeDbRow` exactly (lamports→SOL ÷1e9; raw token f64; vsol raw→f64; `real_*` dropped) | `lab/src/lake/export.rs` (`export_lake`), `lab/src/lake/mod.rs` (layout) |
| 3 · DuckDB corpus | `LakeSource: CorpusSource` reads the lake via an in-memory DuckDB: candidate select + per-mint `ROW_NUMBER` cap over the trades glob (`hive_partitioning=true`), fingerprints from the dimension → same `TokenTrades`/`SweepTrade` output as `DbSource`. Uses DuckDB's **row API** only (not `query_arrow`) so its bundled arrow never clashes with lab's `arrow 53` | `lab/src/lake/duck.rs` |

CLI: `cargo run -p lab -- lake-export` (batch job; reads `SWEEP_LAKE_DIR`, default OS-temp `pumpfun-lake`). `duckdb = { features=["bundled"] }` is a lab-only dep (lab never ships to EC2).

**Cutover (env-toggled; runtime metric-match still pending DB/EC2):** the grouped-sweep handler picks its `CorpusSource` per-request via `SWEEP_CORPUS_SOURCE` (`grouped_sweep.rs::corpus_source_is_lake`). Unset/anything-but-`lake` → the default `load_grouped_corpus` (PG + Parquet cache); `SWEEP_CORPUS_SOURCE=lake` → `LakeSource::new(lake_root()).load(sel)`. On the lake path the handler skips the separate PG `attach_fingerprints` (gated on `corpus.has_fingerprints`, which `LakeSource` sets). The toggle keeps PG the default so the *same* sweep can run both ways for the Phase-4 "done-when" check — lake-sourced `strategy_run_metrics` must match a PG-sourced baseline — which still requires the live pipeline to produce data. Flip the default to lake once that diff is green.

## Adding a strategy

`strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`) + `registry.rs` arm (table triple + dispatch) + `<x>_grouped_sweep_*` migration + frontend param-key list + axes defs. Engine, grouping, repo, handler, and page are reused unchanged.
