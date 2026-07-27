# Sweep — strategy-agnostic param-sweep engine

File-level map of `backend/src/sweep/`. The generic param-sweep & backtest stack.
Related: [@arch/strategies.md](@arch/strategies.md) (pure fns the sweep reuses), [@arch/database.md](@arch/database.md) (sweep tables), [@arch/frontend.md](@arch/frontend.md) (sweep page).
Deep-dive detail: `@plans/sweep/sweep-engine-detail.md` (engine internals + metrics reference), `@plans/sweep/ram-sizing.md` (RAM ladder + measured perf), `@plans/sweep/sim-parity.md` (sweep↔simulate divergences).

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
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `SweepMethod` (Grid/Random/LHS/Refine); the **two-stage entry** (`entry_key` → `entry_candidates` shared per class → `resolve_entry_from` per combo) plus `prepare_token`, `build_exit_ctx`/`exit_ctx_key`, `resolve_exit`. **Re-exports** `CostModel`/`round_trip_with_costs`/`ExitCode` from `trading_core::strategies::kernel` — the sweep keeps no second copy of the cost/exit math |
| `projection.rs` | `SweepTrade` — slim per-trade row (wallet interned to `u32`); `WalletInterner`; `project_trades` |
| `corpus.rs` | `CorpusSource` trait + the corpus model (`TokenTrades`, `Corpus`, slim `SweepTrade` projection via `from_trades`); `Selection` (cap + time window + curve_only); `sweep_per_mint_cap` — **uncapped by default** (`SWEEP_DEFAULT_PER_MINT_CAP = i64::MAX`): analysis runs over each token's **entire** history, so the grouped sweep matches single-rule simulate for high-volume tokens. `SWEEP_PER_MINT_CAP` (≥1) is an opt-in bound to cut corpus weight (`tokens × trades/token`) for a lighter run. The sole impl is `LakeSource` (see `lake/duck.rs`) — the old PG `DbSource` + Parquet corpus-cache + `attach_fingerprints` were deleted at the lake cutover |
| `engine.rs` | `run_sweep` — rayon over tokens; per-token reuse of the entry-**candidate** walk (never a resolved entry — see below); `SweepObserver` cancel; buffer recycling |
| `progress.rs` | `SweepObserver` trait; `SweepProgress` (phase-tagged SSE); `NoopObserver` |
| `aggregate.rs` | `ComboAgg` (a thin wrapper over the core kernel's `RunAgg`) → `ComboMetrics` (= core `RunMetrics` + `combo_id`, via `from_run`). O(1) per combo via the core `QuantileSketch` (~0.6 KB, ~15% rel. error for median/p90) — the sketch/robust-score/exit-index math lives once in `trading_core::strategies::kernel` |
| `retention.rs` | `retained_combo_ids` — keeps per-metric-extreme combos + best_combo (~660 rows/group max); used write-time AND at compaction |
| `grouping.rs` | `TokenFingerprint`, `GroupField`, `GroupKey`; `normalize_label_vec` (shared with corpus filter). All `GroupField`s are `tokens`-creation facts **except** `FirstSlotBuySol`/`FirstSlotSellSol` — the first trade-derived fields, sourced from `tokens_info` (creation-slot buy/sell SOL). Their lake `fp_first_slot_*` cols + `creation_stats_repo::grouped()`'s `LEFT JOIN tokens_info` + `export_tokens`' join all exist to feed them. **Group-key rendering** (`render_field`) is exact-value for discrete fields but **bins** the continuous SOL amounts (`InitialBuySol`, `MaxCostLamports`, `SpendableLamportsIn`, `FirstSlot{Buy,Sell}Sol`) into `SOL_BUCKET_WIDTH` (0.1 SOL)-wide `"lo–hi"` ranges (`bucket_sol_label`); the dashboard SQL (`creation_stats_repo::sol_bucket_sql`) mirrors it byte-for-byte so both surfaces produce identical labels. Making the width runtime-configurable is a separate, not-yet-merged per-run knob. |
| `grouped_engine.rs` | `run_grouped_sweep`; two-phase driver (large groups serial, small groups parallel); `make_group_result`; coarse→refine (`run_grouped_with_refine`); partial persistence via `GroupSink` |
| `obs.rs` | Process RSS + host RAM reads; sweep milestone clock |
| `registry.rs` | `sweep_tables(strategy_id)` (one arm: `"generic"` → `grouped_sweep_*`), `run_grouped(...)`; `MAX_COMBOS`; resource fences (`bounded_threads` = cores/2 by default, host-RAM admission) |
| `generic/` | `GenericSweepStrategy` — the one sweep family (Phase 7 retired the per-strategy tpsl/swing adapters). `axes.rs` = fingerprint + TP/SL + metric-condition axes → `RuleParams` combos; `strategy.rs` = `Strategy` impl (`TokenState = MetricSeries` precompute; `resolve_entry` mirrors `can_enter` / `resolve_exit` scan the series) + `Pricing` (notional + fill model + cost model) + `ExitClass` bind-time classification and its per-class / vector row finders; `exit_index.rs` = prefix-extrema hulls answering an arbitrary monotone predicate (`first_max_row` / `first_min_row`), plus the `at`-monotonicity flag a `held` binary search needs; `guard.rs` asserts scan ≡ `run_replay` (under every `FillModel`), index/SIMD ≡ scalar, and that a TP/SL rule actually *reaches* the index |

### The entry is exit-dependent — the fold caches candidates, not entries

The engine's `can_enter` gate refuses to buy **while the exit conditions already hold**,
and `resolve_entry` mirrors it. So the resolved entry is a function of the *whole* rule,
not just the entry axes: two combos with the same `entry_key` and different exits can
legitimately enter on different rows. The fold's single-slot cache is keyed on
`entry_key`, so caching the resolved entry there made the first combo of each class
donate its entered set to every sibling — wrong `n_fired`, wrong entry rows and prices on
any grouped sweep with **exit-side metric axes**. Found + fixed 2026-07-26 (mechanism,
proof and blast radius in [../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md)).

The fold now runs the entry in two stages:

* **Stage A** `entry_candidates` — the exit-independent walk (dead check, mono-kills,
  entry-condition eval), opened once per `entry_key` per token and **resumed** as combos
  ask for deeper candidates, so the short-circuit at the first admissible row survives.
* **Stage B** `resolve_entry_from` — per combo: walk the shared candidates applying that
  combo's veto, then price the first admissible row through a per-class fill memo.

Pure TP/SL sweeps (the 1M-combo shape) are untaxed: their exit reqs are position-scoped,
read `NaN` before entry, and so can never veto (`BoundCombo::entry_veto_possible`), making
Stage B a candidate lookup plus a memo hit. `ExitCtx` (the prefix-extrema hulls) is now
rebuilt on `exit_ctx_key` — the resolved `fill_row` — not on entry-key staleness.
Locked by `guard::fold_gives_each_exit_variant_its_own_entry` (fold ≡ per-combo `scan` ≡
`run_replay`, both combo orders) and `engine::tests::fold_reresolves_entry_per_exit_variant_within_one_class`.

**Stored runs predate the fix.** Any grouped run with exit-side metric axes recorded
before 2026-07-26 carries poisoned aggregates for every combo that was not first in its
entry class — re-run them, and expect the crown to move.

### Corpus scope: saved fingerprint vs manual filters

Two mutually-exclusive ways the start request narrows the loaded corpus, both applied in-memory *after* the lake load (so the unfiltered Parquet/`sweep_corpus_cache` entry stays reusable):

- **`fingerprint_id`** (sweep form → *Group by fingerprint* → *Scope by saved fingerprint*, mirroring the Flow-discovery control) — keeps only tokens `hunter_engine::fingerprint::matches` accepts for that saved fingerprint: the **engine match SSOT**, exact axes exact and the continuous SOL axes by **bucket**, i.e. the token set the live entry gate would arm on. `group_by` still partitions *within* the matched slice (empty ⇒ one `ALL` group).
- **`ix_labels_filter` + `field_filters`** — the manual path: exact-set label match and exact-value field pins. These cannot express a bucket axis, which is why scoping is a separate request field rather than a filter prefill. Ignored (and stored as `NULL`) whenever `fingerprint_id` is set.

The scope is persisted on the run row (`grouped_sweep_runs.fingerprint_id`, `lab/migrations/0009_sweep_fingerprint_id.sql`, no FK) because it is not reconstructible from the filter columns: the token-results reload re-applies the same match, re-run restores it in the form, and **promote reuses the scope fingerprint itself** instead of synthesizing one from the group key — an `ALL` group key would otherwise yield an axis-less fingerprint that matches every token.

### Metric scope: token-scoped columns vs position-scoped axes

Almost every metric is **token-scoped** — one value per token per event, so the
precompute records it as a `SeriesColumn` and the per-combo scan reads it off the
flat buffer. `m_position` (`retrace` / `pnl` / `held`) is **position-scoped**: its
value anchors on *your* entry fill, so it cannot be precomputed token-independently
(a static column would only ever record `NaN` — the track holds no position state).

Consequences, all enforced in code:

* `axes.rs` rejects an `m_position` axis on the **entry** side (it reads `NaN`
  before entry, so the condition could never fire) and contributes **no column**
  for it on the exit side.
* `strategy.rs::resolve_exit` carries a running since-entry peak/trough (seeded to
  the fill price, folded forward per row *before* that row's decision — mirroring
  `reduce.rs::evaluate_token`) and evaluates position-scoped exit reqs through
  `position_value(..)` against that `PositionCtx`, exactly as
  `CompiledRule::exit_fired` does. Token-scoped exit reqs still read their column.
* The desugared TP/SL `pnl` reqs (`ReqOrigin::{TakeProfit,StopLoss}`) go through that
  **same** walk. The scan no longer re-derives them as an `entry_price · (1 ∓ pct/100)`
  price branch — that was a second representation of a fact the engine already
  desugars, and it compared in *price* space where the fold compares in *pnl* space.
  The exit label comes from the fired req's `ReqOrigin`, and priority
  (`Dead > SL > TP > authored`) is carried by `exit_reqs` **order**: `compile`
  prepends SL then TP. `CompiledRule::{take_profit,stop_loss}` survive as the
  authoring / DB / FE surface; only the sweep's *evaluation* of them is gone.
* `m_price_window` **is** token-scoped, so `trail`/`rise` precompute as an ordinary
  `SeriesColumn::Window` — routed to the price-extrema deque (`ensure_price_window`),
  not the flow ring buffer. Its window counts toward `SparseGrid::max_window_secs`:
  a rolling high decays as prints age out, so the decay-region ticks must be emitted
  exactly like a flow window's.

Not wired: **re-entry** (`RuleParams.reentry`). The sweep's `TokenOutcome` is one
episode per (token, combo); multi-episode accumulation would change the outcome
model, aggregation and persistence. Re-entry validates via simulate/replay instead.

### Flow axes (`m_flow_split` / `m_flow_split_window`)

When axes reference a flow group, the corpus loads with `Selection.with_flow`
(trade `ix_labels` + `wallet`). The start body carries optional
`volume_ix_patterns: string[][]` — applied **corpus-wide** for that run (not per
fingerprint). Missing patterns with flow axes ⇒ `400`. **Promote** copies the
run's patterns into the created fingerprint's
`metric_config.m_flow_split.volume_ix_patterns` (`find_or_create` ignores
`metric_config` for identity, then patches). Discovery (lab
`/strategies/flow-discovery`) is a separate job that scores structures and writes
the same key — mutual `409` with sweeps. See
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md).

