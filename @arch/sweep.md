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
| `corpus.rs` | `CorpusSource` trait + the corpus model (`TokenTrades`, `Corpus`, slim `SweepTrade` projection via `from_trades`); `Selection` (cap + time window + curve_only); `sweep_per_mint_cap`. The sole impl is `LakeSource` (see `lake/duck.rs`) — the old PG `DbSource` + Parquet corpus-cache + `attach_fingerprints` were deleted at the lake cutover |
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

Tables per strategy: `<strategy>_grouped_sweep_{runs,groups,combos,results}` — **lab-only**, defined in `lab/migrations/` and applied by `lab::storage::lab_migrations` (never on EC2/live; see [@arch/database.md](@arch/database.md)). Generic `grouped_sweep_repo.rs` (table-name-driven). Incremental writes: run header up front (`status='running'`), groups appended one at a time, finalized on completion. Crash-recovery: `reconcile_orphaned_runs` at boot. Retention filter applied write-time so only ~660 rows/group are ever inserted.

API: `POST /api/strategies/sweeps` (start, detached → 202 with `run_id`), `POST .../cancel`, `DELETE .../sweeps/{run_id}`, `PATCH .../sweeps/{run_id}` (rename), `DELETE .../sweeps?before=` (prune), `GET` for runs/groups/results.

## Parquet lake + DuckDB corpus (Phase 4 — `lab/src/lake/`)

The 3-hop analysis pipeline that feeds the sweep on the workstation:
`EC2-PG → local-PG → Parquet lake → DuckDB`.

| Hop | What | Where |
| --- | --- | --- |
| 1 · sync | Sealed daily Timescale chunks (yesterday-and-older) pulled EC2→local PG over postgres_fdw/SSH; `wallet_dict` id-preserving, no partition loop (Timescale auto-chunks) | `scripts/db-incremental-sync.ps1` |
| 2 · lake export | Each newly-sealed local day → **immutable** `trades/dt=YYYY-MM-DD/data.parquet` (write-once, temp+rename); `tokens/tokens.parquet` dimension (fingerprint cols) rewritten each run. Streamed + row-group-flushed. Units mirror `trade_repo`'s `TradeDbRow` exactly (lamports→SOL ÷1e9; raw token f64; vsol raw→f64; `real_*` dropped) | `lab/src/lake/export.rs` (`export_lake`), `lab/src/lake/mod.rs` (layout) |
| 3 · DuckDB corpus | `LakeSource: CorpusSource` (the **sole** corpus source) reads the lake via an in-memory DuckDB: candidate select + per-mint `ROW_NUMBER` cap over the trades glob (`hive_partitioning=true`), fingerprints from the dimension → `TokenTrades`/`SweepTrade`. Per-mint order is `(slot, tx_index, leg_index, block_time)` — `block_time` is the final tiebreaker because the RPC backfill path leaves `tx_index`=0 (only the live LaserStream feed sets it) and `leg_index`=0 for single-leg txs, so the first three are non-unique; the 4-tuple is unique per mint, giving a deterministic total order. Uses DuckDB's **row API** only (not `query_arrow`) so its bundled arrow never clashes with lab's `arrow 53`. Since `real_*_reserves` were dropped on export, the loader **reconstructs** `real_sol_reserves` per row from the priced reserve pair + `venue` (`approx_real_sol_reserves`: AMM→`reserve_sol`, curve→`reserve_sol−30` clamped ≥0) so the sim's real-reserve gates (tpsl2 `min_liq_sol`/organic-liq, dead-token) resolve — an approximation of the live program-emitted value, not lamport-identical | `lab/src/lake/duck.rs` |

CLI: `cargo run -p lab -- lake-export` (batch job; reads `SWEEP_LAKE_DIR`, default OS-temp `pumpfun-lake`). Add `--include-today` to also export today's still-open UTC day (force-overwritten, non-immutable) — the only way to sweep current-day data, since the default export is sealed-days-only. `duckdb = { features=["bundled"] }` is a lab-only dep (lab never ships to EC2).

**Cutover (DONE):** the lake is the **sole** grouped-sweep corpus source. The grouped-sweep handler always calls `LakeSource::new(lake_root()).load(sel)`, and `list_token_results` reloads from the lake when its warm in-memory cache (Option A) misses. The PG `DbSource`, `load_grouped_corpus`/`load_or_build`, the Parquet corpus-cache, and the separate `attach_fingerprints` pass are gone — `LakeSource` embeds fingerprints (`has_fingerprints`). Validated by a byte-identical lake-vs-PG metric diff (the divergence was a non-unique trade-order key; fixed with the `block_time` tiebreaker above). `SWEEP_CORPUS_SOURCE` is retired.

## Adding a strategy

`strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`) + `registry.rs` arm (table triple + dispatch) + `<x>_grouped_sweep_*` tables in a `lab/migrations/` SQL file + frontend param-key list + axes defs. Engine, grouping, repo, handler, and page are reused unchanged.
