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
  ─► <strategy>_grouped_sweep_{runs,groups,results} (DB)
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
|---|---|
| `mod.rs` | Module map + parity note. |
| `strategy.rs` | `Strategy` + `ParamSpace` traits. **Coarse→refine:** `ParamSpace::refine(survivors) -> Vec<Params>` (default empty) returns a **coordinate-move neighborhood** — each survivor with one axis moved to its adjacent candidate (`neighbor_indices`), holding the others fixed (≤ 2× multi-valued-axes neighbors/survivor, linear in axes); both tpsl impls build it via `index_of` (generic lookup so float axes dodge `clippy::float_cmp`). `RefineSpec { top_k }` + `parse_method(s) -> (SweepMethod, Option<RefineSpec>)` parse `refine:N[:K]` into a coarse LHS:N pass + a per-group top-`K` refine (`K` default 3). Plus the original `Strategy` `Strategy` is **factored into entry then exit** (not one `simulate`): assoc types `Entry`/`EntryKey: PartialEq`/`TokenState`, and `entry_key(&Params) -> EntryKey` / `prepare_token(&[SweepTrade]) -> TokenState` / `resolve_entry(&[SweepTrade], &TokenState, &Params) -> Entry` / `resolve_exit(&[SweepTrade], &Entry, &Params) -> TokenOutcome`. The engine resolves the (expensive) entry **once per distinct `entry_key` per token** and reuses it across that key's exit sub-grid (sound because the resolved entry depends only on the entry params, never the exit ladder). A param-free/inseparable entry sets `EntryKey = ()` so it resolves once per token. `prepare_token` computes param-independent per-token state **once** before the combo loop (tpsl2's launch cohort; `TokenState = ()` when there's none) and threads it into every `resolve_entry`. All take the slim `&[SweepTrade]` projection (not `Trade`). `TokenOutcome` (`Copy`, no String), `ExitCode`, `SweepMethod` (Grid / `Random{n,seed}` / `LatinHypercube{n,seed}`), the shared `lhs_index_plan(rng,n,axis_lens) -> Vec<Vec<usize>>` (a **real** discrete LHS: per-axis balanced strata `⌊n/len⌋–⌈n/len⌉`, each column independently shuffled so axes decorrelate — what `Random`'s uniform draw only balances in expectation; both strategies' `sample` use it for the LHS arm), and the cost layer: frictionless `round_trip` (test baseline) + `CostModel`/`CostModel::pumpfun_default` + `round_trip_with_costs` (the single price gate every `resolve_exit` uses). |
| `projection.rs` | `SweepTrade` — the slim per-trade row the hot loop walks (only the `TradeRow` fields; wallet interned to a token-local `u32`, `tx_signature` the one retained string; ~3× smaller than `Trade`, so cohort membership is `u32`-keyed not String-keyed). `project_trades(&[Trade]) -> (Vec<SweepTrade>, Vec<Box<str>>)` interns wallets per token via `WalletInterner`. The shared entry/exit/cohort fns are generic over `TradeRow` (defined in `models/trade.rs`), so they run over **either** the live `Trade` or `SweepTrade` with one implementation — decision parity preserved, live path unchanged. |
| `corpus.rs` | `CorpusSource` trait + `CacheSource` (live `TokenCache`) / `DbSource` (chunked, per-mint-capped batch query). Each token is **projected once at load** into a slim, wallet-interned `SweepTrade` buffer (see `projection.rs`) — `Trade` never enters the sweep loop. `TokenTrades { trades: Arc<Vec<SweepTrade>>, wallets: Arc<Vec<Box<str>>> (u32→address, only for Parquet write), fp: TokenFingerprint }` (`fp` = grouping only — `simulate` never reads it). `TokenTrades::from_trades` is the single projection entry point (cache/DB/tests all route through it; `curve_only` filters `Trade` before the projection drops `venue`). `Selection` = cap + `created_after`/`created_before` window + `TradeWindow` (`LaunchWindow` default = earliest-N per mint so the launch prefix the entry logic needs survives the per-mint cap, vs `Recent` = newest-N for live parity) + curve_only. Compact **slim** Parquet corpus cache (`SweepTrade` columns + wallet/tx strings re-interned on read; fingerprint-free; drops `vtok`/`rtok`/`venue` no sweep fn reads). `attach_fingerprints(pool, &mut Corpus)` — a cheap separate chunked `tokens` lookup (incl. JSONB `max_sol_cost`/`spendable_sol_in` → bigint) that fills each token's `fp`. **Corpus reuse across runs:** `load_grouped_corpus(db, sel, cache_dir, fresh)` serves a **closed/settled** window (`selection_is_cacheable`: `created_before` ≥ `CORPUS_CACHE_SETTLE_HOURS`=2h in the past) from a selection-keyed Parquet cache (`selection_cache_key` over window+caps+curve_only+mints) via `load_or_build`, skipping the DB load on a hit; an open/recent window always loads fresh, and `fresh=true` forces a rebuild + cache rewrite. `corpus_cache_dir()` = `$SWEEP_CORPUS_CACHE_DIR` else a temp subdir. Fingerprints are attached after (cache is fingerprint-free). |
| `engine.rs` | `run_sweep` — `rayon` over tokens (combos inner, slice stays cache-hot); single fold thread → one `ComboAgg` per combo. Returns `SweepStats` + `Vec<ComboMetrics>`. **Reused verbatim per group.** Per token it computes `prepare_token` once (shared per-token state, e.g. the launch cohort), keeps the last resolved `(entry_key, entry)` and calls `resolve_entry` only when the key changes, then `resolve_exit` per combo — so on a grid (exit axes = low-order digits ⇒ contiguous same-entry blocks) the expensive entry resolves **once per entry-tuple, not per combo** (E×/token, not E·X×). `random:N` still resolves correctly; only the reuse rate drops. Takes a `&dyn SweepObserver`: the fold thread calls `token_done()` per folded token (uncontended progress), and producers run via `try_for_each_with` so a cancel makes rayon stop scheduling tokens at once. The cancel flag is also polled **inside** each token's combo fold every `CANCEL_CHECK_STRIDE` (256) combos, so a large combo set (up to `HARD_MAX_COMBOS`) bails mid-token instead of after — worst-case stop latency is one chunk × ≤pool-size in-flight tokens (sub-100ms), not one full token. A token that bails mid-fold is never sent to the folder; the caller discards the partial result. |
| `progress.rs` | `SweepObserver` trait (`set_total`, `token_done`, `cancelled`) — keeps the engine transport-agnostic. `SweepProgress` impl broadcasts throttled `SseEvent::SweepProgress { strategy_id, processed, total }` (~100 frames/run + final) where `total` = surviving tokens across all groups, and surfaces the shared `AppState.sweep_cancel` flag to the hot loop. `NoopObserver` for tests. |
| `aggregate.rs` | `ComboAgg` (streaming accumulator) → `ComboMetrics` (one ranked row): win rate, total/expectancy/median/mean/p90/best/worst PnL, `std_pnl_pct`, profit factor, robust `score` (`μ−Z·σ/√n` on closed trades), holding stats, exit-reason mix. **O(1) per combo:** median/p90 (pnl% + holding) come from a fixed-size `QuantileSketch` (DDSketch-style signed log buckets, `SKETCH_N`=256/sign, ~4% relative error), not a per-token `Vec` — so a combo's memory no longer grows with the tokens it fires on (the old `Vec<f32>`/`Vec<i32>` reached multi-GB at `HARD_MAX_COMBOS` × big groups). Counts are integers ⇒ the estimate is **fold-order-independent** (parallel/grouped folds reproduce it). Best/worst (running min/max) and all means (running sums) stay **exact**; only the two interior quantiles approximate. |
| `grouping.rs` | Strategy-blind grouping. `TokenFingerprint` (creator, token program, CU limit/price, cashback, max_sol_cost, spendable_sol_in, initial_buy_sol, ix_labels), `from_token`, `extract_lamports`, `normalize_labels`. `GroupField` enum (serde snake_case — matches the API + UI; the `creator_wallet`/`token_program_id` variants stay for legacy runs but the UI no longer offers them — singleton/constant ⇒ useless groups). `GroupKey(Vec<(GroupField,String)>)` + `to_json`. `group_key(fp, fields)` / `render_field` (exact-value; `∅` sentinel for missing; empty fields = single "ALL" group). **Binning is a future extension living entirely in `render_field`.** |
| `grouped_engine.rs` | `run_grouped_sweep` — `partition` builds `HashMap<GroupKey, Vec<usize>>` (O(tokens)), drop groups `< min_tokens`, deterministic order (largest-first, ties by key JSON). **Two-phase driver** routes groups by size vs the pool to keep cores busy without holding every group's accumulators resident: **large** groups (`≥ LARGE_GROUP_TOKEN_FACTOR`(4)`× pool_threads` tokens) are swept one-at-a-time via `run_sweep` (its inner `par_iter` saturates the pool on one group — the ALL/few-large case is unchanged); **small** groups are swept **across groups** via `par_iter` over the groups, each folded single-threaded by `sweep_group_serial` (calls the shared `engine::fill_outcomes`) — so many-small-groups runs no longer idle most cores. Peak accumulator memory = `threads × combos` (each group finalised to `ComboMetrics` then freed), **not** a full fold-time partition's `groups × combos × ComboAgg` (tens of GB at default ~1k groups × 5k combos). Results filled by survivor position so order stays deterministic across both phases; no nested pools. `make_group_result` wraps `best_combo(metrics, group_tokens, CoverageFloor)` (max robust realized `score` among combos clearing the floor `{ min_fired_abs, fire_frac }`, ties by `n_fired`/total PnL, most-fired fallback when none clear). `&dyn SweepObserver`: `set_total(surviving tokens)` up front, polls `cancelled()` (bails `"sweep cancelled"`). **Coarse→refine** lives in `run_grouped_with_refine`: no `RefineSpec` ⇒ a plain `run_grouped_sweep`; with one ⇒ a silent (progress-suppressed via `progress::CancelOnly`, still cancellable) coarse pass, then `top_combo_ids(metrics, top_k)` per group seeds survivors (deduped across groups by `params_json`), `Strategy::refine` builds the neighborhood, and the **deduped union** (coarse-kept-first, `params_json`-keyed, truncated to the combo `cap` so the cap only ever trims refinement) is re-swept — that final pass drives the bar and its combo list is the stored combo space. Re-sweeping the union (vs merging two combo spaces) keeps one `combo_id` space, one `best_combo`, deterministic order. |
| `registry.rs` | The **one** place a strategy is wired in. `MAX_COMBOS` cap; `tables_for(strategy_id) -> Option<GroupedSweepTables>` (per-strategy table triple; arms for `"tpsl1"`/`"tpsl2"`) + `strategy_ids()`; `run_grouped(..., floor: CoverageFloor, ..., observer: Arc<dyn SweepObserver + Send>)` dispatch → `sweep_tpsl2` / `sweep_tpsl1` (each resolves axes via its `AxesSpec`, grid combo-count pre-check vs cap, samples + clamps coarse combos, runs `run_grouped_with_refine` (observer + coverage `floor` + `Option<RefineSpec>` + combo `cap` threaded in — `params_json` for the final combo list is captured **inside** the task since refine may grow it) on a **bounded** rayon pool inside `spawn_blocking`). `GroupedSweepOutput` (combo_count, combo_params indexed by combo_id, resolved axes_json, groups). **Neither sweep needs a DB rule:** the base rule is synthesized in-process (`sweep_base_rule_tpsl{1,2}`) — the only base-rule field a sweep reads is `buy_amount` (`SWEEP_BASE_BUY_AMOUNT_SOL`, the round-trip notional; every entry/exit knob is overlaid by the swept axes, the rest is unused), so `run_grouped` takes no `pool`/`rule_id`. |
| `strategies/mod.rs` | `pub mod tpsl1;` + `pub mod tpsl2;` (a new strategy adds a sibling module here). |
| `strategies/tpsl2.rs` | TPSL2 `Strategy`/`ParamSpace`. `Tpsl2Params`/`Tpsl2Axes` (+ `Serialize`) sweep **all 15** rule knobs — TP/SL always-on, every other knob `Option` where `None` = unbounded/disabled (the default axis for the 10 lower-leverage knobs is a single `[None]`, so they don't expand the grid until the page supplies values). `AxesSpec` (page-editable grid; omitted/empty axis → default), `Tpsl2Axes::from_spec`/`combo_count`; `sample` builds the grid via a shared `combo_at(index)` mixed-radix decode (Grid = `0..combo_count`; `Random` draws **distinct** grid indices **without replacement** — `min(n, grid_size)` combos, logging when the grid is smaller than `n` instead of silently collapsing to duplicates (#9 hygiene); LHS = `lhs_index_plan` over the 15 axis lengths in declaration order), wrapping each into a `Tpsl2Combo { raw, rule }` whose `Tpsl2Rule` is **resolved once at sample time** (not cloned per `(combo×token)` in the hot loop). `prepare_token` returns the token's launch cohort (`scalp_cohort` → `TokenState = HashSet<u32>`, built once per token); `resolve_entry` calls `entry::find_scalp_entry_with_cohort` (fed that cohort) + `find_worst_case_paper_entry`; `resolve_exit` calls `exit::find_trade_driven_exit` on `&combo.rule` — the live fns' `&Tpsl2Rule` signatures are unchanged, so decision parity is exact. `entry_key` = the 8 scalp-gate knobs (`Tpsl2EntryKey`). `refine` walks all 15 axes one at a time around each survivor (coordinate moves). See [strategies.md](strategies.md). |
| `strategies/tpsl1.rs` | TPSL1 `Strategy`/`ParamSpace`. TPSL1 is the token-creation-filter strategy, so it has **no per-trade entry gates** and **no cohort exit** — its swept set is the **exit ladder only** (6 knobs: TP/SL + the optional trailing/time/stall/liquidity exits). `Tpsl1Params`/`Tpsl1Axes`/`AxesSpec` mirror tpsl2's shape over that smaller set. `sample` wraps each combo into a `Tpsl1Combo { raw, rule }` (rule resolved once at sample time, as tpsl2; grid / uniform `Random` / `lhs_index_plan`-driven LHS over its 6 axes). `resolve_entry` uses `tpsl_sniper_1::entry::find_entry_fill_in_trades(trades, 1)` (cap 1, matching `run_backtest`; the token-creation filter ran upstream during corpus selection) — entry is **param-free**, so `EntryKey = ()` and it resolves once per token (no per-token state either: `TokenState = ()`); `resolve_exit` calls `exit::find_trade_driven_exit` on `&combo.rule` — see [strategies.md](strategies.md). `refine` walks its 6 exit axes one at a time (coordinate moves). `params_json` emits the `exit_*` keys (a subset of tpsl2's). |

