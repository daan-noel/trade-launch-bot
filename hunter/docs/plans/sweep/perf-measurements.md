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

## Finding: the sweep starves itself onto the slow path

**The documented "primary path" never executes.** Across 405 groups in **both** runs,
the small-group **token-outer** fold (series built once per token) was selected
**0 times**; the **batch-outer fallback** (series rebuilt once per batch) fired **314
times**, at `n_batches=3` — i.e. every token's `MetricSeries` is built three times.

The selector is `full_combo_aggs_fit(…) <= usable_host_bytes() − 256 MB`, and the
instrumented inputs say why:

```
group_tokens=11 combos=19807 n_batches=3 threads=14
max_series_kb=174  agg_mb=294  series_mb=2  fold_budget_mb=32  usable_mb=0
```

`usable_host_bytes()` is `host_available − reserve`. Mid-run, **the sweep's own RSS
is what consumed `host_available`** (6.3 GB peak in Run A), so it measures ≈0 usable
and concludes it has no room — while the accumulator set it is rejecting needs only
**294 MB on a 15.7 GB box** with a 1 GB reserve. `fold_budget_mb` collapses to its
32 MB floor for the same reason.

It is a feedback loop: allocate → free RAM drops → `usable → 0` → pick the path that
rebuilds series per batch → 3× the dominant cost. It also explains the 27M→20M
`evals_per_sec` decay (the decay tracks RSS growth) and why Run B's 62-token group
took 229 s: those tokens have large histories, so each rebuilt series is ~1 s and the
fallback pays it `n_batches` times.

**Not fixed here, deliberately.** The fix means changing what RAM the admission
logic considers reusable — the process's own already-allocated heap. That is the
exact logic whose failure mode is a mid-run abort, and "never aborted by a
recoverable error" is the standing requirement for this subsystem. It needs a
decision, not a drive-by patch. Options worth weighing:

1. Price the sweep's own resident buffers as reusable headroom (`available + own
   reclaimable RSS`) rather than treating them as consumed by a third party.
2. Decide the fold order **once per run at admission** (when headroom is real)
   instead of per group mid-run, so the choice is not a function of the sweep's own
   momentary RSS.
3. Stop double-counting in `full_combo_aggs_fit`: in the token-outer path the `aggs`
   *are* the fold buffers, yet `need = agg + series + fold_budget` adds a full
   separate fold budget on top.

Option 2 is the most conservative — it changes *when* the decision is made, not how
much RAM is considered safe.

The diagnostic fields above (`agg_mb` / `series_mb` / `fold_budget_mb` / `usable_mb`
on the fallback log line) were added to make this visible; keep them.
