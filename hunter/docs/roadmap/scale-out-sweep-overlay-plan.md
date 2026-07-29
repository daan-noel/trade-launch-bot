# Scale-out Pass 2 overlay (grouped-sweep)

> Status: SHIPPED (07-29), v2 dynamic grid (07-29). Extends
> `partial-exits-plan.md` §5 without making `scale_out` a swept axis — axes
> stay fast; only top-K pay the staged scalar path.

## Problem

Grouped-sweep executes scale-out (`resolve_exit_staged`) but never generates it:
`AxesModel::assemble` only builds TP/SL/metric conditions. Putting a ladder on
every combo would kill `fast_exit` (index/SIMD) for the whole grid.

## Design (v2 — dynamic per-combo grid)

v1 shipped a single FIXED ladder applied uniformly to every top-K combo. v2
replaces the ladder with a small **grid** of candidate ladders and searches it
independently per combo, including "no ladder" (the combo's own Pass-1 exit)
as a candidate — so a combo the grid doesn't help is never made worse:

```text
Pass 1 (cheap):  axes grid as today → rank on fast_exit
Pass 2 (dynamic): each group's top-K combos independently re-scored against
                  EVERY candidate ladder in the grid PLUS their own baseline
                  exit → each combo keeps whichever wins for IT → re-crown group
Promote / drill-in: read params as-is — the winning ladder (if any) is already
                     baked into that specific combo's own params, no run-wide
                     merge at read time.
```

- Run-level fields: `scale_out` (JSONB `ExitStage[][]` — one array per
  candidate ladder) + `scale_out_top_k` (int). Mig `0013_sweep_scale_out_overlay.sql`
  (column shape unchanged from v1 — JSONB — only its *content* is now a grid).
  `scale_out` on the run row is the **search space**, not what any one combo
  ended up with — read that combo's own `params.scale_out`.
- `_combos.params` / group `best_params` carry each combo's own winning ladder
  (or none) directly — merged in once, at write time, by
  `grouped_engine::retained_combo_params` from `GroupResult::scale_out_winners`
  (`combo_id -> ExitStage[]`, populated only for combos where a candidate beat
  that combo's own baseline). No separate scale_out merge step exists anywhere
  downstream (promote, drill-in, group summary) — the persisted `params` column
  is already the final truth.
- Winner selection: `GenericSweepStrategy::post_group_rescore` scans each
  candidate ladder for each top-K combo, and adopts the best of
  `{baseline, variant_0, variant_1, ...}` using the same `rank_combo` ordering
  `best_combo`/`top_combo_ids` use elsewhere (checklist score, then fired
  count, then marked PnL) — factored into the pure, unit-tested
  `pass2_candidate_wins` helper.
- Cost: `variants.len() + 1` staged scans per top-K combo (bounded — never a
  swept axis over the whole grid).
- Default FE presets (any subset checkable): bank 50/70/85% at +30/50/80% TP,
  remainder `held >= 20/30/45`.
- v1 stage conditions: `m_position` / `take_profit` only (axes precompute
  columns) — unchanged in v2, validated per-ladder in the grid.

## Non-goals

- Sweeping stage counts / sell_bps as axes.
- Stamping scale_out onto every combo of Pass 1.
- Parallel baseline/staged metric columns (Pass-2 winner overwrites the
  combo's `ComboMetrics` row in place; the pre-overwrite Pass-1 numbers are not
  separately retained).
- A continuous/adaptive trail (`trail_pct = f(pnl)`) — the grid stays a small,
  fixed set of discrete ladders; if a combo needs something between two
  presets, add a preset, don't parameterize the shape.

## Done criteria

- `cargo check -p hunter-lab` clean; Pass-2 hook before `group_done`. ✓
- Unit tests: grid parsing (`ExitStage[][]`, rejects a flat single ladder) +
  the dynamic winner-selection logic (`pass2_*` tests in `generic/strategy.rs`)
  — keeps baseline when nothing beats it, adopts a beating candidate, picks the
  single best of several (not the first improvement). ✓
- FE: per-preset checkboxes + top-K on sweep form; re-run restores checked
  state from the run's stored grid. ✓
- Promote of a Pass-2 run opens RuleEditor with the PROMOTED COMBO'S OWN
  winning `scale_out` attached (not a run-wide one). ✓
