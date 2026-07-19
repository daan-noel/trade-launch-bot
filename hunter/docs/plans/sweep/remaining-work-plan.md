# Grouped sweep — remaining work

Status: **2026-07-19**, branch `strategy-redesign`.
Goal: grouped sweep as **fast as possible** within local RAM, **never aborted** by a
recoverable error, with **detailed running status**.

The documentation and UI items from this round are **done** (`b392ac10`). The
divergence record, the correctness backlog and the ranked perf backlog now live
permanently in [sim-parity.md](sim-parity.md). What remains is the part that needs a
machine and a corpus:

---

## Two smoke runs — nothing else should start before these

Neither has been run. Both are cheap and both gate real decisions.

### 1. One normal grouped-sweep run

Confirms the round's correctness work, and produces the numbers that decide whether the
P1–P3 perf items are worth doing at all. Capture from the run's log:

| Field | Decides |
| --- | --- |
| `refine_coarse_pass` / `refine_final_pass` secs | whether P3 (double corpus sweep) matters |
| `writer_drain` secs | whether P2 (redundant row sends) matters |
| `slowest_group_secs` / `_index` / `_tokens` | whether one group dominates the tail |
| `corpus_load` secs | whether load, not fold, is the constraint |
| `evals_per_sec` (debug) | the baseline any P1 change must beat |

Also confirm: groups stream in mid-run, a `partial` finalize reports honest counts, and
the drill-in table agrees with its stored row.

### 2. One low-RAM run

Exercises the degradation ladder that replaced the admit-or-refuse guard — the path the
original `65536 > 8192` abort came from. Fill RAM so the host sits under the reserve,
then start a run.

Expect smaller batches / fewer threads, a `SweepNotice`, **and completion**. A refusal
or an abort here is a bug. See [ram-sizing.md](ram-sizing.md).

---

Once both runs are in, pick up the perf backlog (P1–P3) from
[sim-parity.md](sim-parity.md) — and skip any item the timings show is not the
constraint.
