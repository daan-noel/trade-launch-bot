# Sweep — strategy-agnostic param-sweep engine + grouped sweeps

File-level map of `backend/src/sweep/` — the **generic** param-sweep & backtest
stack. Reference this instead of re-reading source. Related:
[strategies.md](strategies.md) (the tpsl1/tpsl2 pure fns the sweep reuses),
[database.md](database.md) (sweep tables), [frontend.md](frontend.md) (the sweep
page). The `Strategy` impls are `sweep/strategies/tpsl2.rs` and
`sweep/strategies/tpsl1.rs` (below) — both wired through the same generic stack.

## Core idea

A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads
the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in the
loop), then **folds** the per-(combo, token) outcomes into one ranked metrics row
per combo. The engine/aggregate/grouping layers only ever see `TokenOutcome`, so
they are strategy-blind: a new strategy adds only a `Strategy`/`ParamSpace` impl.

Two sweep shapes share this stack:

- **Flat sweep** (`run_sweep`) — one global ranked table over the whole corpus.
  No longer exposed standalone; it is the per-group primitive the grouped sweep
  calls.
- **Grouped sweep** (`run_grouped_sweep`) — the primary/only entry point. Partition
  the corpus by an exact-value **fingerprint key**, run the flat sweep **per group**,
  surface each group's best combo. Answers "for tokens with *this* fingerprint,
  which param combo is best?".

```
corpus (DB-range, once) ─► attach_fingerprints ─► partition by GroupKey
  ─► (drop groups < min_tokens) ─► run_sweep per group ─► best combo by robust score (coverage-floored)
  ─► retention filter (keep per-metric extremes + best_combo) ─► <strategy>_grouped_sweep_{runs,groups,results} (DB)
  ─► /api/strategies/sweeps[...] ─► Grouped Sweep React page
```

- **Decision parity:** a strategy's `simulate` calls the *same* pure fns the live
  path uses, so backtest and live resolve identical entry/exit decisions.
- **PnL:** cost-aware (`round_trip_with_costs`). A `CostModel` (per-leg trading
  fee + symmetric execution slippage + fixed Jito-tip/priority-fee), resolved once
  per run from the live `pump_trader` constants (`CostModel::pumpfun_default`) and
  carried on the strategy, is charged on **both** legs so entry/exit stay symmetric
  and the table reflects what the live trader pays. The frictionless `round_trip`
  is kept only as the unit-test/analytic baseline.
- **Ranking (grouped):** a group's best combo = max robust **realized** `score`
  (`μ−1.64·σ/√n` over **closed** trades — variance-penalised, open positions
  excluded) among combos clearing a **coverage floor** (`max(min_fired_abs,
  ceil(fire_frac · group_tokens))`, default `10` / `5%`, per-run configurable via
  the request body). Ties break by `n_fired` then total PnL; if no combo clears
  the floor, fall back to the most-fired combo (logged, low-confidence). This is
  the **same metric the drill-in table sorts by**, so the crowned combo *is* row
  1 of its own table — and `best_expectancy_sol` is kept only as a secondary
  readout. The floor is the over-fit guard (a combo that won big on 2 of 200
  tokens no longer out-ranks one proven over 150).

## `backend/src/sweep/` (generic)