## Workstation resource fences (lab-only)

Grouped sweep runs hard **inside** a reserved slice of the analysis box so the desktop stays usable. No mid-run throttle. **No env overrides** — values are auto-detected / hardcoded:

| Policy | Value |
| --- | --- |
| Rayon threads | `max(1, cores − 2)` (e.g. 14 on 16 logical CPUs) |
| RAM reserve | keep host RAM free for OS/UI. Mid-run usable RAM = `max(free − reserve, total − reserve − corpus_baseline)` (`registry::usable_from`): the structural term prices the run's own **transient** buffers as reusable so the sweep doesn't starve on its own RSS, while the corpus (measured once at `corpus_loaded`) stays priced as consumed — bounded so `baseline + usable ≤ total − reserve` (desktop reserve always free). No half-of-free degradation. **Per-run knob** — the sweep form's *RAM reserve* radio (4G/2G/1G/512M/256M) sends `ram_reserve_mb`; default **1 GB** (`DEFAULT_SWEEP_RAM_RESERVE_MB`), clamped server-side to 256 MB…32 GB. A *preference*, not a cliff: a tight reserve costs wall-clock, not the run (see below). Held in a process-global (`registry::set_ram_reserve_mb`, safe because the handler single-flights sweeps) so every sizing/shard/fold helper reads it without threading a param through the rayon paths. Run-local: not persisted on the run row (a property of the box at run time, not of the analysis) |
| **Sizing under RAM pressure** | **Degrade, don't refuse.** `registry::plan_sweep_sizing` walks a ladder — threads `N→1`, then fold budget `cap→floor` — and runs at the largest plan that fits. The plan's fold budget is installed as a *ceiling* (`FOLD_BUDGET_CEILING`), so the per-call live sizing still shrinks further if free RAM drops mid-run. Every degradation is reported (`SweepObserver::notice` → `sweep_notice` SSE → info toast) so a slow run is explained, not mysterious. The only refusal left is a **true-floor overflow**: 1 thread + 1 token's series + the minimum fold batch + the minimum shard still don't fit. See [../plans/sweep/ram-sizing.md](../plans/sweep/ram-sizing.md) |
| Series admission | `min(12 GB cap, usable)`; the flat 12 GB is the fallback when host RAM is unreadable (non-Windows/Linux) — that case is now reported as a notice rather than silently disabling the guard |
| Fold batch budget | `usable / 4` clamped to 32..=512 MB; hard max **65 536** combos/batch (8192 when under reserve) |
| Driver (large groups) | **wave-outer** when shard fits (series once/token); else **pass-outer** with disk **spill** of finalized metrics |
| Driver (small groups) | `sweep_group_serial`: **token-outer** (series built once/token, combos folded in batches over it) when the full `n_combos × ComboAgg` set fits across all workers; else **batch-outer** fallback (bounded `batch × ComboAgg`, series rebuilt once/batch). Single-batch groups are token-outer either way. The fit test reads `usable_host_bytes()`, which prices the run's **permanent** resident set (the corpus) as consumed but its own **transient** fold buffers as reusable headroom — without that, the sweep's own RSS drove `usable → 0` mid-run and token-outer never fired (measured 0/405 groups; fixed 2026-07-19, now 152/405 normal & 601/601 under tight reserve, with a 7.9× faster coarse pass on the tight run). Before/after: [../plans/sweep/ram-sizing.md](../plans/sweep/ram-sizing.md#measured-performance-2026-07-19) |
| Exit-scan path | **Bind-time req classification, then a per-class search.** `BoundCombo::new` classifies every exit req once per combo (`ExitClass`): a monotone `m_position.pnl` bound → prefix-extrema hull, **O(log n)**; a `>=` bound on `m_position.held` → binary search on `series.at`, **O(log n)**; `m_position.retrace` → running-peak scan, **O(n)** (vectorized — *not* O(log n); a running peak is not a static prefix query); `m_position.bounce` → running-trough scan, **O(n)**; anything else (token-scoped column, multi-arm DNF, `=`/`!=`) → `General`. One `General` req drops the whole rule to the scalar walk. `bound.fast_exit` = no `General`, and it — not `has_exit_metrics()` — is what gates building the index (`wants_exit_index`). The earliest row across the classified reqs wins, ties broken by `exit_reqs` order (so desugared SL > TP > authored); `Dead` outranks all. Optional **AVX-512** toggle (`resolve_exit_simd`, 8×`f64`) for A/B on the pure-`pnl`-bound shape, comparing **in pnl space** per lane (IEEE ops are exactly rounded ⇒ bit-identical to scalar, so no threshold inversion is needed); other shapes delegate to the index path. **Byte-identical** to scalar (guards `index_exit_scan_matches_scalar_*` + `simd_exit_scan_matches_scalar_across_paths`), plus a **reachability** guard (`tp_sl_rules_actually_reach_the_exit_index`) — the Phase-2 desugaring made `has_exit_metrics()` true for every TP/SL rule, which silently disabled both fast paths for a whole phase without breaking a single equality test. Money math stays the one `kernel` copy. Lab-only. AVX-512 measured 2026-07-19: scalar linear **0.63 s** → SIMD **0.29 s** (2.2×, release); SIMD is ~2.3× *slower* in debug — leave the toggle off under plain `cargo run`. |
| Pricing (fill + cost model) | **Part of a run's identity, chosen per run.** `Pricing { buy_amount_sol, fill_model, cost }` threads from the request through `run_grouped` → `GenericSweepStrategy` → every scan fn. `fill_model` picks which trade in the window prices each leg (the same `FillModel` `ReplayConfig` threads — fill *eligibility* is identical across models, so the taken set never moves, only the price); `cost_model` picks `pumpfun_default` vs `pumpfun_fee_only`. Both persist on the run row (migration `0010`) and the drill-in re-simulates under the run's own pair, for the same reason it re-uses the run's `as_of` (parity plan B7). `NULL` ⇒ the legacy pair (`worst_case` + `pumpfun_default`). **Fixed per-leg tip/priority** inside either cost model comes from process-wide `FeeTuning` (`JITO_MIN_TIP_SOL` + `CU_PRICE_MICRO_LAMPORTS` — same knobs live applies to the trader; lab installs at boot). **Pair fill + cost coherently:** an explicit fill model already prices execution slippage, so `pumpfun_default` charges it twice — and since `fixed_cost_sol_per_leg` is per-leg, that haircut scales with how often a combo fires, i.e. it is *not* rank-preserving across combos. The FE warns when a run carries that pair. Guard: `assert_parity` runs scan ≡ `run_replay` under **every** `FillModel`. |
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
| `token_cap` | UI / `Selection` | Newest-N corpus trim (`ORDER BY created_at DESC LIMIT`); clamped to `MAX_TOKEN_CAP` = 100k, persisted post-clamp | Smaller / newer-biased sample (simulate has no cap) |
| `created_after` / `created_before` | UI / `Selection` | Smaller time slice | Misses other days |
| `min_tokens` | UI | Drops tiny groups | Fewer groups ranked |
| `max_combos` / narrower axes / `random:N` / `refine:N:K` | UI | Fewer combos | Less param coverage |
| `curve_only` | UI | Drops AMM legs | No post-migration path |
| `SWEEP_PER_MINT_CAP` | optional env (not in `.env.example`) | Caps trades/token | Breaks simulate parity |

## Persistence + API

Tables per strategy: `<strategy>_grouped_sweep_{runs,groups,combos,results}` — **lab-only**, defined in `lab/migrations/` and applied by `lab::storage::lab_migrations` (never on EC2/live; see [@arch/database.md](@arch/database.md)). Generic `grouped_sweep_repo.rs` (table-name-driven). Incremental writes: run header up front (`status='running'`), groups appended one at a time by a single DB-writer task fed over an unbounded channel, finalized on completion via `finalize_run`. Crash-recovery: `reconcile_orphaned_runs` at boot. Retention filter applied write-time so only ~660 rows/group are ever inserted.

**Corpus freshness is stamped on the run.** `grouped_sweep_runs.corpus_last_trade_at`
(lab migration `0011`) is the corpus-wide `max(block_time)` captured at corpus load —
the same instant the frozen-tail resolve anchors on (one definition:
`Corpus::last_trade_at`). The sweep is LakeSource-only while `simulate` splices the
fresh PG tail, so a stale export is otherwise invisible: the sweep freezes positions as
`Open (est)` at old prices that a simulate of the same rule watches die. The run panel
renders it as a **Data through** row next to Pricing, warning when the lake was ≥1 h
behind the run's start. `NULL` on rows written before the column existed ⇒ "unknown".

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

CLI: `cargo run -p lab -- lake-export` (batch job; reads `SWEEP_LAKE_DIR`, default OS-temp `pumpfun-lake`). Add `--include-today` to also export today's still-open UTC day (force-overwritten, non-immutable) — the only way to sweep current-day data, since the default export is sealed-days-only. Each sealed day also writes `_meta.json` (`row_count`); a later export re-seals if the sidecar is missing or mismatches PG `COUNT(*)`. One-shot current-day refresh: `./scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake`. `duckdb = { features=["bundled"] }` is a lab-only dep (lab never ships to EC2).

**Cutover (DONE):** the lake is the **sole** grouped-sweep corpus source. The grouped-sweep handler always calls `LakeSource::new(lake_root()).load(sel)`, and `list_token_results` reloads from the lake when its warm in-memory cache (Option A) misses. The PG `DbSource`, `load_grouped_corpus`/`load_or_build`, the Parquet corpus-cache, and the separate `attach_fingerprints` pass are gone — `LakeSource` embeds fingerprints (`has_fingerprints`). Validated by a byte-identical lake-vs-PG metric diff (the divergence was a non-unique trade-order key; fixed with the `block_time` tiebreaker above). `SWEEP_CORPUS_SOURCE` is retired.

**Single-rule simulate shares the lake too — ONE row type.** The tpsl1/tpsl2/swing1 `.../simulate` backtests read the **same** lake through the **same** `LakeSource::load`/`SweepTrade`; there is no separate `SimTrade`. The only difference is `Selection::with_signatures`: the sweep loads it `false` (rows stay slim — the trigger is resolved by index, not signature), simulate loads it `true` so `SweepTrade::tx_signature` (an `Option<Box<str>>`, `None` on the sweep) is populated for the result tables' Solscan links. Shared entry point `strategies::sim_fetch::fetch_sim_histories` (uncapped per-mint, `curve_only: false`, stale-lake warn). The trades Parquet schema carries `tx_signature` (~88 B/row, only read when `with_signatures`); DuckDB reads use `union_by_name=true` so pre-migration day files null-fill until a full re-export. Because one loader + one type serve both, sim↔sweep pricing is parity by construction; `lake::duck::parity_tests::signature_flag_changes_only_the_signature` (no longer `--ignored` — auto-runs when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise) pins that the flag touches nothing but `tx_signature`, and `duck::tests::reader_columns_are_canonical` ties the reader's column names to `lake/schema.rs`.

**The generic swing analyzer reads the lake too.** `lab/src/api/handlers/tokens/swing.rs`'s `detect_token_swings`/`detect_tokens_swings_batch` (the plain swing-detection endpoints, distinct from the `swing1` strategy) now go through the same `sim_fetch::fetch_sim_history_one`/`fetch_sim_histories` as `swing1-detect` — uncapped, full-history, `curve_only` applied at load (the projected `CorpusTrade` has no `venue`). The batch endpoint resolves its **entire** mint list in one `fetch_sim_histories` call (one DuckDB scan, mints staged into a temp table) instead of the old per-mint PG fan-out. `filter_trades_to_window` in that file is generic over `TradeRow` so it serves both the live `Trade` and lake `CorpusTrade` off the same accessors.

**Full audit (all migrated except one narrow, accepted exception).** Every bulk trade-history read path under `lab/src/` is lake-sourced: grouped sweep, tpsl1/tpsl2/swing1 simulate, swing1-detect, the generic swing analyzer, and the three backtests. The only remaining PG touch on the `trades` table is `grouped_sweep.rs`'s `resolve_fill_signatures` (called from `list_token_results`): a bounded, indexed `(mint, slot, side)` lookup against `TradeRepo` that back-fills `entry_tx`/`exit_tx` Solscan links for a combo's fills, since the sweep loads `Selection::with_signatures = false` (see above) and its slim `CorpusTrade` never carries a signature. This is a deliberate keep — it's a handful of indexed point-lookups, not a bulk scan, and the alternative (threading `tx_signature` through every sweep row) costs ~88 B/row for a field only the drill-in view needs. Everything else PG still serves in `lab` (sweep run/group/combo/result metadata, `strategy_rules`/`strategy_runs`/`strategy_positions`, the `tokens`/`tokens_info` dimension + candidate scan, the token-list boot seed) is dimension/job state, not trade history, and was never a lake-migration candidate.

## Metric-combo discovery pipeline (`lab/src/discovery/`)

Lab-only, built entirely on top of the generic sweep engine above (no new engine) —
an automated screen → family-grid → out-of-sample-validate pipeline that finds which
metric/param combos actually make money for a cohort, ranked by a stability-weighted
objective, then hands a one-click promote into the shared rule editor. Nothing ships to
EC2; live/paper are untouched (an analysis aid that *outputs* combos, like the sweep).
Registry-driven throughout: a metric added to `REGISTRY` needs no pipeline edit (family
tag + unit/scope/monotonic flags are all it reads).

| File | Role |
| --- | --- |
| `discovery/objective.rs` | `DiscoveryWeights` (tunable constants below) + `discovery_score(ComboStats) → Ranked \| BelowMinClosed \| NoFire` — a pure re-rank over persisted `ComboMetrics`, not a `checklist_score`/kernel edit (that stays the live/paper/sweep SSOT) |
| `discovery/candidates.rs` | `screen_plan` (registry → screenable metrics + `SkipReason`) → `collect_percentiles` (measured `[p05..p99]` per metric, via the engine's own `MetricSeries` — deliberately **not** DuckDB SQL, else percentile semantics could drift from `hunter_engine`) → `build_menus` (`p10/p25/p50/p75/p90` + `off`, rounded by unit) → feeds `AxesModel` directly; the hand-derived table in [axis-value-candidates.md](../plans/sweep/axis-value-candidates.md) is now generated, not authored |
| `discovery/screen.rs` | Layer 1: `ScreenStrategy`, an additive scan mode (`GenericSweepStrategy::share_precompute`) that sweeps every candidate metric alone against a fixed TP/SL baseline over **one** shared per-token precompute (~6N combos, not 6^N) → `Verdict{Keep\|DropNoEdge\|DropSpike\|DropThin\|DropNoBaseline}` per metric → ranked shortlist |
| `discovery/family.rs` | Layer 2: `plan_families` groups the Layer-1 shortlist by the registry's `MetricFamily` tag (`price`/`flow`/`flow_split`/`liquidity-age`, mirrors the hue-wheel families), grids within each family, then runs an O(families²) pairwise interaction check (pin A's best, sweep B) → `Independent \| Interacting \| Inconclusive` per ordered pair, plus each family's `BestCombo` (canonical `RuleParams` JSON, ready to promote) |
| `discovery/validate.rs` | Layer 3: `split_tokens` (age-based train/validate split) + `validate_candidates` re-scores each Layer-2 winner on the held-out slice via `simulate_one_combo` under the run's own `Pricing`/`as_of` → `ValidationVerdict{Holds\|Degraded\|Failed\|ThinValidate\|NoFireValidate\|UnrankableTrain}` (the two "can't tell" outcomes are never silently a pass) |
| `discovery/pipeline.rs` | `run_pipeline` — splits the cohort first, fits Layers 1–2 on train, validates on the held-out slice (a degenerate split fits the whole cohort and reports `no_validation` rather than a vacuous pass) |
| `discovery/dto.rs` + `api/handlers/strategies/metric_discovery.rs` | `PipelineDto` (report flattened to stable wire vocabulary) + `POST /api/strategies/metric-discovery` (+`/cancel`/`/last`/`/{run_id}`, SSE progress, single-flight mutually exclusive with sweep/flow-discovery, cohort scoping by fingerprint/`ix_labels`/field filters) |
| `frontend/src/lab/pages/strategies/MetricDiscoveryPage.tsx` | shortlist → family winners + interaction map → validation verdicts, **Promote…** on any winner (builds a `PromotedRuleDraft` client-side from `params_json` — no dedicated promote endpoint) |

**Objective (Layer 1's ranking core):** `robust_profit × fire_rate × win_component ×
min_n_gate`, where `robust_profit` is median-anchored (not mean — one whale winner can't
carry a combo) with an open-position mark discounted by `OPEN_HAIRCUT`, `win_component`
blends `win_rate` with a capped `profit_factor`, and `min_n_gate` hard-zeroes any combo
with `n_closed < MIN_CLOSED` (the anti-overfit backbone — no profit% lets a 4-trade
"edge" rank). **Open (unpinned) constant:** `OPEN_HAIRCUT` / `profit_factor` cap /
`MIN_CLOSED` / plateau-penalty weight are seeded from the `axis-value-candidates.md`
anchors but never validated-and-pinned as a permanent tuning — revisit once a discovery
run's picks are checked against live/paper outcomes.

**Perf shape is scan/precompute-bound, not fold-bound** (few combos; cost is the corpus
load + per-token `MetricSeries` build + the exit scan). A discovery run should therefore
pick a **tighter RAM reserve** (bigger resident series wave, fewer precompute rebuilds)
and **AVX-512 on in release builds** (2.2× on the pnl-bound exit scan every TP/SL
baseline carries) rather than inherit the interactive sweep's defaults — see
`discovery/screen.rs`'s knob table. The dominant lever regardless is precompute reuse:
one corpus load + one series-union precompute shared across every metric screen, not
N re-loads.

**Data reality:** the fingerprint dimension (`tokens`/`tokens_info`) covers only ~7% of
the tradable universe (a backfill gap, not a design choice) — this throttles Layer-2/3
*grouping/scoping* only; Layer-1 metric-axis screening runs over the full trade corpus
unaffected. Default to a tight single-regime cohort (one fingerprint scope or
`ix_labels`-only) — widen only when a regime can't clear `MIN_CLOSED`.

## Adding a strategy

`strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`) + `registry.rs` arm (table triple + dispatch) + `<x>_grouped_sweep_*` tables in a `lab/migrations/` SQL file + frontend param-key list + axes defs. Engine, grouping, repo, handler, and page are reused unchanged.
