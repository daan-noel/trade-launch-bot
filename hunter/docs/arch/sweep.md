# Sweep — strategy-agnostic param-sweep engine

File-level map of `backend/src/sweep/`. The generic param-sweep & backtest stack.
Related: [@arch/strategies.md](@arch/strategies.md) (pure fns the sweep reuses), [@arch/database.md](@arch/database.md) (sweep tables), [@arch/frontend.md](@arch/frontend.md) (sweep page).
Deep-dive detail: `@plans/sweep/sweep-engine-detail.md`, `@plans/sweep/sweep-metrics-explained.md`.

## Core idea

A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in the loop), folds per-(combo, token) outcomes into one ranked metrics row per combo. Engine/aggregate/grouping layers are strategy-blind — a new strategy adds only a `Strategy`/`ParamSpace` impl.

Two sweep shapes:

- **Flat sweep** (`run_sweep`) — one global ranked table. No longer exposed standalone; it is the per-group primitive the grouped sweep calls.
- **Grouped sweep** (`run_grouped_sweep`) — the primary entry point. Partition corpus by a **fingerprint key** (discrete fields exact-value; continuous SOL amounts binned into fixed-width ranges), run flat sweep per group, surface each group's best combo.

Decision parity: a strategy's `simulate` calls the same pure fns the live path uses.

## The sweep is an approximate ranking tool (deliberate)

**The sweep ranks candidates; `simulate` is the authority on any single combo's PnL.**
Several divergences from `simulate`/live are intentional trades of fidelity for speed —
they are listed here so a future session does not "fix" them:

| Divergence | Why it exists |
| --- | --- |
| **Tail-cap asymmetry.** Sweep caps a token's series at `last_trade + DEAD_QUIET + TAIL_MARGIN`; replay caps at the corpus-wide last trade. A quiet-but-liquid token therefore reads `Open` in sweep and closed `Metrics` in simulate. | Per-token tails keep the series short; a corpus-wide horizon would extend every token to the newest trade in the run |
| **Concurrency caps stripped.** Sweep runs `max_concurrent_tokens: u32::MAX`, so `n_fired` / `total_pnl_sol` are **upper bounds** vs. what a live rule under its own caps would achieve. | Caps make token outcomes order-dependent, which would serialize the rayon token fan-out |
| **Sketched quantiles.** Persisted quantiles come from a 64-bucket DDSketch (~15% rel. error); `simulate` and the sweep drill-in compute exact ones. **Ranking is unaffected** — `score` is exact. | O(1) memory per combo; exact quantiles would need every per-token value retained |
| **`pnl_percent` is not notional-invariant.** `fixed_cost_sol_per_leg` does not scale with trade size, so PnL% is only comparable across runs at the *same* `buy_amount_sol`. | The notional *chain* itself is consistent; this is the residual of a fixed per-leg cost, not a bug |

Ranked full list of sweep↔simulate divergences, including the not-yet-accepted ones:
[../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md).

## `backend/src/sweep/` (generic)

