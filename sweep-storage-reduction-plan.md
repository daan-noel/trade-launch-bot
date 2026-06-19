# Grouped-sweep storage reduction: dedup + per-metric-extremes retention

## Context

`tpsl2_grouped_sweep_results` is **29 GB / 26 M rows** (`rows = runs × groups × combos`; combo_count
per run is 186k–331k). Two independent problems:

1. **Per-row bloat (lossless to fix).** Every `(group, combo)` row stores the full `params` JSONB
   even though params depend only on `combo_id` (run-level), plus `bigint`/`double` columns that only
   need `int`/`real`. **This is already written as migration `0007` but NOT applied** — the DB is at
   `_sqlx_migrations` version 5; `0006` + `0007` are pending (backend not restarted since they landed).
   The running Rust code (`append_group`, `metrics_to_result`) *already* assumes the post-`0007` schema
   (binds `i32`/`f32`, writes a `_combos` dict), so `0007` must run for sweeps to insert correctly
   anyway. Applying it is lossless and reclaims most of the 29 GB.

2. **Row-count explosion (lossy to fix).** `0007` shrinks bytes-per-row but not the multiplicative row
   count. We additionally retain only the analytically interesting combos per group.

**Why not pure top-K (my earlier suggestion):** top-K by `score` discards a combo that's best on
`win_rate` but mid on `score`. The user's idea — keep the top/bottom extremes of *every* ranking metric
— preserves the frontier on all metrics. Adopted, **with guards proven necessary by the data:** in the
largest group (331,776 combos) `win_rate` has only 156 distinct values and its worst value alone matches
**110,592 combos** (exactly the `score IS NULL`, `n_closed < 2` junk). So "keep all combos sharing a
top/bottom value" is unbounded and would save almost nothing. We bound it.

## Decisions (locked with user)

- **Order:** apply `0007` first (lossless), then retention.
- **Tie handling:** drop NULL/under-sampled combos; cap representatives per tied value (bounded).
- **Where:** both a one-time compaction of existing data AND a write-time filter for future runs.
- **Metric set:** the 11 `pnl`-group ranking columns (see below).

## The 11 metrics (the `pnl` column group, `frontend-react/src/components/sweep/sweepColumns.tsx`)

`score`, `win_rate`, `total_pnl_sol`, `expectancy_sol`, `profit_factor`, `median_pnl_pct`,
`mean_pnl_pct`, `p90_pnl_pct`, `best_pnl_pct`, `worst_pnl_pct`, `std_pnl_pct`
— all present on `ComboMetrics` (`backend/src/sweep/aggregate.rs`); `score` + `profit_factor` are
`Option<f64>`, the rest `f64`.

## Retention algorithm (the shared core)

New module **`backend/src/sweep/retention.rs`** — one pure function, reused write-time and at compaction
so both agree exactly:

```
fn retained_combo_ids(metrics: &[ComboMetrics], best_combo_id: u32, cfg: &RetentionCfg) -> HashSet<u32>
```