| File | Owns |
| --- | --- |
| `mod.rs` | Module map + parity note. |
| `strategy.rs` | `Strategy` + `ParamSpace` traits. **Coarse→refine:** `ParamSpace::refine(survivors) -> Vec<Params>` (default empty) returns a **coordinate-move neighborhood** — each survivor with one axis moved to its adjacent candidate (`neighbor_indices`), holding the others fixed (≤ 2× multi-valued-axes neighbors/survivor, linear in axes); both tpsl impls build it via `index_of` (generic lookup so float axes dodge `clippy::float_cmp`). `RefineSpec { top_k }` + `parse_method(s) -> (SweepMethod, Option<RefineSpec>)` parse `refine:N[:K]` into a coarse LHS:N pass + a per-group top-`K` refine (`K` default 3). Plus the original `Strategy` `Strategy` is **factored into entry then exit** (not one `simulate`): assoc types `Entry`/`EntryKey: PartialEq`/`TokenState`, and `entry_key(&Params) -> EntryKey` / `prepare_token(&[SweepTrade]) -> TokenState` / `resolve_entry(&[SweepTrade], &TokenState, &Params) -> Entry` / `resolve_exit(&[SweepTrade], &Entry, &Params) -> TokenOutcome`. The engine resolves the (expensive) entry **once per distinct `entry_key` per token** and reuses it across that key's exit sub-grid (sound because the resolved entry depends only on the entry params, never the exit ladder). A param-free/inseparable entry sets `EntryKey = ()` so it resolves once per token. `order_for_entry_cache(&mut [Params])` (default no-op) lets a strategy reorder the final combo set so same-`entry_key` combos are contiguous — the grouped driver calls it once on the shared combo vec before the per-group sweeps, restoring the single-slot entry cache's hit rate under random/lhs/refine orderings (a full Grid is already entry-contiguous); tpsl2 stable-sorts by its 8 scalp knobs. `prepare_token` computes param-independent per-token state **once** before the combo loop (tpsl2's launch cohort; `TokenState = ()` when there's none) and threads it into every `resolve_entry`. All take the slim `&[SweepTrade]` projection (not `Trade`). `TokenOutcome` (`Copy`, no String), `ExitCode`, `SweepMethod` (Grid / `Random{n,seed}` / `LatinHypercube{n,seed}`), the shared `lhs_index_plan(rng,n,axis_lens) -> Vec<Vec<usize>>` (a **real** discrete LHS: per-axis balanced strata `⌊n/len⌋–⌈n/len⌉`, each column independently shuffled so axes decorrelate — what `Random`'s uniform draw only balances in expectation; both strategies' `sample` use it for the LHS arm), and the cost layer: frictionless `round_trip` (test baseline) + `CostModel`/`CostModel::pumpfun_default` + `round_trip_with_costs` (the single price gate every `resolve_exit` uses). |
| `projection.rs` | `SweepTrade` — the slim per-trade row the hot loop walks (only the `TradeRow` fields; wallet interned to a token-local `u32`; **no** `tx_signature` — Phase 1.2 dropped it, ~halving the row, since the only sweep consumer (the worst-case-entry trigger match) is now index-based; `TradeRow::tx_signature` returns `""` for the sweep row). ~3× smaller than `Trade`, so cohort membership is `u32`-keyed not String-keyed. `project_trades<T: TradeRow<Wallet = String>>(&[T]) -> (Vec<SweepTrade>, Vec<Box<str>>)` interns wallets per token via `WalletInterner` — it projects any `String`-walleted source (the DB-loaded full `Trade`). `WalletInterner` lives here but is also reused resident on each live `TokenState` (which interns its `CachedTrade`s to `u32` itself — Phase B step 2 — hence `WalletInterner: Clone`). The shared entry/exit/cohort fns are generic over `TradeRow` (defined in `models/trade.rs`), so they run over **either** the live `Trade` or `SweepTrade` with one implementation — decision parity preserved, live path unchanged. The tpsl2 entry uses `find_scalp_entry_with_cohort_indexed` + `find_worst_case_paper_entry_at` (both in `entry/scalp.rs`) so the trigger never round-trips through a signature string; the live/backtest paths keep the signature-keyed wrappers (`find_scalp_entry_with_cohort` / `find_worst_case_paper_entry`), unchanged. |
| `corpus.rs` | `CorpusSource` trait + `DbSource` (chunked, per-mint-capped batch query) — the sole impl. Each token is **projected once at load** into a slim, wallet-interned `SweepTrade` buffer (see `projection.rs`) — `Trade` never enters the sweep loop. `TokenTrades { trades: Arc<Vec<SweepTrade>>, wallets: Arc<Vec<Box<str>>> (u32→address, only for Parquet write), fp: TokenFingerprint }` (`fp` = grouping only — `simulate` never reads it). One projection entry point: `TokenTrades::from_trades<T: TradeRow<Wallet = String>>` interns a `String`-walleted source (DB `Trade` + tests). `curve_only` filters before projection drops venue — on the DB row's `venue` String. `Selection` = cap + `created_after`/`created_before` window + `TradeWindow` (`LaunchWindow` default = earliest-N per mint so the launch prefix the entry logic needs survives the per-mint cap, vs `Recent` = newest-N for live parity) + curve_only. Compact **slim** Parquet corpus cache (`SweepTrade` columns + wallet/tx strings re-interned on read; fingerprint-free; drops `vtok`/`rtok`/`venue` no sweep fn reads). `attach_fingerprints(pool, &mut Corpus)` — a cheap separate chunked `tokens` lookup (incl. JSONB `max_sol_cost`/`spendable_sol_in` → bigint) that fills each token's `fp`. **Streaming read (Phase 2a primitive):** `stream_corpus_parquet(path, emit)` walks the mint-contiguous file and invokes `emit` with each `TokenTrades` the instant its mint run ends — only one token's buffer resident at a time (the enabler for a future streamed grouped fold). `read_corpus_parquet` is the thin collect-into-`Vec` wrapper, so the round-trip test covers both. **Corpus reuse across runs:** `load_grouped_corpus(db, sel, cache_dir, fresh)` serves a **closed/settled** window (`selection_is_cacheable`: `created_before` ≥ `CORPUS_CACHE_SETTLE_HOURS`=2h in the past) from a selection-keyed Parquet cache (`selection_cache_key` over window+caps+curve_only+mints) via `load_or_build`, skipping the DB load on a hit; an open/recent window always loads fresh, and `fresh=true` forces a rebuild + cache rewrite. `corpus_cache_dir()` = `$SWEEP_CORPUS_CACHE_DIR` else a temp subdir. Fingerprints are attached after (cache is fingerprint-free). |
| `engine.rs` | `run_sweep` — `rayon` over tokens (combos inner, slice stays cache-hot); single fold thread → one `ComboAgg` per combo. Returns `SweepStats` + `Vec<ComboMetrics>`. **Reused verbatim per group.** Per token it computes `prepare_token` once (shared per-token state, e.g. the launch cohort), keeps the last resolved `(entry_key, entry)` and calls `resolve_entry` only when the key changes, then `resolve_exit` per combo — so on a grid (exit axes = low-order digits ⇒ contiguous same-entry blocks) the expensive entry resolves **once per entry-tuple, not per combo** (E×/token, not E·X×). `random:N` still resolves correctly; only the reuse rate drops. Takes a `&dyn SweepObserver`: the fold thread calls `token_done()` per folded token (uncontended progress), and producers run via `try_for_each_with` so a cancel makes rayon stop scheduling tokens at once. The cancel flag is also polled **inside** each token's combo fold every `CANCEL_CHECK_STRIDE` (256) combos, so a large combo set (up to `HARD_MAX_COMBOS`) bails mid-token instead of after — worst-case stop latency is one chunk × ≤pool-size in-flight tokens (sub-100ms), not one full token. A token that bails mid-fold is never sent to the folder; the caller discards the partial result. The folder **recycles** each drained `Vec<TokenOutcome>` back to the producers over a return channel, so the parallel path refills one buffer per token instead of allocating a fresh `vec![; combos]` every token (#4); the serial small-group fold already reuses one buffer across its tokens. |
| `progress.rs` | `SweepObserver` trait (`set_total`, `token_done`, `cancelled`) — keeps the engine transport-agnostic. `SweepProgress` impl broadcasts throttled `SseEvent::SweepProgress { strategy_id, phase, processed, total }` (~100 frames/run + final) tagged with `phase` (`"coarse"` / `"sweep"` / `"saving"`); `SweepProgress::new` takes a `phase` arg so the handler can construct separate observers per phase. Only the `"sweep"` observer writes `AppState.sweep_progress` (ProgressCell); the `"coarse"` observer gets a throwaway cell so status recovery shows the sweep phase. `NoopObserver` for tests. |
| `aggregate.rs` | `ComboAgg` (streaming accumulator) → `ComboMetrics` (one ranked row): win rate, total/expectancy/median/mean/p90/best/worst PnL, `std_pnl_pct`, profit factor, robust `score` (`μ−Z·σ/√n` on closed trades), holding stats, exit-reason mix. **O(1) per combo:** median/p90 (pnl% + holding) come from a fixed-size `QuantileSketch` (DDSketch-style signed log buckets, `SKETCH_N`=64/sign, `u16` saturating counts, ~0.6 KB/combo, ~15% relative error), not a per-token `Vec` — so a combo's memory no longer grows with the tokens it fires on (the old `Vec<f32>`/`Vec<i32>` reached multi-GB at `HARD_MAX_COMBOS` × big groups). Counts are integers ⇒ the estimate is **fold-order-independent** (parallel/grouped folds reproduce it). Best/worst (running min/max) and all means (running sums) stay **exact**; only the two interior quantiles approximate. |
| `retention.rs` | **Storage-retention filter** (the shared core, used write-time **and** at compaction so existing/future data are selected identically). `retained_combo_ids(metrics, best_combo_id, &RetentionCfg) -> HashSet<u32>`: per group, keep the top/bottom-`N` **distinct** values of each of the 11 ranking metrics (the `pnl` column group), capped at `cap_per_value` combos per tied value, plus the `best_combo_id`. Two guards make it bounded (proven necessary by the data — `win_rate`'s worst value alone matched 110k combos): a combo only contributes to a metric's value set if `n_closed ≥ min_closed` (=2, the `score` validity rule, drops the under-sampled junk) and the value is finite/`Some`; and the per-value cap stops a flat distribution from blowing up. Tie-break inside the cap = `score` desc then `combo_id` asc (deterministic). `RetentionCfg { top_n:3, bottom_n:3, cap_per_value:10, min_closed:2 }` (`Default` = the locked prod config). Worst case `11×(top+bottom)×cap` = 660 rows/group (vs 186k–331k), the cross-metric union dedups further. Pure + order-independent; the cap sort carries each combo's score so it stays O(n·log n) (no per-comparison lookup). |
| `grouping.rs` | Strategy-blind grouping. `TokenFingerprint` (creator, token program, CU limit/price, cashback, max_sol_cost, spendable_sol_in, initial_buy_sol, ix_labels), `from_token`, `extract_lamports`, `normalize_labels` / `normalize_label_vec` (sort+dedup a `Value` array / a `Vec<String>` into the stable `ix_labels` form — the latter shared with the grouped-sweep exact-set ix_labels corpus filter so a filter set normalizes identically to the fingerprint it's matched against). `GroupField` enum (serde snake_case — matches the API + UI; the `creator_wallet`/`token_program_id` variants stay for legacy runs but the UI no longer offers them — singleton/constant ⇒ useless groups). `GroupKey(Vec<(GroupField,String)>)` + `to_json`. `group_key(fp, fields)` / `render_field` (exact-value; `∅` sentinel for missing; empty fields = single "ALL" group). **Binning is a future extension living entirely in `render_field`.** |
| `grouped_engine.rs` | `run_grouped_sweep` — `partition` builds `HashMap<GroupKey, Vec<usize>>` (O(tokens)), drop groups `< min_tokens`, deterministic order (largest-first, ties by key JSON). **Two-phase driver** routes groups by size vs the pool to keep cores busy without holding every group's accumulators resident: **large** groups (`≥ LARGE_GROUP_TOKEN_FACTOR`(4)`× pool_threads` tokens) are swept one-at-a-time via `run_sweep` (its inner `par_iter` saturates the pool on one group — the ALL/few-large case is unchanged); **small** groups are swept **across groups** via `par_iter` over the groups, each folded single-threaded by `sweep_group_serial` (calls the shared `engine::fill_outcomes`) — so many-small-groups runs no longer idle most cores. Peak accumulator memory = `threads × combos` (each group finalised to `ComboMetrics` then freed), **not** a full fold-time partition's `groups × combos × ComboAgg` (tens of GB at default ~1k groups × 5k combos). Results filled by survivor position so order stays deterministic across both phases; no nested pools. `make_group_result` wraps `best_combo(metrics, group_tokens, CoverageFloor)` (max robust realized `score` among combos clearing the floor `{ min_fired_abs, fire_frac }`, ties by `n_fired`/total PnL, most-fired fallback when none clear). `&dyn SweepObserver`: `set_total(surviving tokens)` up front, polls `cancelled()` (bails `"sweep cancelled"`). **Coarse→refine** lives in `run_grouped_with_refine(coarse_observer, observer, ...)`: no `RefineSpec` ⇒ a plain `run_grouped_sweep` using `observer`; with one ⇒ the coarse pass runs under `coarse_observer` (its own phase-tagged SSE stream) and uses `NoopSink` (its combo-id space is throwaway; a cancel still propagates via the observer's cancel flag), then `top_combo_ids(metrics, top_k)` per group seeds survivors (deduped across groups by `params_json`), `Strategy::refine` builds the neighborhood, and the **deduped union** (coarse-kept-first, `params_json`-keyed, truncated to the combo `cap`) is re-swept under `observer` (the final/persisted pass). Re-sweeping the union keeps one `combo_id` space, one `best_combo`, deterministic order. **Partial persistence (Phase 4):** `GroupSink` (+ `NoopSink`) is an incremental per-group callback — `run_grouped_sweep` calls `begin(group_count, combo_count)` once after the surviving + final-combo sets are fixed, then `group_done(group_index, &GroupResult, &combo_params)` per **fully-folded** group (deterministic order for large groups, arrival order for the small-group `par_iter`; impls must be `Sync`). A group is only emitted after its cancel check passes, so a sink may treat every emit as complete/persistable. The handler's sink forwards each emit to a single DB-writer task. |
| `registry.rs` | The **one** place a strategy is wired in. `MAX_COMBOS` cap; `tables_for(strategy_id) -> Option<GroupedSweepTables>` (per-strategy table triple; arms for `"tpsl1"`/`"tpsl2"`) + `strategy_ids()`; `run_grouped(..., coarse_observer: Arc<dyn SweepObserver + Send>, observer: Arc<dyn SweepObserver + Send>)` dispatch → `sweep_tpsl2` / `sweep_tpsl1` (each resolves axes via its `AxesSpec`, grid combo-count pre-check vs cap, samples + clamps coarse combos, runs `run_grouped_with_refine(coarse_observer, observer, ...)` on a **bounded** rayon pool inside `spawn_blocking`). `GroupedSweepOutput` (combo_count, resolved axes_json, groups). **Neither sweep needs a DB rule:** the base rule is synthesized in-process (`sweep_base_rule_tpsl{1,2}`) — the only base-rule field a sweep reads is `buy_amount` (`SWEEP_BASE_BUY_AMOUNT_SOL`). |
| `strategies/mod.rs` | `pub mod tpsl1;` + `pub mod tpsl2;` (a new strategy adds a sibling module here). |
| `strategies/tpsl2.rs` | TPSL2 `Strategy`/`ParamSpace`. `Tpsl2Params`/`Tpsl2Axes` (+ `Serialize`) sweep **all 15** rule knobs — TP/SL always-on, every other knob `Option` where `None` = unbounded/disabled (the default axis for the 10 lower-leverage knobs is a single `[None]`, so they don't expand the grid until the page supplies values). `AxesSpec` (page-editable grid; omitted/empty axis → default), `Tpsl2Axes::from_spec`/`combo_count`; `sample` builds the grid via a shared `combo_at(index)` mixed-radix decode (Grid = `0..combo_count`; `Random` draws **distinct** grid indices **without replacement** — `min(n, grid_size)` combos, logging when the grid is smaller than `n` instead of silently collapsing to duplicates (#9 hygiene); LHS = `lhs_index_plan` over the 15 axis lengths in declaration order), wrapping each into a `Tpsl2Combo { raw, rule }` whose `Tpsl2Rule` is **resolved once at sample time** (not cloned per `(combo×token)` in the hot loop). `prepare_token` returns `Tpsl2TokenState { cohort, cohort_bought }` — the launch cohort (`scalp_cohort`) plus its total-bought bag (the E5 denominator, computed only when `cohort_ratio` is swept), built **once per token** and shared across every entry *and* exit resolve; `resolve_entry` calls `entry::find_scalp_entry_with_cohort` (fed `state.cohort`) + `find_worst_case_paper_entry`; `resolve_exit` calls `exit::find_trade_driven_exit_with_cohort` on `&combo.rule` fed `state.cohort`/`state.cohort_bought` — so the E5 cohort `HashSet` + bag are **not** rebuilt per `(combo × token)` (#1 cohort-exit hoist), while the live fns' `&Tpsl2Rule` signatures stay unchanged, so decision parity is exact. `entry_key` = the 8 scalp-gate knobs (`Tpsl2EntryKey`). `order_for_entry_cache` stable-sorts the combo set by those 8 knobs so same-entry combos are contiguous (restores the engine's entry-cache hit rate under random/lhs/refine; #2). `refine` walks all 15 axes one at a time around each survivor (coordinate moves). See [strategies.md](strategies.md). |
| `strategies/tpsl1.rs` | TPSL1 `Strategy`/`ParamSpace`. TPSL1 is the token-creation-filter strategy, so it has **no per-trade entry gates** and **no cohort exit** — its swept set is the **exit ladder only** (6 knobs: TP/SL + the optional trailing/time/stall/liquidity exits). `Tpsl1Params`/`Tpsl1Axes`/`AxesSpec` mirror tpsl2's shape over that smaller set. `sample` wraps each combo into a `Tpsl1Combo { raw, rule }` (rule resolved once at sample time, as tpsl2; grid / uniform `Random` / `lhs_index_plan`-driven LHS over its 6 axes). `resolve_entry` uses `tpsl_sniper_1::entry::find_entry_fill_in_trades(trades, 1)` (cap 1, matching `run_backtest`; the token-creation filter ran upstream during corpus selection) — entry is **param-free**, so `EntryKey = ()` and it resolves once per token (no per-token state either: `TokenState = ()`); `resolve_exit` calls `exit::find_trade_driven_exit` on `&combo.rule` — see [strategies.md](strategies.md). `refine` walks its 6 exit axes one at a time (coordinate moves). `params_json` emits the `exit_*` keys (a subset of tpsl2's). |

## Combo column families (the per-combo result row)

Every drill-in combo row splits into **three families** — useful for knowing what
is a swept *input* vs. a measured *output*, and which family each column's storage
lives in:

- **Rule params** (the swept knob values; the strategy *input*). TPSL2 sweeps **all
  15**; TPSL1 the **exit subset** (6). These are *identical for a given `combo_id`
  across every group in a run*, so they are **deduped** into a per-run
  `<strategy>_grouped_sweep_combos(run_id, combo_id, params)` dictionary
  (migration `0007`) instead of being repeated on every `(group, combo)` results
  row (was the dominant on-disk cost). Read back by JOINing on `(run_id, combo_id)`.
  - Exit: `exit_take_profit`, `exit_stop_loss`, `exit_trailing_stop_pct`,
    `exit_time_stop_secs`, `exit_stall_secs`, `exit_liquidity_drop_pct`,
    `exit_cohort_ratio` (cohort = TPSL2 only).
  - Entry (TPSL2 scalp gates only): `entry_min_age_secs`, `entry_max_age_secs`,
    `entry_min_alive_sol`, `entry_min_organic_sol`, `entry_pullback_pct`,
    `entry_higher_low_secs`, `entry_max_cohort_held`, `entry_min_liquidity_sol`,
    `entry_min_organic_liq`. `entry_max_age_secs` is the scalp-window ceiling (paired
    with the `min_age` floor); it joins the entry-key (9 knobs now) since it changes
    the resolved entry. Its default axis is `[None]` — present but grid-inert until the
    page supplies values.
- **Evaluate params** (the measured *ranking/scoring outputs*): `score`, `win_rate`,
  `total_pnl_sol`, `expectancy_sol`, `profit_factor`, `median_pnl_pct`,
  `mean_pnl_pct`, `p90_pnl_pct`, `best_pnl_pct`, `worst_pnl_pct`, `std_pnl_pct`.
  Stored per `(group, combo)` on the results row; all the PnL/score floats are
  `REAL` (f32 — display/ranking only, migration `0007`), kept f64 in-memory for the
  fold/ranking precision. These 11 metrics are exactly what `retention.rs` keeps the
  extremes of — only the metric-extreme survivors per group reach the results row
  (write-time filter + the compaction probe for existing rows).
- **Extra-info params** (the remaining measured context): `combo_id`, `n_fired`,
  `n_open`, `n_closed` (counts, `INTEGER`), `avg_holding_secs`,
  `median_holding_secs`, and the exit-reason mix `n_exit_{take_profit,stop_loss,
  trailing,stall,time,liquidity,cohort,open}`. Also per `(group, combo)`.

## Persistence (per-strategy tables) + API

Tables are **separate per strategy** (`<strategy>_grouped_sweep_{runs,groups,results}`,
see [database.md](database.md)). Both TPSL2's and TPSL1's identical-shape triples
live in `0001_init` (TPSL1's `n_exit_cohort` column stays 0 — kept for schema
parity with the generic repo's INSERT); `0002` adds `status` + `groups_done` to
both `_runs` tables (Phase 4 partial persistence). The repo is generic and
**table-name-driven**; the registry maps `strategy_id` → the table triple.

- `models/grouped_sweep.rs` — `GroupedSweepRun` / `GroupedSweepGroupSummary` /
  `GroupedSweepResult` (serialize-only API models) + `GroupedSweepGroupWrite` (the
  write unit: a group + its ranked combo rows). The group summary/write carry
  `best_score: Option<f64>` (the headline robust-realized metric, `0003` migration
  added the nullable column) alongside the secondary `best_expectancy_sol`. The run
  model carries `status` (`running`/`completed`/`cancelled`) + `groups_done`
  (Phase 4 — a `cancelled` run is honestly partial), and the **history-metadata**
  columns (`0005`): `ix_labels_filter` / `field_filters` (the corpus filters the
  run used, stored verbatim from the request so the history panel + re-run can read
  them — previously applied in-memory then dropped), `token_cap` / `max_combos`
  (the submitted caps, distinct from the realized `token_count`/`combo_count`), and
  a user-editable `label`. All nullable → legacy rows read `null`.
- `storage/repositories/grouped_sweep_repo.rs` — `GroupedSweepTables { runs,
  groups, results }` + **incremental** `GroupedSweepRepo` writes (Phase 4):
  `insert_run` (run header up front, `status='running'`),
  `update_run_counts(run_id, group_count, combo_count)` (once the engine fixes
  them, for the picker's "done/total"), `append_group(run_id, group_write)` (one
  completed group + its combo rows in `chunks(2000)`, bumps `groups_done`, own
  txn), `finalize_completed(run_id, group_count, combo_count, axes_spec)` /
  `mark_cancelled(run_id)` (terminal status), `reconcile_orphaned_runs` (boot
  crash-recovery: `running` → `cancelled`), `update_label(run_id, Option<&str>)`
  (rename — blank clears to NULL; rows-affected so the handler 404s an unknown id),
  plus the reads `list_runs(limit)`,
  `list_groups(run_id)`, `list_results(run_id, group_id)` and deletes
  `delete_run(run_id)` / `delete_runs_before(cutoff)` (FK `ON DELETE CASCADE`).
  **Compaction** (one-time retention of existing rows, used by the `compact-sweeps`
  probe): `list_all_groups_for_compaction()` → every `(group_id, best_combo_id)`;
  `fetch_combo_metrics_for_group(group_id)` → the 11 metrics + `n_closed` mapped into
  `ComboMetrics` (other fields zeroed — retention reads only those) so the probe runs
  the **same** `retained_combo_ids`; `delete_combos_except(group_id, &keep)` (one
  `<> ALL($keep)` delete); `vacuum_full_results()` (physically reclaim disk, `ACCESS
  EXCLUSIVE`). Table names come only from fixed registry consts → SQL interpolation is
  injection-safe.
- `api/handlers/strategies/grouped_sweep.rs` — generic handler set:
  - `POST /api/strategies/sweeps` (`start_grouped_sweep`; body
    `{strategy_id, created_after?, created_before?, curve_only?,
    group_by: GroupField[], ix_labels_filter?: string[], min_tokens?,
    min_fired_abs?, fire_frac?, method?,
    axes?, token_cap?, max_combos?, fresh?}`; `ix_labels_filter` (page
    alternative to grouping by `ix_labels`, mutually exclusive on the form) is an
    **exact-set** instruction-label filter — applied in-memory after
    `attach_fingerprints` (so the unfiltered Parquet corpus cache is reused across
    filter values), keeping only tokens whose normalized `ix_labels` set equals it;
    empty/omitted ⇒ no filter; `min_fired_abs`/`fire_frac` are the
    coverage-floor knobs, default `10` / `0.05`; `method` = `grid` | `random:N` |
    `lhs:N` | `refine:N[:K]` (the refine form runs a coarse LHS:N pass then a
    per-group top-`K` neighborhood refine, `K` default 3, and the run's stored
    `method` tag is `"refine"`))
    — resolves
    tables, claims the **single-flight** gate (`AppState.sweep_running`; one
    CPU-heavy sweep at a time, 409 if busy), then **detaches the run via
    `actix_web::rt::spawn`** (`run_grouped_sweep_job`). Detaching is essential: a
    browser refresh / SPA nav aborts the POST, so if the run lived in the request
    future Actix would drop it mid-sweep — the `Gate` would fire
    (`sweep_running`→false, progress reset) and `/api/jobs/status` recovery would
    find nothing. **The POST returns as soon as the run is *admitted* — not when
    the sweep finishes:** the job reports its early result over a `oneshot`
    (`early_tx`) — a pre-fold validation error (no tokens, bad filter, …) or
    `202 {run_id, status:"started"}` once the run header is `insert_run`'d — and the
    handler returns that; the fold then runs detached. This frees the POST
    connection before the multi-minute fold. **Why it matters:** holding it open
    kept one of the browser's ~6 per-host HTTP/1.1 connections busy for the whole
    run, so a concurrent `POST .../sweeps/cancel` could sit queued in the browser
    until the sweep ended — making cancel look like a no-op. Post-admission outcomes
    (done / cancel / config-error) are surfaced via the DB run status + the
    `SweepFinished` SSE frame, not the (now `()`-returning) job. The spawned job
    loads the corpus via
    `load_grouped_corpus` (selection-keyed Parquet cache for a closed/settled window,
    else a fresh DB load; `fresh=true` forces a rebuild) + `attach_fingerprints`,
    runs via `registry::run_grouped` with two phase-tagged observers:
    `coarse_observer` (`phase="coarse"`, throwaway ProgressCell) for the coarse
    LHS pass and `observer` (`phase="sweep"`, real ProgressCell for status recovery)
    for the final sweep. The DB-writer task emits a third `phase="saving"` stream:
    on `Begin` it announces `0/N` groups saving; on each successful `append_group`
    it increments the count — so the frontend shows an honest saving bar instead of
    being stuck at 100%. Clears `sweep_cancel` first; on every exit path its `Gate`
    releases the single-flight gate, resets `sweep_progress`, and emits the terminal
    `SweepFinished`.
    **Partial persistence (Phase 4):** the job `insert_run`s the header
    (`status='running'`) **before** the sweep, spawns one DB-writer task, and
    passes a `HandlerSink` into `run_grouped`; the engine's per-group emits flow
    over an unbounded channel into that one task (`append_group` per group — serialized
    so concurrent small-group folds don't race the connection), so a cancel/crash
    keeps whatever already committed. These are all reached **after** the POST has
    already returned `202 {run_id}`, so each just stamps DB state + lets the `Gate`
    emit `SweepFinished` (the client reacts to that, not a response body): on success
    → `finalize_completed` (terminal counts + resolved axes); on **cooperative
    cancel** → `mark_cancelled` (the partial groups are KEPT, `status='cancelled'`);
    on a post-admission config error (bad axes / over-cap grid, no groups) →
    `delete_run` (drop the empty placeholder — the client briefly navigated to it,
    then the `SweepFinished` runs-list refresh drops back to the newest run). A crash
    leaves `status='running'`, reconciled to `cancelled` at next boot
    (`reconcile_orphaned_runs`).
  - `POST /api/strategies/sweeps/cancel` (`cancel_grouped_sweep`) — flips
    `AppState.sweep_cancel`; the engine polls it and bails. No-op if idle.
  - `DELETE /api/strategies/sweeps/{run_id}?strategy_id=` (`delete_run`) — drop one
    run (groups + results cascade via FK); 404 on unknown id.
  - `PATCH /api/strategies/sweeps/{run_id}?strategy_id=` (`rename_run`; body
    `{label}`) — set/clear a run's user-given name (blank ⇒ NULL); 404 on unknown
    id; invalidates `GroupedSweep`.
  - `DELETE /api/strategies/sweeps?strategy_id=&before=<rfc3339>` (`prune_runs`) —
    delete all runs created strictly before `before` (`before` required so it can't
    wipe everything). Both deletes invalidate the page's `GroupedSweep` cache tag.
  - `GET …/sweeps?strategy_id=&limit=` (runs), `GET …/sweeps/{run_id}/groups`
    (group summaries, best robust `score` first — `ORDER BY best_score DESC NULLS
    LAST`), `GET
    …/sweeps/{run_id}/groups/{group_id}/results` (a group's ranked combo rows;
    paged via `page`/`limit`, sorted via the multi-key `sort=col:dir,col:dir,…`
    param — `build_order_by` resolves each level through `resolve_sort` (direct
    metric col or `p_<param>` JSONB expr) and appends a stable `combo_id` tiebreak;
    legacy single `sort_col`/`sort_dir` still accepted; default `score DESC`).
    All GETs take `strategy_id` to resolve the table set.

## Frontend

See [frontend.md](frontend.md). `pages/strategies/GroupedSweepPage.tsx` +
`components/sweep/{SweepConfigForm,groupColumns,groupedTypes,fingerprintFilters,FingerprintGroupPicker}` + RTK `GroupedSweep`
(`fingerprintFilters.ts` holds the shared `parseNumbers`/`parseIxLabelsFilter` value-filter
parsers, and `FingerprintGroupPicker.tsx` the shared group-by + value-filter control — both
also reused by the dashboard's "Creation by token group" section)
hooks. The page is a generic `GroupedSweepView` parameterized by
`{strategyId, paramKeys, axes, storageKey, title}`, with two thin wrappers
exported: `GroupedSweepPage` (TPSL2, `/strategies/grouped-sweep`) and
`Tpsl1GroupedSweepPage` (TPSL1, `/strategies/grouped-sweep-tpsl1`). Each passes its
own `*_PARAM_KEYS` (matching the backend `params_json`), its `*_AXES`, a
per-strategy localStorage key, and namespaces the `DataTable` `tableId` by
`strategyId` so column toggles don't collide. `SweepConfigForm` takes `axes` +
`storageKey` props (no longer hardcoded to TPSL2) and hides the **Entry gates**
subsection when a strategy has no entry axes (TPSL1). Group-summary `DataTable` →
click a group → drill-in combo `DataTable`
(reuses `buildSweepColumns(paramKeys, paramColors?)`). The drill-in passes a
**per-column tint plan** (`lib/sweepParamColors.computeParamColumnColors(results,
paramKeys)`, memoized per group): a knob that's **constant** across the group's
combos renders dimmed (the eye skips fixed knobs), while a **varying** knob gives
each distinct value a stable low-opacity full-cell background (assigned in ascending
value order, palette local per column) so equal values read as a color band down the
column — near-identical combos differ only where the cells light up. The tint lands
on the `<td>` via the generic `ColumnDef.cellClassName(row)` hook (distinct from the
row-cluster `cellGroupClassName` the rule table uses; here every row is in the same
group, so the signal is per-value, not per-row). `buildGroupColumns(paramKeys)` (same key list the
drill-in receives): Group (fingerprint chips), the **Metrics** columns
(Tokens/Fired/**Best-score** — the headline ranking metric, default sort — then
Best-expectancy as a secondary readout, each a real sortable + numeric-filterable column),
then one column **per swept param** read from `best_params`, `group`-tagged
`entry`/`exit` so `DataTable` draws the block divider + tint, with the
high-leverage knobs `defaultVisible` and the rest behind the Columns toggle. The
page passes `groupLabels={{metrics,entry,exit}}` so `DataTable` renders a spanning
banner row over each block. Config form: created-at range, group-by field picker, an **instruction-label
filter** textarea (JSON array of labels → `ix_labels_filter`; disabled while the
`ix_labels` group-by is selected, since grouping by the set and pinning one set
are mutually exclusive; invalid JSON blocks Run),
editable param grid (prefilled with the backend defaults) split into **Entry gates ·
scalp** / **Exit gates** subsections (axes carry a `group` tag in `TPSL2_AXES`,
ordered field-for-field to match the TPSL2 rule modal), method (grid / random:N /
**coarse→refine** — the refine option adds a *Coarse N* + *Top-K / group* input
and submits `refine:N:K`), min-tokens, curve-only, projected combo-count badge
(grid = axis product; random/refine = the coarse N; blocks Run over the cap). The
Run-picker row also carries **Delete run** (current run) + **Clear runs before
`<date>`** (prune) controls — `useDeleteGroupedSweepRunMutation` /
`usePruneGroupedSweepsMutation`, both confirm via `window.confirm` and invalidate
`GroupedSweep` so the list refetches. **Partial runs (Phase 4):** the run picker
label shows `running N/total` or `partial N/total groups` for a non-`completed`
run (`runGroupsLabel`), and a `warning` `InlineAlert` banner above the group table
flags an in-progress/cancelled run so a partial set is never mistaken for a full
sweep. **`run()` jumps to the new run the moment start returns** (`202 {run_id}`,
not when the sweep ends) and watches it fill in live via per-group writes + SSE; a
cancel just leaves that same (now `cancelled`/partial) run selected, refreshed by
the `SweepFinished` runs-list invalidation. **Sweep history management (`0005`):** below the run picker,
`components/sweep/SelectedSweepHistory` renders a read-only summary of the
selected run's full launch config — token range, grouping, method, caps, field
filters, and the `ix_labels_filter` as a pretty multi-line JSON block (the result
tables never show it, so a saved run was otherwise illegible). It hosts the inline
**rename** (`useRenameGroupedSweepRunMutation` → the `PATCH`; the custom name also
prefixes the picker `<option>`) and a **Use these settings** button that re-runs
the config: it bumps a `reuseNonce` the `SweepConfigForm` watches and maps the
stored run back into the form's `SweepConfig` (`runToConfig` — parses the `method`
tag, `axes_spec`, filters, caps), scrolling the form into view; it never
auto-fires (a sweep is expensive). All of this reads the existing runs query —
pure metadata, no extra groups/results fetch.

## Invariants

- The hot loop is strategy-blind and walks the slim, wallet-interned `SweepTrade`
  projection built once per token at load (not `Trade`); the shared entry/exit/
  cohort fns are generic over `TradeRow` so the *same* code serves the live path
  (`T = Trade`) and the sweep (`T = SweepTrade`) — decision parity preserved, live
  runtime unchanged (monomorphized). Cohort sets are `u32`-keyed. Each combo's
  `Tpsl2Rule`/`Tpsl1Rule` is resolved **once at sample time** (`Tpsl{1,2}Combo`),
  not cloned per `(combo×token)`. The launch-cohort `HashSet<u32>` is now hoisted
  to **once per token** via `Strategy::prepare_token` (tpsl2 returns `scalp_cohort`,
  threaded into `find_scalp_entry_with_cohort`); the engine computes it before the
  combo loop and shares it across every entry resolve, so it no longer rebuilds
  per entry-tuple (was E×/token). The live/backtest paths still call
  `find_scalp_entry`, which computes the same cohort inline — decisions identical.
  The E5 cohort **and** its bought-bag denominator are likewise hoisted to once-per-
  token (`Tpsl2TokenState`) and fed to `find_trade_driven_exit_with_cohort`, so the
  exit no longer rebuilds the cohort `HashSet` + `cohort_flow` bag per `(combo ×
  token)` (#1); the live/backtest paths keep `find_trade_driven_exit` (inline
  cohort), decisions identical. (Only the cohort net-at-entry seed stays per-resolve
  — it depends on the per-tuple entry time.) Grouping is an
  O(tokens) partition on top of `engine.rs`; the per-token fold logic is shared
  (`engine::fill_outcomes`) between the parallel `run_sweep` and the grouped
  driver's serial `sweep_group_serial`, so both resolve identically.
- **Bounded sweep memory.** `ComboAgg` is O(1) per combo (fixed `QuantileSketch`,
  no per-token sample `Vec`) **and small** (~0.6 KB: 64 `u16` buckets/sign × 2
  sketches), so per-combo memory is independent of corpus size — and the one
  contiguous `vec![ComboAgg; n_combos]` stays affordable at high `max_combos`
  (a 256-bucket `u32` `ComboAgg` was ~4.2 KB ⇒ ~1.3 GB at ~315k combos, which
  OOM-aborts the process).
  The grouped driver holds at most `pool_threads × combos` accumulators at once
  (large groups swept serially via `run_sweep`; small groups across-groups via
  `par_iter`, each finalised to small `ComboMetrics` then freed) — deliberately
  not a full fold-time partition, which would keep `groups × combos × ComboAgg`
  resident (tens of GB at default ~1k groups × 5k combos). Quantile counts are
  integers ⇒ median/p90 are fold-order-independent.
  A group's finalised `Vec<ComboMetrics>` is also **freed right after the
  `GroupSink` persists it** (`free_persisted_metrics`): groups stream to the sink
  one at a time, so retaining every emitted group's full per-combo metrics in the
  returned `Vec<GroupResult>` would hold `groups × combos × ComboMetrics` resident
  — GBs at a large combo set (a `random:N`/`refine` run near `HARD_MAX_COMBOS`),
  which OOM-aborted the process even though every group was already on disk. The
  only post-sweep reader (the handler) just wants `groups.len()`, so the heavy
  field is dropped and the headline fields (`token_count`, `best_*`) stay. The
  coarse refine pass uses `NoopSink` (no emit) and keeps its metrics for
  `top_combo_ids`.
- DB per-mint cap keeps the **launch window** by default (`Selection.window =
  TradeWindow::LaunchWindow`, earliest-first `ROW_NUMBER`), not the newest N — so a
  high-volume token's first minutes (what `find_scalp_entry` decides on) are always
  present instead of being silently dropped (Rec 4). `TradeWindow::Recent` ranks
  newest-first for live-cache parity; the window is part of `selection_cache_key`.
- Bound every load: `Selection` caps tokens + per-mint trades; `min_tokens` drops
  weak groups **before** any sweep work; `MAX_COMBOS` (default 5000) caps
  combos/group — a run may raise it via `max_combos`, server-clamped to
  `HARD_MAX_COMBOS` (500k).
- **Sweep per-mint cap** (`sweep_per_mint_cap()` in `corpus.rs`): the handler caps
  each token to `SWEEP_PER_MINT_CAP` (default `SWEEP_DEFAULT_PER_MINT_CAP` = 5000),
  **not** the live `MAX_TRADES_RETAINED` (50k) — launch-window scalp entries decide
  on the first minutes, so this is a ~10–25× corpus + inner-loop cut for high-volume
  tokens. `DbSource::load` warns when any token hits the cap (raise it if a slow
  exit looks truncated).
- **Corpus memory pre-check** (`load_grouped_corpus` → `DbSource::estimate_corpus_bytes`):
  before loading any trade rows, estimate `Σ min(trade_count, per_mint_cap) ×
  estimated_per_trade_bytes()` via one `GROUP BY count(*)` per mint chunk and reject
  if it exceeds `SWEEP_CORPUS_MEMORY_BUDGET_MB` (default 512). This guards the
  **corpus** axis — the unbounded OOM-killer the `SWEEP_MEMORY_BUDGET_MB` accumulator
  guard does not cover — turning a process-killing mid-load OOM into a clean 4xx.
  Skipped on a Parquet **cache hit** (the cached corpus is already bounded; reading
  it never re-hits the DB). `estimated_per_trade_bytes` is derived from
  `size_of::<SweepTrade>()` so it tracks struct changes instead of drifting.
- **Combo-space batching** (`combo_batch_size`/`combo_batch_count` in `engine.rs`):
  the fold holds `vec![ComboAgg; combos]` per active fold (small-group phase runs up
  to `threads` concurrently), so a large combo set would peak at
  `threads × combos × ComboAgg`. Instead the combo space is folded in **batches** of
  `combo_batch_size` — the largest batch with `threads × batch × ComboAgg ≤
  SWEEP_MEMORY_BUDGET_MB` (default 1024) — each finalised to `ComboMetrics` and freed
  before the next, so peak is `threads × batch × ComboAgg` independent of total combo
  count. A combo set too big to hold at once is **swept in sequential batches, not
  rejected** (the old `check_sweep_memory` behaviour); `HARD_MAX_COMBOS` still bounds
  the work. Combo ids stay global (`offset + local`) so `best_combo` ranks across all
  batches and stored `combo_id`s are unchanged; the fold is order-independent so
  batching is a pure memory/CPU trade (cost: `n_batches × corpus_walk`, entries
  re-resolved per batch). Each token is folded once **per batch**, so the grouped
  driver scales the progress total to `tokens × n_batches`. Pairs with the slimmed
  `ComboAgg` (~0.6 KB).
- **Corpus memory pre-check** is the *other* budget (`SWEEP_CORPUS_MEMORY_BUDGET_MB`)
  — see the corpus-load bullet above; the two axes (corpus trade buffers vs. fold
  accumulators) are bounded independently.
- **Holistic admission** (Phase 0.2, `corpus::admit_corpus_load`): the corpus-axis
  and accumulator budgets can each clear in isolation yet **jointly** OOM once the
  already-resident live caches (~1 GB) are added. So admission also sums *current
  process RSS + estimated corpus + accumulator budget* against one total ceiling
  `SWEEP_TOTAL_MEMORY_BUDGET_MB` (default 3072; `total_memory_budget_bytes`). RSS is
  best-effort (`obs::process_rss_bytes`); if unreadable, the corpus-axis guard
  stands alone (prior behaviour). Skipped on a Parquet cache hit (no DB load).
- **Observability** (Phase 0.3, `sweep/obs.rs`): process RSS (`memory-stats` crate,
  cross-platform incl. win32) + a wall-clock are logged at each milestone —
  `admitted` / `corpus_loaded` / `done` in the handler, and at partition + "all
  groups folded" in `grouped_engine`. Every later memory phase is judged against
  this baseline on **both** axes (peak `rss_mb`, total `elapsed_s`), so a memory win
  that silently regresses speed is caught.
- **Memory levers** (Phase 1.3/1.4, request params — the cheapest knobs to bring a
  run under budget, no code change): `token_cap` cuts the token count, a tighter
  `created_after`/`created_before` window shrinks the candidate set before any trade
  load, `SWEEP_PER_MINT_CAP` cuts trades/token, and `curve_only` drops post-migration
  AMM legs before projection. `curve_only` stays an **explicit opt-in** (not a forced
  per-strategy default): dropping AMM legs changes the corpus for migrated tokens, so
  defaulting it on would diverge the backtest from the both-venue live path — a
  parity regression. Use it deliberately when the post-migration tail is noise for
  the strategy under test.
- Bounded rayon pool + the shared single-flight gate keep a sweep from starving
  the live trading hot path. `bounded_threads()` sizes the pool against the whole
  thread budget — `cores − tokio worker_threads − HTTP_WORKERS`, floored at 1 —
  so on a small box the sweep can't pin the cores ingest / sell-confirm run on;
  override with `SWEEP_RAYON_THREADS`.
- **Partial results are honest (Phase 4).** Only **fully-folded** groups are ever
  persisted (the engine emits a group only after its cancel check passes) and the
  checkpoint is the **group** boundary, never a half-folded group — a partial
  group's robust score + coverage floor assume the whole group folded, so showing
  one as complete would crown a "best combo" over a biased subset. A cancelled /
  crash-recovered run is marked `status='cancelled'` and rendered with a partial
  banner, never as a complete sweep. Groups are swept largest-first, so a partial
  set is the *most* statistically meaningful groups.
- **Storage retention is identical write-time and at compaction.** `group_to_write`
  (handler) filters each group's combo rows through `retention::retained_combo_ids`
  before persisting, so a fresh run only writes the per-metric-extreme survivors (+
  best_combo) — no sweep-eval cost, just **fewer INSERT rows/binds**. The existing
  26 M-row backlog is pruned by the same function via the **`probe compact-sweeps
  [tpsl1|tpsl2]`** subcommand (`main.rs::run_compact_sweeps`, DB-only — dispatched
  before trader/ingest init): per group it fetches the 11 metrics, runs
  `retained_combo_ids`, `delete_combos_except`, then `VACUUM (FULL)`s the table
  (`ACCESS EXCLUSIVE`, offline window). Because both paths call the one pure fn,
  existing and future rows survive by the same rule. The `_combos` param dictionary
  stays full (it's tiny — one row/combo/run); only the `_results` rows are trimmed,
  and the read-path JOIN naturally shows only survivors.
- **Adding a strategy** = `strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`)
  - a `registry.rs` arm (table triple + dispatch) + a `<x>_grouped_sweep_*`
  migration + (frontend) a param-key list / axes defs. Engine, grouping, repo,
  handler, and page are reused.