| File | Owns |
| --- | --- |
| `mod.rs` | Module map |
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `SweepMethod` (Grid/Random/LHS/Refine); entry-cache reuse (`entry_key`, `prepare_token`, `resolve_entry`, `resolve_exit`). **Re-exports** `CostModel`/`round_trip_with_costs`/`ExitCode` from `trading_core::strategies::kernel` — the sweep keeps no second copy of the cost/exit math |
| `projection.rs` | `SweepTrade` — slim per-trade row (wallet interned to `u32`); `WalletInterner`; `project_trades` |
| `corpus.rs` | `CorpusSource` trait + the corpus model (`TokenTrades`, `Corpus`, slim `SweepTrade` projection via `from_trades`); `Selection` (cap + time window + curve_only); `sweep_per_mint_cap` — **uncapped by default** (`SWEEP_DEFAULT_PER_MINT_CAP = i64::MAX`): analysis runs over each token's **entire** history, so the grouped sweep matches single-rule simulate for high-volume tokens. `SWEEP_PER_MINT_CAP` (≥1) is an opt-in bound to cut corpus weight (`tokens × trades/token`) for a lighter run. The sole impl is `LakeSource` (see `lake/duck.rs`) — the old PG `DbSource` + Parquet corpus-cache + `attach_fingerprints` were deleted at the lake cutover |
| `engine.rs` | `run_sweep` — rayon over tokens; entry-cache reuse per token; `SweepObserver` cancel; buffer recycling |
| `progress.rs` | `SweepObserver` trait; `SweepProgress` (phase-tagged SSE); `NoopObserver` |
| `aggregate.rs` | `ComboAgg` (a thin wrapper over the core kernel's `RunAgg`) → `ComboMetrics` (= core `RunMetrics` + `combo_id`, via `from_run`). O(1) per combo via the core `QuantileSketch` (~0.6 KB, ~15% rel. error for median/p90) — the sketch/robust-score/exit-index math lives once in `trading_core::strategies::kernel` |
| `retention.rs` | `retained_combo_ids` — keeps per-metric-extreme combos + best_combo (~660 rows/group max); used write-time AND at compaction |
| `grouping.rs` | `TokenFingerprint`, `GroupField`, `GroupKey`; `normalize_label_vec` (shared with corpus filter). All `GroupField`s are `tokens`-creation facts **except** `FirstSlotBuySol`/`FirstSlotSellSol` — the first trade-derived fields, sourced from `tokens_info` (creation-slot buy/sell SOL). Their lake `fp_first_slot_*` cols + `creation_stats_repo::grouped()`'s `LEFT JOIN tokens_info` + `export_tokens`' join all exist to feed them. **Group-key rendering** (`render_field`) is exact-value for discrete fields but **bins** the continuous SOL amounts (`InitialBuySol`, `MaxCostLamports`, `SpendableLamportsIn`, `FirstSlot{Buy,Sell}Sol`) into `SOL_BUCKET_WIDTH` (0.1 SOL)-wide `"lo–hi"` ranges (`bucket_sol_label`); the dashboard SQL (`creation_stats_repo::sol_bucket_sql`) mirrors it byte-for-byte so both surfaces produce identical labels. Making the width runtime-configurable = [dynamic-bucket-size-plan.md](../dynamic-bucket-size-plan.md) |
| `grouped_engine.rs` | `run_grouped_sweep`; two-phase driver (large groups serial, small groups parallel); `make_group_result`; coarse→refine (`run_grouped_with_refine`); partial persistence via `GroupSink` |
| `obs.rs` | Process RSS + host RAM reads; sweep milestone clock |
| `registry.rs` | `tables_for(strategy_id)`, `strategy_ids()`, `run_grouped(...)`; `MAX_COMBOS`; `sweep_base_rule_tpsl{1,2}`; resource fences (`bounded_threads` = cores/2 by default, host-RAM admission) |
| `strategies/tpsl2.rs` | TPSL2 `Strategy`/`ParamSpace` — sweeps all 14 knobs; entry-cache by 8 scalp-gate knobs; `prepare_token` is a no-op (`TokenState = ()`) |
| `strategies/tpsl1.rs` | TPSL1 `Strategy`/`ParamSpace` — sweeps exit ladder only (6 knobs); param-free entry resolves once per token |

## Workstation resource fences (lab-only)

Grouped sweep runs hard **inside** a reserved slice of the analysis box so the desktop stays usable. No mid-run throttle. **No env overrides** — values are auto-detected / hardcoded:

| Policy | Value |
| --- | --- |
| Rayon threads | `max(1, cores − 2)` (e.g. 14 on 16 logical CPUs) |
| RAM reserve | keep host RAM free for OS/UI. Mid-run usable RAM = `max(free − reserve, total − reserve − corpus_baseline)` (`registry::usable_from`): the structural term prices the run's own **transient** buffers as reusable so the sweep doesn't starve on its own RSS, while the corpus (measured once at `corpus_loaded`) stays priced as consumed — bounded so `baseline + usable ≤ total − reserve` (desktop reserve always free). No half-of-free degradation. **Per-run knob** — the sweep form's *RAM reserve* radio (4G/2G/1G/512M/256M) sends `ram_reserve_mb`; default **1 GB**, clamped server-side to 256 MB…32 GB. A *preference*, not a cliff: a tight reserve costs wall-clock, not the run (see below). Held in a process-global (`registry::set_ram_reserve_mb`, safe because the handler single-flights sweeps) so every sizing/shard/fold helper reads it without threading a param through the rayon paths. Run-local: not persisted on the run row (a property of the box at run time, not of the analysis) |
| **Sizing under RAM pressure** | **Degrade, don't refuse.** `registry::plan_sweep_sizing` walks a ladder — threads `N→1`, then fold budget `cap→floor` — and runs at the largest plan that fits. The plan's fold budget is installed as a *ceiling* (`FOLD_BUDGET_CEILING`), so the per-call live sizing still shrinks further if free RAM drops mid-run. Every degradation is reported (`SweepObserver::notice` → `sweep_notice` SSE → info toast) so a slow run is explained, not mysterious. The only refusal left is a **true-floor overflow**: 1 thread + 1 token's series + the minimum fold batch + the minimum shard still don't fit. See [../plans/sweep/ram-sizing.md](../plans/sweep/ram-sizing.md) |
| Series admission | `min(12 GB cap, usable)`; the flat 12 GB is the fallback when host RAM is unreadable (non-Windows/Linux) — that case is now reported as a notice rather than silently disabling the guard |
| Fold batch budget | `usable / 4` clamped to 32..=512 MB; hard max **65 536** combos/batch (8192 when under reserve) |
| Driver (large groups) | **wave-outer** when shard fits (series once/token); else **pass-outer** with disk **spill** of finalized metrics |
| Driver (small groups) | `sweep_group_serial`: **token-outer** (series built once/token, combos folded in batches over it) when the full `n_combos × ComboAgg` set fits across all workers; else **batch-outer** fallback (bounded `batch × ComboAgg`, series rebuilt once/batch). Single-batch groups are token-outer either way. The fit test reads `usable_host_bytes()`, which prices the run's **permanent** resident set (the corpus) as consumed but its own **transient** fold buffers as reusable headroom — without that, the sweep's own RSS drove `usable → 0` mid-run and token-outer never fired (measured 0/405 groups; fixed 2026-07-19, now 152/405 normal & 601/601 under tight reserve, with a 7.9× faster coarse pass on the tight run). Before/after: [../plans/sweep/perf-measurements.md](../plans/sweep/perf-measurements.md) |
| Sharding | large `N` split into RAM-sized combo ranges; up to 4 shards in parallel (RAM-capped); spill+merge |
| Smarter search | full `grid` with ≥200k combos and no refine → auto `lhs:50000` + refine (override with explicit `refine:` / `random:`) |
| Combo materialisation | index-only `GenericCombo { idx }`; `CompiledRule` bound per batch; combo JSON for **retained survivors only** |
| Combo-side sizing | peak priced as **one shard**, not full N; the planner prices it at the *shardable floor* (`MIN_SHARD_COMBOS` = 8192) since `plan_shards` can always cut down to that |
| Failure persistence | a failed run row is **never deleted** — `partial` when groups had already folded (they stay queryable), `failed` when none had. Groups stream to the DB as they fold, so a stop at group 380/400 keeps 380 groups |
| Horizon clamps | sparse-grid ceilings ~7d; gap tick hard-cap; `combo_count` checked mul |

Start log includes cores, threads, wave, planned/shard-peak combos, RSS, host total/available MB.

**Last-resort corpus/combo shrink knobs** (manual UI / rare opt-in; change *what* is computed) — use only when admission still fails after thread reduction:

| Knob | Where | Effect | Fidelity cost |
| --- | --- | --- | --- |
| `ram_reserve_mb` | UI (RAM reserve radio) | Raises the admission ceiling by giving the sweep more of host free RAM | None to the sim — but the box gets less headroom |
| `token_cap` | UI / `Selection` | Fewer tokens | Smaller sample |
| `created_after` / `created_before` | UI / `Selection` | Smaller time slice | Misses other days |
| `min_tokens` | UI | Drops tiny groups | Fewer groups ranked |
| `max_combos` / narrower axes / `random:N` / `refine:N:K` | UI | Fewer combos | Less param coverage |
| `curve_only` | UI | Drops AMM legs | No post-migration path |
| `SWEEP_PER_MINT_CAP` | optional env (not in `.env.example`) | Caps trades/token | Breaks simulate parity |

## Persistence + API

Tables per strategy: `<strategy>_grouped_sweep_{runs,groups,combos,results}` — **lab-only**, defined in `lab/migrations/` and applied by `lab::storage::lab_migrations` (never on EC2/live; see [@arch/database.md](@arch/database.md)). Generic `grouped_sweep_repo.rs` (table-name-driven). Incremental writes: run header up front (`status='running'`), groups appended one at a time by a single DB-writer task fed over an unbounded channel, finalized on completion via `finalize_run`. Crash-recovery: `reconcile_orphaned_runs` at boot. Retention filter applied write-time so only ~660 rows/group are ever inserted.

**Terminal status is honest about partial persistence.** The writer task tracks the persisted-group tally + the first write error; `finalize_run` stamps `completed` only when every folded group actually committed, else `partial` (an engine-complete run whose DB writes fell short) — distinct from `cancelled` (user abort). The reason rides the `SweepFinished` SSE frame's `error` field so the client toasts it.

**Live per-group visibility (no wait for the whole run).** Each committed group emits a `SweepGroupDone` SSE frame `{run_id, group_index, groups_done, group_count}` (plus one announce frame with `group_index: null` when the surviving counts are first known). The frontend (`BackgroundJobsContext`) throttles a per-run `GroupedSweepGroups` cache invalidation off these frames, so the groups table streams in mid-run (largest group first) and a "groups saved N/M" counter renders. Persistence is a concurrent drain, **not** a progress phase — the progress phases are strictly sequential `corpus → coarse → sweep` (`SweepProgress` observer; `corpus` = the DuckDB lake load, indeterminate `total: 0`; `coarse` = refine runs only). The old `saving` phase interleaved with `sweep` frames and made the phase bars flip-flop.

API: `POST /api/strategies/sweeps` (start, detached → 202 with `run_id`), `POST .../cancel`, `DELETE .../sweeps/{run_id}`, `PATCH .../sweeps/{run_id}` (rename), `DELETE .../sweeps?before=` (prune), `GET` for runs/groups/results.

## Parquet lake + DuckDB corpus (Phase 4 — `lab/src/lake/`)

The 3-hop analysis pipeline that feeds the sweep on the workstation:
`EC2-PG → local-PG → Parquet lake → DuckDB`.

| Hop | What | Where |
| --- | --- | --- |
| 1 · sync | Sealed daily Timescale chunks (yesterday-and-older) pulled EC2→local PG over postgres_fdw/SSH; `wallet_dict` id-preserving, no partition loop (Timescale auto-chunks) | `scripts/db-incremental-sync.ps1` |
| 2 · lake export | Each newly-sealed local day → **immutable** `trades/dt=YYYY-MM-DD/data.parquet` (write-once, temp+rename); `tokens/tokens.parquet` dimension (fingerprint cols) rewritten each run. Streamed + row-group-flushed. Units mirror `trade_repo`'s `TradeDbRow` exactly (lamports→SOL ÷1e9; raw token f64; vsol raw→f64; `real_*` dropped). Column **names** are single-sourced in `lake/schema.rs` (writer schema references the consts; a guard test pins the writer's field order to `TRADE_WRITE_COLS`/`TOKEN_WRITE_COLS` and a by-name round-trip catches a same-typed builder swap in `finish()`) | `lab/src/lake/export.rs` (`export_lake`), `lab/src/lake/schema.rs` (column names), `lab/src/lake/mod.rs` (layout) |
| 3 · DuckDB corpus | `LakeSource: CorpusSource` (the **sole** corpus source) reads the lake via an in-memory DuckDB: candidate select + per-mint `ROW_NUMBER` cap over the trades glob (`hive_partitioning=true`), fingerprints from the dimension → `TokenTrades`/`SweepTrade`. Per-mint order is `(slot, tx_index, leg_index, block_time)` — `block_time` is the final tiebreaker because the RPC backfill path leaves `tx_index`=0 (only the live LaserStream feed sets it) and `leg_index`=0 for single-leg txs, so the first three are non-unique; the 4-tuple is unique per mint, giving a deterministic total order. Uses DuckDB's **row API** only (not `query_arrow`) so its bundled arrow never clashes with lab's `arrow 53`. Since `real_*_reserves` were dropped on export, the loader **reconstructs** `real_sol_reserves` per row from the priced reserve pair + `venue` (`approx_real_sol_reserves`: AMM→`reserve_sol`, curve→`reserve_sol−30` clamped ≥0) so the sim's real-reserve gates (tpsl2 `min_liquidity_sol`, dead-token) resolve — an approximation of the live program-emitted value, not lamport-identical | `lab/src/lake/duck.rs` |