Per group:
1. **Eligibility:** a combo contributes to a metric's value set only if `n_closed >= cfg.min_closed`
   (=2, matching `score`'s validity rule) and that metric's value is `Some`/finite (skip `None`/`NaN`).
   This drops the ~33% `n_closed < 2` junk from *every* metric's extremes (fixes the 110k-bucket case).
2. For each of the 11 metrics: collect the **top `cfg.top_n` (=3)** and **bottom `cfg.bottom_n` (=3)**
   *distinct* values among eligible combos.
3. For each selected value, keep at most **`cfg.cap_per_value` (=10)** combos that hold it, tie-broken by
   `score` desc then `combo_id` asc (deterministic). This is the bound: worst case
   `11 × 6 × 10 = 660` rows/group, vs 186k–331k today (~99.7% cut); real count is lower after the
   cross-metric union dedups.
4. **Always include `best_combo_id`** so the group summary (`_groups.best_combo_id` / `best_params`)
   stays joinable.

`RetentionCfg { top_n:3, bottom_n:3, cap_per_value:10, min_closed:2 }` as a `Default` const (tunable in
one place). Add a unit test (`cargo test --bin backend`) covering: tie cap, NULL/under-sampled exclusion,
best-combo always kept, a flat-distribution group (all same `win_rate`) staying bounded.

## Phase 1 — Apply pending migrations (lossless)

Restart `cargo run -p backend` (or run migrations) so `0006` + `0007` apply.
- **Disk caveat:** `0007`'s `ALTER` rewrites the 29 GB table under `ACCESS EXCLUSIVE`, transiently needing
  ~29 GB free. **If disk is tight, run Phase 3 (compaction) BEFORE restarting** — deleting ~95% of rows
  on the wide table first makes the `0007` rewrite cheap. (Compaction reads only the 11 metric columns +
  `n_closed`, which exist in both schemas, so it works either way. This reorder still honors "both happen".)
- Verify: `_sqlx_migrations` shows versions 6, 7 `success=true`; `_combos` table exists; `_results`
  `params` column gone; size dropped.

## Phase 2 — Write-time retention (future runs) — ✅ IMPLEMENTED

`group_to_write` (`backend/src/api/handlers/strategies/grouped_sweep.rs`) now computes
`retention::retained_combo_ids(&g.metrics, g.best_combo_id, &RetentionCfg::default())` and filters
`g.metrics` before `.map(metrics_to_result).collect()`. `fired_count` is still read before the filter.
Shared core lives in `backend/src/sweep/retention.rs` (registered in `sweep/mod.rs`); cap-sort carries
each combo's score so it stays O(n·log n) on 331k-combo groups. 4 unit tests cover tie-cap, NULL/
under-sampled exclusion, best-combo-always-kept, flat-distribution bound (validated via standalone
`rustc` run — the `--bin backend` test target is **pre-existing broken**: 23 unrelated compile errors
re `Corpus.has_fingerprints` / `real.rs`, not from this work).

## Phase 3 — Compaction of existing 26 M rows — ✅ IMPLEMENTED, ⏳ NOT YET RUN

`probe compact-sweeps [tpsl1|tpsl2]` (no arg = both) — `main.rs::run_compact_sweeps`, DB-only
(dispatched before trader/ingest init). Per group: `fetch_combo_metrics_for_group` → `retained_combo_ids`
→ `delete_combos_except(group_id, &keep)`; then `vacuum_full_results()` (`VACUUM (FULL, ANALYZE)`,
`ACCESS EXCLUSIVE`). Prints per-strategy before→after row totals, max kept/group, and flags any group
exceeding the 660 bound. Repo methods added to `grouped_sweep_repo.rs`.
- **REMAINING (operational):** after migrations finish, run `cargo run -p backend -- probe compact-sweeps`
  during the offline window, then confirm sizes (Verification §3).

## Phase 4 — Docs — ✅ DONE

- `@docs/sweep.md`: added `retention.rs` to the file table, the retention stage in the flow diagram,
  the write-time-vs-compaction invariant, the new repo compaction methods, and the metric-extreme note
  on the results row.
- `@docs/database.md`: noted the retention behavior on `_results` + the compaction repo methods.

## Read-path note (accepted)

After retention the drill-in combo table shows only metric-extreme survivors. Sorting by a *non*-ranking
column (holding time, exit counts, or a `p_*` param) yields a sparse view — accepted per the scope
decision. `best_combo` is always present; per-value PnL color bands and the `_combos` JOIN are unaffected.

## Files

- **new** `backend/src/sweep/retention.rs` (`retained_combo_ids` + `RetentionCfg` + tests)
- `backend/src/api/handlers/strategies/grouped_sweep.rs` (filter in `group_to_write`)
- `backend/src/main.rs` (+ probe module) for `compact-sweeps`
- `backend/src/sweep/mod.rs` (register `retention`)
- `docs/sweep.md`, `docs/database.md`

## Verification

1. `cargo check --bin backend` clean; `cargo test --bin backend` (new retention tests pass);
   `cargo clippy` on touched files.
2. Compaction dry-run sanity: on one run, compare `SELECT count(*)` per group before vs after; confirm
   `best_combo_id` rows survive and counts ≤ the `660`/group bound.
3. Sizes: `pg_total_relation_size('tpsl2_grouped_sweep_results')` before/after each phase
   (expect 29 GB → ~4–5 GB after `0007` → hundreds of MB after retention + `VACUUM FULL`).
4. Frontend: `npm run build` clean; load the Grouped Sweep page — groups list, drill-in combo table,
   default `score` sort, and best-combo row all render; no extra re-render on live ticks.