## Persistence (per-strategy tables) + API
Tables are **separate per strategy** (`<strategy>_grouped_sweep_{runs,groups,results}`,
see [database.md](database.md)). Both TPSL2's and TPSL1's identical-shape triples
live in `0001_init` (TPSL1's `n_exit_cohort` column stays 0 — kept for schema
parity with the generic repo's INSERT). The repo is generic and **table-name-driven**; the registry maps
`strategy_id` → the table triple.

- `models/grouped_sweep.rs` — `GroupedSweepRun` / `GroupedSweepGroupSummary` /
  `GroupedSweepResult` (serialize-only API models) + `GroupedSweepGroupWrite` (the
  write unit: a group + its ranked combo rows). The group summary/write carry
  `best_score: Option<f64>` (the headline robust-realized metric, `0003` migration
  added the nullable column) alongside the secondary `best_expectancy_sol`.
- `storage/repositories/grouped_sweep_repo.rs` — `GroupedSweepTables { runs,
  groups, results }` + `GroupedSweepRepo::{save_run (run + groups + each group's
  combo rows, one txn, results in `chunks(2000)`), list_runs(limit),
  list_groups(run_id), list_results(run_id, group_id), delete_run(run_id),
  delete_runs_before(cutoff)}` (deletes rely on the `_groups`/`_results` FK
  `ON DELETE CASCADE`). Table names come only
  from fixed registry consts → SQL interpolation is injection-safe.
- `api/handlers/strategies/grouped_sweep.rs` — generic handler set:
  - `POST /api/strategies/sweeps` (`start_grouped_sweep`; body
    `{strategy_id, created_after?, created_before?, curve_only?,
    group_by: GroupField[], min_tokens?, min_fired_abs?, fire_frac?, method?,
    axes?, token_cap?, max_combos?, fresh?}`; `min_fired_abs`/`fire_frac` are the
    coverage-floor knobs, default `10` / `0.05`; `method` = `grid` | `random:N` |
    `lhs:N` | `refine:N[:K]` (the refine form runs a coarse LHS:N pass then a
    per-group top-`K` neighborhood refine, `K` default 3, and the run's stored
    `method` tag is `"refine"`))
    — resolves
    tables, claims the **single-flight** gate (`AppState.sweep_running`; one
    CPU-heavy sweep at a time, 409 if busy), then **detaches the run via
    `actix_web::rt::spawn`** (`run_grouped_sweep_job`) and awaits it for the
    response. Detaching is essential: a browser refresh / SPA nav aborts the POST,
    so if the run lived in the request future Actix would drop it mid-sweep — the
    `Gate` would fire (`sweep_running`→false, progress reset) and `/api/jobs/status`
    recovery would find nothing. The spawned job loads the corpus via
    `load_grouped_corpus` (selection-keyed Parquet cache for a closed/settled window,
    else a fresh DB load; `fresh=true` forces a rebuild) + `attach_fingerprints`,
    runs via `registry::run_grouped` with a
    `SweepProgress` observer (clears `sweep_cancel` first; streams `sweep_progress`
    SSE), persists, and on every exit path its `Gate` releases the single-flight
    gate, resets `sweep_progress`, and emits the terminal `SweepFinished`. A
    cooperative cancel returns `{cancelled:true}` (no run persisted) instead of
    erroring.
  - `POST /api/strategies/sweeps/cancel` (`cancel_grouped_sweep`) — flips
    `AppState.sweep_cancel`; the engine polls it and bails. No-op if idle.
  - `DELETE /api/strategies/sweeps/{run_id}?strategy_id=` (`delete_run`) — drop one
    run (groups + results cascade via FK); 404 on unknown id.
  - `DELETE /api/strategies/sweeps?strategy_id=&before=<rfc3339>` (`prune_runs`) —
    delete all runs created strictly before `before` (`before` required so it can't
    wipe everything). Both deletes invalidate the page's `GroupedSweep` cache tag.
  - `GET …/sweeps?strategy_id=&limit=` (runs), `GET …/sweeps/{run_id}/groups`
    (group summaries, best robust `score` first — `ORDER BY best_score DESC NULLS
    LAST`), `GET
    …/sweeps/{run_id}/groups/{group_id}/results` (a group's ranked combo rows).
    All GETs take `strategy_id` to resolve the table set.

## Frontend
See [frontend.md](frontend.md). `pages/strategies/GroupedSweepPage.tsx` +
`components/sweep/{SweepConfigForm,groupColumns,groupedTypes}` + RTK `GroupedSweep`
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
(reuses `buildSweepColumns`). `buildGroupColumns(paramKeys)` (same key list the
drill-in receives): Group (fingerprint chips), the **Metrics** columns
(Tokens/Fired/**Best-score** — the headline ranking metric, default sort — then
Best-expectancy as a secondary readout, each a real sortable + numeric-filterable column),
then one column **per swept param** read from `best_params`, `group`-tagged
`entry`/`exit` so `DataTable` draws the block divider + tint, with the
high-leverage knobs `defaultVisible` and the rest behind the Columns toggle. The
page passes `groupLabels={{metrics,entry,exit}}` so `DataTable` renders a spanning
banner row over each block. Config form: created-at range, group-by field picker,
editable param grid (prefilled with the backend defaults) split into **Entry gates ·
scalp** / **Exit gates** subsections (axes carry a `group` tag in `TPSL2_AXES`,
ordered field-for-field to match the TPSL2 rule modal), method (grid / random:N /
**coarse→refine** — the refine option adds a *Coarse N* + *Top-K / group* input
and submits `refine:N:K`), min-tokens, curve-only, projected combo-count badge
(grid = axis product; random/refine = the coarse N; blocks Run over the cap). The
Run-picker row also carries **Delete run** (current run) + **Clear runs before
`<date>`** (prune) controls — `useDeleteGroupedSweepRunMutation` /
`usePruneGroupedSweepsMutation`, both confirm via `window.confirm` and invalidate
`GroupedSweep` so the list refetches.

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
  (`find_trade_driven_exit`'s E5 cohort set is still per-resolve.) Grouping is an
  O(tokens) partition on top of `engine.rs`; the per-token fold logic is shared
  (`engine::fill_outcomes`) between the parallel `run_sweep` and the grouped
  driver's serial `sweep_group_serial`, so both resolve identically.
- **Bounded sweep memory.** `ComboAgg` is O(1) per combo (fixed `QuantileSketch`,
  no per-token sample `Vec`), so per-combo memory is independent of corpus size.
  The grouped driver holds at most `pool_threads × combos` accumulators at once
  (large groups swept serially via `run_sweep`; small groups across-groups via
  `par_iter`, each finalised to small `ComboMetrics` then freed) — deliberately
  not a full fold-time partition, which would keep `groups × combos × ComboAgg`
  resident (tens of GB at default ~1k groups × 5k combos). Quantile counts are
  integers ⇒ median/p90 are fold-order-independent.
- DB per-mint cap keeps the **launch window** by default (`Selection.window =
  TradeWindow::LaunchWindow`, earliest-first `ROW_NUMBER`), not the newest N — so a
  high-volume token's first minutes (what `find_scalp_entry` decides on) are always
  present instead of being silently dropped (Rec 4). `TradeWindow::Recent` ranks
  newest-first for live-cache parity; the window is part of `selection_cache_key`.
- Bound every load: `Selection` caps tokens + per-mint trades; `min_tokens` drops
  weak groups **before** any sweep work; `MAX_COMBOS` (default 5000) caps
  combos/group — a run may raise it via `max_combos`, server-clamped to
  `HARD_MAX_COMBOS` (500k).
- Bounded rayon pool + the shared single-flight gate keep a sweep from starving
  the live trading hot path. `bounded_threads()` sizes the pool against the whole
  thread budget — `cores − tokio worker_threads − HTTP_WORKERS`, floored at 1 —
  so on a small box the sweep can't pin the cores ingest / sell-confirm run on;
  override with `SWEEP_RAYON_THREADS`.
- **Adding a strategy** = `strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`)
  + a `registry.rs` arm (table triple + dispatch) + a `<x>_grouped_sweep_*`
  migration + (frontend) a param-key list / axes defs. Engine, grouping, repo,
  handler, and page are reused.