CLI: `cargo run -p lab -- lake-export` (batch job; reads `SWEEP_LAKE_DIR`, default OS-temp `pumpfun-lake`). Add `--include-today` to also export today's still-open UTC day (force-overwritten, non-immutable) — the only way to sweep current-day data, since the default export is sealed-days-only. `duckdb = { features=["bundled"] }` is a lab-only dep (lab never ships to EC2).

**Cutover (DONE):** the lake is the **sole** grouped-sweep corpus source. The grouped-sweep handler always calls `LakeSource::new(lake_root()).load(sel)`, and `list_token_results` reloads from the lake when its warm in-memory cache (Option A) misses. The PG `DbSource`, `load_grouped_corpus`/`load_or_build`, the Parquet corpus-cache, and the separate `attach_fingerprints` pass are gone — `LakeSource` embeds fingerprints (`has_fingerprints`). Validated by a byte-identical lake-vs-PG metric diff (the divergence was a non-unique trade-order key; fixed with the `block_time` tiebreaker above). `SWEEP_CORPUS_SOURCE` is retired.

**Single-rule simulate shares the lake too — ONE row type.** The tpsl1/tpsl2/swing1 `.../simulate` backtests read the **same** lake through the **same** `LakeSource::load`/`SweepTrade`; there is no separate `SimTrade`. The only difference is `Selection::with_signatures`: the sweep loads it `false` (rows stay slim — the trigger is resolved by index, not signature), simulate loads it `true` so `SweepTrade::tx_signature` (an `Option<Box<str>>`, `None` on the sweep) is populated for the result tables' Solscan links. Shared entry point `strategies::sim_fetch::fetch_sim_histories` (uncapped per-mint, `curve_only: false`, stale-lake warn). The trades Parquet schema carries `tx_signature` (~88 B/row, only read when `with_signatures`); DuckDB reads use `union_by_name=true` so pre-migration day files null-fill until a full re-export. Because one loader + one type serve both, sim↔sweep pricing is parity by construction; `lake::duck::parity_tests::signature_flag_changes_only_the_signature` (no longer `--ignored` — auto-runs when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise) pins that the flag touches nothing but `tx_signature`, and `duck::tests::reader_columns_are_canonical` ties the reader's column names to `lake/schema.rs`.

