# Grouped sweep — RAM sizing (degrade, don't refuse)

Deep-dive reference for how a grouped sweep sizes itself against free host RAM.
Structure/summary lives in [../../arch/sweep.md](../../arch/sweep.md).

## The problem this replaced

The engine used to **admit or refuse**: it computed a *preferred* plan (full thread
pool, full fold budget), estimated its resident peak, and `bail!`ed if that peak
didn't fit `usable = host_free − desktop_reserve − slack`.

Three things made that wrong:

1. **The peak is a choice, not a constant.** Threads, the series wave, the fold
   batch, the shard width and disk spill are all tunable downward, and the engine
   already had every one of those knobs. Refusing meant "your preferred plan
   doesn't fit" but *reported* it as "this run is impossible".
2. **The run had already appeared to start.** `POST /api/strategies/sweeps` returns
   `202` immediately and the fold runs detached, so the refusal landed *after* the
   corpus load — minutes in, looking like a mid-run abort.
3. **The refusal deleted the run row**, discarding any groups that had committed.

Net effect: a multi-minute analysis job died because a browser was open, and took
its partial results with it.

## The ladder

`registry::plan_sweep_sizing(preferred_threads, max_series_bytes, planned_combos)`
returns a `SweepPlan { threads, wave, fold_budget, notes }`.

```
budget          = usable − SWEEP_ALLOC_SLACK_BYTES
combo_floor     = MIN_SHARD_COMBOS (8192) × GENERIC_PER_COMBO_RESIDENT_BYTES
for_series_fold = budget − combo_floor

floor_need = max_series_bytes + fold_floor      ← 1 thread, 1 token, min batch
if floor_need > for_series_fold  →  refuse (the ONLY refusal)

rung 1: threads = largest t ≤ preferred with t × series ≤ for_series_fold − fold_floor
rung 2: fold    = (for_series_fold − threads × series) clamped to floor..=cap
```

Two properties make this sound:

- **No circularity.** The combo side is priced at `MIN_SHARD_COMBOS`, its
  *shardable floor* — `shard::plan_shards` can always cut the combo space to that
  width regardless of how many combos were requested — so the combo term is a
  constant here rather than a function of the fold budget being solved for.
- **Monotone.** Each rung only lowers the peak, so the first fit found is the
  largest plan that fits.

### The one remaining refusal

A true-floor overflow: one thread, one token's series, one minimum fold batch and
one minimum shard still exceed usable RAM. That is a genuine
"this-machine-cannot-do-this" — one token's `MetricSeries` is irreducible, since
the token has to be resident to be scanned. Its message names the floor and the
levers that can actually move it (desktop reserve, `token_cap`, date range), not
generic "narrow your axes" advice that wouldn't help.

## Ceiling, not pin — how mid-run adaptation survives

The planner installs its chosen fold budget in `FOLD_BUDGET_CEILING`, and
`sweep_memory_budget_bytes()` applies it as `min(live_sizing, ceiling).max(floor)`.

This matters because that function is re-read **on every call** by
`combo_batch_size`, `max_combos_per_shard`, `max_parallel_shards` and
`full_combo_aggs_fit`. So:

- the plan **bounds** the peak (the run never grows past what was admitted), and
- live sizing still **shrinks below** it when free RAM drops after admission.

That closes the old "free RAM dropped between admission and folding — nothing
re-checks it" hole without needing a mid-run abort.

**Not adapted mid-run:** the rayon pool size. Rebuilding the pool mid-fold would
mean tearing down in-flight work, which costs more than the RAM it reclaims. A run
that started wide stays wide on threads and narrows on batches/shards/waves
instead. This is a deliberate limit, not an oversight.

## Reporting

Degradation is never silent — an unexplained 4× slowdown reads as a hang.

`SweepObserver::notice(&str)` (default no-op, so test/replay observers ignore it)
→ `SweepProgress::notice` → `SseEvent::SweepNotice` → `sweep_notice` SSE frame →
an **info** toast in `BackgroundJobsContext`. Cold path: a handful of calls at
sweep start, never from the fold loop.

Notes are emitted for: reduced thread count (with the expected slowdown factor), a
capped fold budget, running under the desktop reserve, and — new — **host RAM
being unreadable** (non-Windows/Linux), where the guard is inert and sizing falls
back to the flat `DEFAULT_SWEEP_ADMISSION_BUDGET_MB`. That last case used to be
silent.

## Failure persistence

`run_grouped_sweep_job`'s error branch calls `repo.mark_status` instead of
`delete_run`:

| Condition | Status | Why |
| --- | --- | --- |
| `groups_done > 0` | `partial` | Groups stream to the DB as they fold, so they are real, correct, and queryable — the same honest status a short-write run gets |
| `groups_done == 0` | `failed` | Config error (bad axes / over-cap grid) or a floor-overflow refusal; nothing folded, but the attempt stays inspectable |

The reason still rides the terminal `SweepFinished` frame, so the client toasts it
— but now it describes a run the user can still open rather than one that vanished.

## Why the desktop reserve dropped 2 GB → 1 GB

With the ladder in place the reserve is a *preference* (how much headroom you want
for the desktop) rather than a *cliff* (what makes the run fail). The old 2 GB
default was the single biggest source of spurious refusals on a workstation with a
browser open. Undershooting it now costs wall-clock, not the run.
