# Grouped sweep — visibility, stability, speed plan (2026-07-17)

> **STATUS: IMPLEMENTED 2026-07-17** on branch `strategy-redesign` (NOT committed).
> P0 (SweepGroupDone SSE + corpus phase + saving-phase removal + live groups
> refetch), P1 (honest `partial` finalize; the boot reaper already existed), and
> P2 (token-outer small-group fold) all landed. Backend `cargo check`/clippy/tests
> green (95 sweep tests + new `sweep_group_serial_is_batch_invariant`); frontend
> `build:lab` + `lint` clean. Runtime smoke over a real multi-group sweep still
> pending. Sections below are the original design; deviations noted inline.

Goals (user): **speed and stability**, plus two UX gaps — (1) the progress bar
doesn't show what the sweep is actually doing, (2) group results are invisible
until the whole sweep finishes.

Diagnosis: the engine architecture (two-phase driver, RAM-timed batching,
shard+spill, admission guard, incremental group persistence) is the right shape
and stays. Both UX gaps are wiring, not engine design; the one real speed lever
left is redundant series rebuilds on multi-batch folds.

## Root causes found

1. **Groups invisible mid-run** — backend already persists each group on
   `sink.group_done` (writer task → `append_group`), but the frontend never
   refetches: `getGroupedSweepGroups` (labEndpoints.ts) has no `providesTags`
   and no polling, and there is no per-group SSE frame. The data is in the DB
   the whole time; nothing tells the UI to look.
2. **Progress bar confusion — phase flip-flop bug.** The writer emits
   `phase:"saving"` frames concurrently with the engine's `phase:"sweep"`
   frames (persistence drains while later groups fold). The frontend phase
   model (BackgroundJobsContext `upsert`) assumes sequential phases: any frame
   from a different phase marks the previous one `done`. Alternating
   sweep/saving frames flip both bars done/active for the whole run.
3. **Corpus load is silent.** The DuckDB lake load (longest pre-fold phase)
   emits no frames — indeterminate bar, no label.
4. **No group context on frames.** `SweepProgress` carries only
   token-equivalent processed/total — never "group g of G".
5. **Stale runs after crash.** `status="running"` is only cleared in-process
   (Gate). A lab-bin crash strands the run as running forever.
6. **Silent partial finalize.** Writer `append_group` errors are only logged;
   a run can finalize `completed` with missing groups.
7. **Multi-batch series rebuild.** `sweep_group_serial` (and the large path's
   pass-outer mode) loop combo-batches outer / tokens inner → every batch
   rebuilds each token's `MetricSeries` (deliberate CPU-for-RAM trade). With
   `n_batches > 1` the dominant cost multiplies.

## P0 — visibility (no engine changes)

- **`SweepGroupDone` SSE frame** `{strategy_id, run_id, group_index,
  groups_done, group_count}` emitted by the handler's writer task after each
  successful `append_group` (a frame is already sent there — extend it).
  Frontend: subscribe in BackgroundJobsContext; when the viewed run matches,
  invalidate a per-run `GroupedSweepGroups` tag (add `providesTags` to
  `getGroupedSweepGroups`). Groups table fills live, largest group first.
- **Remove the "saving" *phase*.** Saving is a concurrent drain, not a stage.
  Show `groups persisted: N/M` as a counter line under the sweep bar instead of
  a phase bar. Phases map becomes strictly sequential: `corpus → coarse →
  sweep`. Fixes the flip-flop with zero backend risk.
- **`phase:"corpus"` frame** sent before the lake load (indeterminate) + one on
  completion, so the longest silent stretch is labeled. Cache-hit runs skip it.
- **Add `groups_done`/`group_count`/`run_id` to `SweepProgress`** so the main
  bar can render sub-status ("group 12/87").

## P1 — stability

- **Boot-time stale-run reaper** — ALREADY EXISTED (`reconcile_orphaned_runs`,
  called per strategy in `lab/main.rs`; marks boot-time `running` rows
  `cancelled`, keeping persisted groups). No change needed.
- **Honest finalize:** compare the writer's persisted-group tally with the
  engine's group count before `finalize_completed`; on mismatch stamp
  `partial` (or `completed_with_errors`) instead of `completed`, and carry the
  first write error onto the run row / terminal frame.

## P2 — speed (measure first)

- **Token-outer fold when all aggs fit** (the deferred perf-plan P3): when
  `full_combo_aggs_fit(n_combos, …)` — true for almost all real runs; ComboAgg
  is small, the series is the heavy part — swap `sweep_group_serial` (and
  pass-outer) to tokens-outer / combo-batches-inner so each token's series is
  built **exactly once** instead of once per batch. Direct `n_batches×` cut of
  series-build cost on multi-batch runs. First instrument
  `series_built / tokens` (one counter + existing obs logging) to confirm the
  multiplier before claiming the win.
- **Refine double-evaluation** (final pass re-sweeps all coarse combos,
  ~2× on `refine` runs): known, but a cross-pass outcome cache is RAM-expensive
  — leave unless measurements say otherwise.

## Explicit non-goals

- No fold-time full partition (holds every group's combos × ComboAgg resident —
  the tens-of-GB trap the two-phase driver exists to avoid).
- No RAM budget/cap raises; admission guard and spill stay as-is.
- No change to the two-phase large/small group routing.
