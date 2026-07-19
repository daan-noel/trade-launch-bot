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

## Sizing preference vs allocation invariant (do not conflate)

Two different things bound a fold batch, and mixing them up aborted runs mid-flight:

| | Value | Time-varying? | May be used for |
| --- | --- | --- | --- |
| `registry::preferred_max_combo_batch()` | 65536, or **8192** under the desktop reserve | **yes** — live free-RAM read | sizing the *next* allocation |
| `engine::HARD_MAX_COMBO_BATCH` | 65536 | no — a `const` | asserting an *existing* allocation |

The rule: **a live reading may size an allocation, never validate one.**

`fold_wave_into` and the pass-outer loop assert against the constant. They used to
assert against the live function, which is a TOCTOU: a batch legally sized at 65536
while RAM was free got retroactively declared illegal the instant free RAM dipped
under the reserve, and the fold bailed —

```
fold_wave_into: n_combos 65536 > hard_max_combo_batch 8192   groups_done=48
```

— throwing away 48 folded groups to "save" memory that was already allocated and
about to be freed. `preferred_batch_never_exceeds_static_invariant` (engine tests)
pins the relation that makes a pinned batch safe: any batch is
`≤ preferred_max_combo_batch() ≤ HARD_MAX_COMBO_BATCH` at the moment it is sized,
so it still clears the guard later however far RAM has since moved.

Batches are also **pinned per shard/group**, read once rather than per pass, so the
`aggs` vec and every combo chunk derived from it agree on one width. Degradation
lands on the next group — which is exactly where it can still be acted on.

## Per-group failure isolation

A group that cannot fold costs the run *that group*, not the sweep. `run_grouped_sweep`
catches non-cancel errors per group in both driver phases, reports them through one
`note_group_failure` SSOT (log + `GroupSink::group_failed` + an operator notice, capped
at 3 toasts), and carries on; the survivor vec is filtered rather than indexed.
Cancellation still aborts — it is not a group failure.

The run then finalizes **`partial` with a reason**, never `completed` over a thinner
group set: `HandlerSink::group_failed` → `SweepWrite::GroupFailed` → the writer task's
`groups_failed` count → the handler's terminal status. A silent drop would be worse
than the abort this replaced.

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

---

# Measured performance (2026-07-19)

The smoke runs that gated the P1–P3 perf backlog, and what they decided. Numbers
from `hunter-lab` **release**, branch `strategy-redesign`, workstation: 16 logical
cores, 15.7 GB RAM, lake = 8 sealed days.

Run config (both runs identical except `ram_reserve_mb`):

```
strategy_id  generic          method       refine:20000:3
group_by     all 7 fingerprint fields      token_cap    60000
min_tokens   10               max_combos   1000000      buy_amount_sol 0.01
```

Corpus: **45,380 tokens · 4,058,935 trades → 405 groups · 28,389 surviving combos.**

## Run A — normal (`ram_reserve_mb: 1024`)

`status=completed`, **405/405 groups persisted**, 78.6 s.

| stage | secs | share |
| --- | --- | --- |
| `corpus_load` | 4.58 | 5.8% |
| `partition` (×2) | 0.16 | 0.2% |
| `refine_coarse_pass` | 25.49 | 32.4% |
| `refine_final_pass` | 47.22 | 60.1% |
| `writer_drain` | **0.0000117** | ~0% |

`slowest_group_secs=2.82` (2062 tokens) out of a 47.1 s fold — **no single group
dominates the tail**. Throughput `evals_per_sec` **peaks at 27.0M and decays to
20.0M** across the coarse pass.

## Run B — RAM-starved (`ram_reserve_mb: 5000`, ~560 MB usable)

`status=completed`, **405/405 groups persisted**, 513.6 s (**6.5× slower**).

The degradation ladder fired and was reported, exactly as the ladder above
specifies:

```
WARN generic sweep: degraded sizing
     note=fold buffers capped at 480 MB (of 512 MB) — smaller combo batches, more series passes
```

`budget_mb` 3358 → 471. **No refusal, no abort** — the ladder cost wall-clock, not
the run. This is the path the original `65536 > 8192` mid-run abort came from; it is
now genuinely degrade-don't-refuse.

`slowest_group_secs=229.5` at `slowest_group_index=60` on a **62-token** group — 49%
of that run's 467 s coarse pass spent on one tiny group. See the finding below.

## Verdicts on the perf backlog

| Item | Verdict | Evidence |
| --- | --- | --- |
| **P2** redundant `insert_combos_indexed` sends | **Skip — do not do** | `writer_drain` is **11.7 µs** (Run A) / **4.1 µs** (Run B). The writer is fully drained before the fold ends; the redundant sends cost nothing measurable. The unbounded fold→writer channel also never backed up (RSS peaks track fold buffers, not queue depth), so the stability concern is unrealised too. |
| **P3** refine sweeps the corpus twice | **Keep as designed** | The coarse pass is 32% of Run A, but it is what cuts 944,784 grid combos to 28,389 for the final pass. It is the algorithm, not waste. The *corpus* is loaded once (`corpus_load` 4.58 s, single); only the series are rebuilt. |
| **P1** `fold_wave_into` per-wave concurrency churn | **Skip — hypothesis refuted** | The fold *is* the constraint (92.5% of Run A), so the gate opens — but the cost is not where P1 said. Per-wave thread/channel/mutex setup is a few thousand spawns over 47 s (sub-1%). The measured hot spot is series-rebuild amplification (below). The plan's own gate says skip P1 if the timings don't support it; they don't. |

