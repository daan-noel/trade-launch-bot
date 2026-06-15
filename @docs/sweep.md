# Sweep — strategy-agnostic param-sweep engine + grouped sweeps

File-level map of `backend/src/sweep/` — the **generic** param-sweep & backtest
stack. Reference this instead of re-reading source. Related:
[strategies.md](strategies.md) (the tpsl2 pure fns the sweep reuses),
[database.md](database.md) (sweep tables), [frontend.md](frontend.md) (the sweep
page). The TPSL2 `Strategy` impl is `sweep/strategies/tpsl2.rs` (below).

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
  ─► (drop groups < min_tokens) ─► run_sweep per group ─► best combo by expectancy
  ─► <strategy>_grouped_sweep_{runs,groups,results} (DB)
  ─► /api/strategies/sweeps[...] ─► Grouped Sweep React page
```

- **Decision parity:** a strategy's `simulate` calls the *same* pure fns the live
  path uses, so backtest and live resolve identical entry/exit decisions.
- **PnL:** frictionless (pure price-to-price `round_trip`). A fees/slippage cost
  layer is a future hook in `round_trip`, not yet modelled.
- **Ranking (grouped):** a group's best combo = max **expectancy per trade**
  (`expectancy_sol`, mean net SOL / fired token), ties broken by `n_fired` then
  total PnL. The per-combo drill-in table still carries the robust `score`
  (`μ−1.64·σ/√n`) as its default sort.

## `backend/src/sweep/` (generic)

| File | Owns |
|---|---|
| `mod.rs` | Module map + parity note. |
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `TokenOutcome` (`Copy`, no String), `ExitCode`, `SweepMethod` (Grid/Random/LHS), frictionless `round_trip`. |
| `corpus.rs` | `CorpusSource` trait + `CacheSource` (live `TokenCache`, zero-copy) / `DbSource` (chunked, per-mint-capped batch query). `TokenTrades` carries `fp: TokenFingerprint` (grouping only — `simulate` never reads it). `Selection` = cap + `created_after`/`created_before` window + curve_only. Compact **trade-only** Parquet corpus cache (fingerprint-free). `attach_fingerprints(pool, &mut Corpus)` — a cheap separate chunked `tokens` lookup (incl. JSONB `max_sol_cost`/`spendable_sol_in` → bigint) that fills each token's `fp`. |
| `engine.rs` | `run_sweep` — `rayon` over tokens (combos inner, slice stays cache-hot); single fold thread → one `ComboAgg` per combo. Returns `SweepStats` + `Vec<ComboMetrics>`. **Reused verbatim per group.** Takes a `&dyn SweepObserver`: the fold thread calls `token_done()` per folded token (uncontended progress), and producers run via `try_for_each_with` so a cancel makes rayon stop scheduling tokens at once (only ≤pool-size in-flight finish, not the whole remaining corpus) — near-immediate abort; the caller discards the partial result. |
| `progress.rs` | `SweepObserver` trait (`set_total`, `token_done`, `cancelled`) — keeps the engine transport-agnostic. `SweepProgress` impl broadcasts throttled `SseEvent::SweepProgress { strategy_id, processed, total }` (~100 frames/run + final) where `total` = surviving tokens across all groups, and surfaces the shared `AppState.sweep_cancel` flag to the hot loop. `NoopObserver` for tests. |
| `aggregate.rs` | `ComboAgg` (streaming accumulator) → `ComboMetrics` (one ranked row): win rate, total/expectancy/median/mean/p90/best/worst PnL, `std_pnl_pct`, profit factor, robust `score` (`μ−Z·σ/√n` on closed trades), holding stats, exit-reason mix. |
| `grouping.rs` | Strategy-blind grouping. `TokenFingerprint` (creator, token program, CU limit/price, cashback, max_sol_cost, spendable_sol_in, initial_buy_sol, ix_labels), `from_token`, `extract_lamports`, `normalize_labels`. `GroupField` enum (serde snake_case — matches the API + UI; the `creator_wallet`/`token_program_id` variants stay for legacy runs but the UI no longer offers them — singleton/constant ⇒ useless groups). `GroupKey(Vec<(GroupField,String)>)` + `to_json`. `group_key(fp, fields)` / `render_field` (exact-value; `∅` sentinel for missing; empty fields = single "ALL" group). **Binning is a future extension living entirely in `render_field`.** |
| `grouped_engine.rs` | `run_grouped_sweep` — partition-then-reuse: `partition` builds `HashMap<GroupKey, Vec<usize>>` (O(tokens)), drop groups `< min_tokens`, build a sub-`Corpus` per group (Arc refcount-clone of `TokenTrades`, no trade buffer copy), call `run_sweep` per group (**sequential** loop; inner `par_iter` uses the one bounded pool — no nested pools). `GroupResult` (key, token_count, stats, metrics, best_combo_id, best_expectancy_sol); `best_combo` picks max expectancy among combos that fired. Takes a `&dyn SweepObserver`: `set_total(surviving tokens)` up front, polls `cancelled()` between groups + after each `run_sweep` (bails `"sweep cancelled"`). |
| `registry.rs` | The **one** place a strategy is wired in. `MAX_COMBOS` cap; `tables_for(strategy_id) -> Option<GroupedSweepTables>` (per-strategy table triple) + `strategy_ids()`; `run_grouped(..., observer: Arc<dyn SweepObserver + Send>)` dispatch → `sweep_tpsl2` (resolves axes via `AxesSpec`, grid combo-count pre-check vs cap, resolves base `Tpsl2Rule`, samples + clamps combos, runs `run_grouped_sweep` (observer threaded in) on a **bounded** rayon pool inside `spawn_blocking`). `GroupedSweepOutput` (combo_count, combo_params indexed by combo_id, resolved axes_json, groups). |
| `strategies/mod.rs` | `pub mod tpsl2;` (a new strategy adds a sibling module here). |
| `strategies/tpsl2.rs` | TPSL2 `Strategy`/`ParamSpace`. `Tpsl2Params`/`Tpsl2Axes` (+ `Serialize`) sweep **all 15** rule knobs — TP/SL always-on, every other knob `Option` where `None` = unbounded/disabled (the default axis for the 10 lower-leverage knobs is a single `[None]`, so they don't expand the grid until the page supplies values). `AxesSpec` (page-editable grid; omitted/empty axis → default), `Tpsl2Axes::from_spec`/`combo_count`; `sample` builds the grid by mixed-radix decode over the axis lengths. Overlays params onto a base `Tpsl2Rule` and calls `entry::find_scalp_entry` + `find_worst_case_paper_entry` + `exit::find_trade_driven_exit` — see [strategies.md](strategies.md). |

## Persistence (per-strategy tables) + API
Tables are **separate per strategy** (`<strategy>_grouped_sweep_{runs,groups,results}`,
see [database.md](database.md), migration `0004`). The repo is generic and
**table-name-driven**; the registry maps `strategy_id` → the table triple.

- `models/grouped_sweep.rs` — `GroupedSweepRun` / `GroupedSweepGroupSummary` /
  `GroupedSweepResult` (serialize-only API models) + `GroupedSweepGroupWrite` (the
  write unit: a group + its ranked combo rows).
- `storage/repositories/grouped_sweep_repo.rs` — `GroupedSweepTables { runs,
  groups, results }` + `GroupedSweepRepo::{save_run (run + groups + each group's
  combo rows, one txn, results in `chunks(2000)`), list_runs(limit),
  list_groups(run_id), list_results(run_id, group_id), delete_run(run_id),
  delete_runs_before(cutoff)}` (deletes rely on the `_groups`/`_results` FK
  `ON DELETE CASCADE`). Table names come only
  from fixed registry consts → SQL interpolation is injection-safe.
- `api/handlers/strategies/grouped_sweep.rs` — generic handler set:
  - `POST /api/strategies/sweeps` (`start_grouped_sweep`; body
    `{strategy_id, rule_id?, created_after?, created_before?, curve_only?,
    group_by: GroupField[], min_tokens?, method?, axes?, token_cap?}`) — resolves
    tables, claims the **single-flight** gate (`AppState.sweep_running`; one
    CPU-heavy sweep at a time, 409 if busy), loads the corpus fresh from
    `DbSource` + `attach_fingerprints`, runs via `registry::run_grouped` with a
    `SweepProgress` observer (clears `sweep_cancel` first; streams `sweep_progress`
    SSE), persists, returns the run. A cooperative cancel returns `{cancelled:true}`
    (no run persisted) instead of erroring.
  - `POST /api/strategies/sweeps/cancel` (`cancel_grouped_sweep`) — flips
    `AppState.sweep_cancel`; the engine polls it and bails. No-op if idle.
  - `DELETE /api/strategies/sweeps/{run_id}?strategy_id=` (`delete_run`) — drop one
    run (groups + results cascade via FK); 404 on unknown id.
  - `DELETE /api/strategies/sweeps?strategy_id=&before=<rfc3339>` (`prune_runs`) —
    delete all runs created strictly before `before` (`before` required so it can't
    wipe everything). Both deletes invalidate the page's `GroupedSweep` cache tag.
  - `GET …/sweeps?strategy_id=&limit=` (runs), `GET …/sweeps/{run_id}/groups`
    (group summaries, best expectancy first), `GET
    …/sweeps/{run_id}/groups/{group_id}/results` (a group's ranked combo rows).
    All GETs take `strategy_id` to resolve the table set.

## Frontend
See [frontend.md](frontend.md). `pages/strategies/GroupedSweepPage.tsx` +
`components/sweep/{SweepConfigForm,groupColumns,groupedTypes}` + RTK `GroupedSweep`
hooks. Group-summary `DataTable` → click a group → drill-in combo `DataTable`
(reuses `buildSweepColumns`). `buildGroupColumns(paramKeys)` (same key list the
drill-in receives): Group (fingerprint chips), the **Metrics** columns
(Tokens/Fired/Best-expectancy — each a real sortable + numeric-filterable column),
then one column **per swept param** read from `best_params`, `group`-tagged
`entry`/`exit` so `DataTable` draws the block divider + tint, with the
high-leverage knobs `defaultVisible` and the rest behind the Columns toggle. The
page passes `groupLabels={{metrics,entry,exit}}` so `DataTable` renders a spanning
banner row over each block. Config form: created-at range, group-by field picker,
editable param grid (prefilled with the backend defaults), method (grid/random:N),
min-tokens, curve-only, projected combo-count badge (blocks Run over the cap). The
Run-picker row also carries **Delete run** (current run) + **Clear runs before
`<date>`** (prune) controls — `useDeleteGroupedSweepRunMutation` /
`usePruneGroupedSweepsMutation`, both confirm via `window.confirm` and invalidate
`GroupedSweep` so the list refetches.

## Invariants
- The hot loop is strategy-blind and allocation-free; grouping is an O(tokens)
  partition + O(groups) Arc-clones bolted on top — `engine.rs`/`aggregate.rs` are
  reused **unchanged** per group.
- Bound every load: `Selection` caps tokens + per-mint trades; `min_tokens` drops
  weak groups **before** any sweep work; `MAX_COMBOS` caps combos/group.
- Bounded rayon pool + the shared single-flight gate keep a sweep from starving
  the live trading hot path.
- **Adding a strategy** = `strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`)
  + a `registry.rs` arm (table triple + dispatch) + a `<x>_grouped_sweep_*`
  migration + (frontend) a param-key list / axes defs. Engine, grouping, repo,
  handler, and page are reused.