**The generic swing analyzer reads the lake too.** `lab/src/api/handlers/tokens/swing.rs`'s `detect_token_swings`/`detect_tokens_swings_batch` (the plain swing-detection endpoints, distinct from the `swing1` strategy) now go through the same `sim_fetch::fetch_sim_history_one`/`fetch_sim_histories` as `swing1-detect` — uncapped, full-history, `curve_only` applied at load (the projected `CorpusTrade` has no `venue`). The batch endpoint resolves its **entire** mint list in one `fetch_sim_histories` call (one DuckDB scan, mints staged into a temp table) instead of the old per-mint PG fan-out. `filter_trades_to_window` in that file is generic over `TradeRow` so it serves both the live `Trade` and lake `CorpusTrade` off the same accessors.

**Full audit (all migrated except one narrow, accepted exception).** Every bulk trade-history read path under `lab/src/` is lake-sourced: grouped sweep, tpsl1/tpsl2/swing1 simulate, swing1-detect, the generic swing analyzer, and the three backtests. The only remaining PG touch on the `trades` table is `grouped_sweep.rs`'s `resolve_fill_signatures` (called from `list_token_results`): a bounded, indexed `(mint, slot, side)` lookup against `TradeRepo` that back-fills `entry_tx`/`exit_tx` Solscan links for a combo's fills, since the sweep loads `Selection::with_signatures = false` (see above) and its slim `CorpusTrade` never carries a signature. This is a deliberate keep — it's a handful of indexed point-lookups, not a bulk scan, and the alternative (threading `tx_signature` through every sweep row) costs ~88 B/row for a field only the drill-in view needs. Everything else PG still serves in `lab` (sweep run/group/combo/result metadata, `strategy_rules`/`strategy_runs`/`strategy_positions`, the `tokens`/`tokens_info` dimension + candidate scan, the token-list boot seed) is dimension/job state, not trade history, and was never a lake-migration candidate.

## Adding a strategy

`strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`) + `registry.rs` arm (table triple + dispatch) + `<x>_grouped_sweep_*` tables in a `lab/migrations/` SQL file + frontend param-key list + axes defs. Engine, grouping, repo, handler, and page are reused unchanged.
