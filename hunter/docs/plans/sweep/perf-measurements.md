# Grouped sweep — measured performance (2026-07-19)

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

The degradation ladder fired and was reported, exactly as
[ram-sizing.md](ram-sizing.md) specifies:

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
