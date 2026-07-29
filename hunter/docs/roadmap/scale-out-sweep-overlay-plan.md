# Scale-out Pass 2 overlay (grouped-sweep)

> Status: SHIPPED (07-29). Extends `partial-exits-plan.md` §5 without making
> `scale_out` a swept axis — axes stay fast; only top-K pay the staged scalar path.

## Problem

Grouped-sweep executes scale-out (`resolve_exit_staged`) but never generates it:
`AxesModel::assemble` only builds TP/SL/metric conditions. Putting a ladder on
every combo would kill `fast_exit` (index/SIMD) for the whole grid.

## Design

```text
Pass 1 (cheap):  axes grid as today → rank on fast_exit
Pass 2 (selective): each group's top-K combos re-scanned with a FIXED
                    scale_out ladder → metrics replaced → re-crown best
Promote / drill-in: merge run.scale_out into params (SSOT on the run row)
```

- Run-level fields: `scale_out` (JSONB ExitStage[]) + `scale_out_top_k` (int).
  Mig `0013_sweep_scale_out_overlay.sql`.
- `_combos.params` stay baseline (no ladder) — shared across groups.
- `best_params` + promote draft + drill-in simulate merge the run ladder when present.
- Default FE preset: bank 70% @ +50% TP, remainder `held >= 30`.
- v1 stage conditions: `m_position` / `take_profit` only (axes precompute columns).

## Non-goals

- Sweeping stage counts / sell_bps as axes.
- Stamping scale_out onto every combo of Pass 1.
- Parallel baseline/staged metric columns (v1 ranks on Pass-2 scores only).

## Done criteria

- `cargo check -p hunter-lab` clean; Pass-2 hook before `group_done`.
- FE: Pass-2 checkbox + top-K on sweep form; re-run restores from run row.
- Promote of a Pass-2 run opens RuleEditor with `scale_out` attached.