## Finding → fix: the sweep starved itself onto the slow path

### The finding

**The documented "primary path" never executed.** Across 405 groups in **both**
original runs, the small-group **token-outer** fold (series built once per token) was
selected **0 times**; the **batch-outer fallback** (series rebuilt once per batch)
fired **314 times**, at `n_batches=3` — every token's `MetricSeries` built three times.

The selector is `full_combo_aggs_fit(…) <= usable_host_bytes() − 256 MB`, and the
instrumented inputs said why:

```
group_tokens=11 combos=19807 n_batches=3 threads=14
max_series_kb=174  agg_mb=294  series_mb=2  fold_budget_mb=32  usable_mb=0
```

`usable_host_bytes()` was `host_available − reserve`. Mid-run, **the sweep's own RSS
is what consumed `host_available`** (6.3 GB peak in Run A), so it measured ≈0 usable
and concluded it had no room — while the accumulator set it was rejecting needs only
**294 MB on a 15.7 GB box**. A feedback loop: allocate → free RAM drops → `usable → 0`
→ pick the path that rebuilds series per batch → 3× the dominant cost. It is also why
Run B's 62-token group took 229 s: large-history tokens have ~1 s series, and the
fallback rebuilt each `n_batches` times.

### The fix (shipped)

**Price the sweep's own *transient* buffers as reusable headroom, keep the corpus
priced as consumed.** `usable_host_bytes()` now takes the **max** of two readings:

- *live* — `available − reserve` (honest about external pressure), and
- *structural* — `total − reserve − resident_baseline`, where `resident_baseline` is
  process RSS captured **once at `corpus_loaded`** (corpus fully resident, no fold
  buffer yet — the run's permanent, non-reclaimable floor).

The structural term does not decay as the sweep fills it, because the sweep's own
fold buffers are exactly what that budget is *for* — they are no longer subtracted
from the sweep's own headroom. It stays abort-safe by construction:
`baseline + usable ≤ total − reserve`, so the desktop reserve is always left free
(the never-OOM contract). Before the baseline is captured (admission, pre-load) the
structural term is 0, so admission sizing is byte-for-byte the old behaviour.

`registry::usable_from` (the pure `max(live, structural)` core) is unit-tested for
the never-exceed bound, the starvation case, external-pressure preference, and the
genuinely-full box. `set_resident_baseline_bytes` is set at `corpus_loaded` and
cleared at admission.

### Measured effect

Same config, re-run after the fix (`ram_reserve_mb: 1024` = normal;
`ram_reserve_mb: 5000` = tight, forces "minimum footprint"):

| | fold path | `refine_coarse` | slowest group | total | outcome |
| --- | --- | --- | --- | --- | --- |
| Normal, **before** | 0 token / 314 batch | 25.5 s | 2.8 s / 2062 tok | 78.6 s | completed |
| Normal, **after** | **152 token / 0 batch** | 26.0 s | 1.6 s / 2062 tok | 76.4 s | completed |
| Tight, **before** | 0 token / 314 batch | **467.7 s** | **229.5 s / 62 tok** | 513.6 s | completed |
| Tight, **after** | **601 token / 0 batch** | **58.9 s** | 8.9 s / 2062 tok | ~115 s | completed |

- **Normal run: ~flat wall-clock** (76.4 vs 78.6 s). The batch-outer waste was on
  *tiny* groups with cheap series, so eliminating it barely moves the normal-run
  total — but the documented path now actually runs and the provably-repeated series
  builds are gone.
- **Tight-reserve run: 7.9× faster coarse pass, and the 229 s pathology is gone** —
  the slowest group is now the genuinely-largest (2062 tokens), not a 62-token group
  paying `n_batches` series rebuilds. It still completed 405/405 at minimum footprint
  (`budget_mb=0`, wave=1): **degrade-don't-refuse held**; the structural budget
  (16101 − 5000 − ~1036 ≈ 10 GB) let token-outer fire while live `available − reserve`
  was 0.

The diagnostic fields (`agg_mb` / `series_mb` / `fold_budget_mb` / `usable_mb` on the
fallback log line) that made this visible are kept.

### Observed during validation (not this fix)

One tight-reserve run's row briefly read `status=cancelled, groups_done=5` while the
engine was still folding, then finalized `completed` 405/405. The engine folded to
completion (the cancel `AtomicBool` was effectively false throughout — a genuine
cancel would have bailed the fold), so this is a **status-write race**, not lost work,
and it is orthogonal to the RAM fix (a live lab UI was open and polling). Worth a
look on its own: a spurious `cancelled` write that finalize later overwrites is
confusing even when harmless.
