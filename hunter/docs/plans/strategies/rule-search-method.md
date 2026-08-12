# Rule search method - deriving the best rule for a fingerprint

The method for turning a fingerprint into the best rule it supports - which metrics, which
sides, which values - plus a computed verdict on whether that rule is worth trading. This
file is the method itself; the automated driver implements it, and the pilot protocol at
the bottom validates it by hand before any driver code exists.

The facts this method reads from live where they are already single-sourced: metric
semantics and the traps that silently produce wrong rules in
[metrics-reference.md](metrics-reference.md), the sweep engine and its performance
constraints in [../../arch/sweep.md](../../arch/sweep.md), and the metric registry itself in
`hunter_engine::metrics::REGISTRY`. Per-cohort evidence for the mechanisms below is
[../../history/2026-08-12-rule-search-worked-examples.md](../../history/2026-08-12-rule-search-worked-examples.md).

## The three failure modes the method is built against

1. **Overfitting by selection.** Testing hundreds of combos and keeping the best inflates
   the winner's number even when every simulation is honest. The inflation is a property of
   selection, so the only defense is scoring decisions on data the selection cannot reach.
2. **Interaction blindness.** A condition's value depends on the rest of the rule: the same
   screen loses under one entry and pays +0.28 SOL under another (g3, playbook). Any method
   that scores metrics once, in isolation, gets this wrong.
3. **Luck illiteracy.** On a right-tail cohort one token can carry a +0.3 SOL marginal.
   Without a measured luck floor, every keep/drop decision is a coin flip dressed as a
   measurement.

## Step 0 - cut the data before looking at it

- Split the cohort span into **K = 4 contiguous time folds** by token creation time, plus a
  **final holdout: the most recent ~7 days**. No search decision ever reads the holdout.
- Regime check: day-split drift in median curve life and median peak liquidity. A regime
  break (fp 0.0432 style: lifetimes 260-1360 s collapsing to 0-17 s) shrinks the usable
  window to the current regime before anything else runs.
- Cohort-size gate: under ~100 matched tokens every downstream number is directional only,
  and the run says so in its verdict.

A broken or thin cohort stops here with a verdict, in under a minute.

## Step 1 - candidate pool: the whole catalog, valued by the cohort's own percentiles

Every metric in `hunter_engine::metrics::REGISTRY` is a candidate: both sides (except
`m_position`, exit-only), both directions where two-sided, one candidate per window in
`{2, 3, 5, 10, 30}` s for the dynamic groups, `take_profit`/`stop_loss` against unreachable
values, plus every condition in the seed rule including its `params.disabled` block.

Values come from the cohort's own percentile ladder (p10/p25/p50/p75/p90), never global
anchors. During selection each candidate carries only 2-3 values - enough to detect *that*
a metric matters; *where exactly* is Step 5's job.

Candidate identity is `(side, group, metric, window)` with the operator excluded, so a low
arm and a high arm on one metric form a **band** (a 2-D region candidate), never two
independent candidates that can select a contradiction.

## Step 2 - split-objective staged search with alternation

| Stage | Exit in place | Scores on | Why |
| --- | --- | --- | --- |
| Entry | neutral (no TP, no SL, no exit conditions) | **per-trade edge**, with a fire floor scaled to cohort size | an entry can only remove trades; total SOL punishes every filter and converges on exit-only rules |
| Exit | the winning entry | **total SOL including the open mark** | exits face no removal penalty, and closed-only PnL crowns configs that never exit |

Then **alternate**: re-run the entry search under the winning exit, then exits again, until
the selected set stops changing, hard-capped at 3 passes. Alternation is the answer to
interaction: each condition's final measurement is taken in the presence of the rule it
actually lives in.

Fire floor: `max(20, 5% of fitting tokens)` fired trades. Per-trade edge with no floor
rewards a rule that fires five times.

## Step 3 - the decision test (the heart of the method)

A candidate is added, or a selected condition survives a drop probe, only when its marginal
(score with it vs without it, everything else held fixed) passes **all three**:

1. **Positive overall** on the fitting window.
2. **Sign-consistent across folds**: positive in at least 3 of the 4 folds. A condition
   that pays in one time slice is fitting that slice.
3. **Clears the luck floor**:
   - *Entry candidates* select tokens, so the null is "which tokens is irrelevant": draw
     random token subsets of the same size as the candidate's fired set, compute each
     draw's marginal, and require the observed marginal above the null's p95.
   - *Exit candidates* change per-token outcomes, so the test is a paired bootstrap over
     per-token deltas (outcome with vs without), requiring the bootstrap CI of the mean
     delta to exclude zero.

   Both resample outcomes already computed; nothing is re-simulated. On a whale-tailed
   cohort the floor is high, and that is exactly the information no unassisted reading of
   a marginal carries.

Robustness sits inside every decision, not in a final exam, so the search cannot wander
into an overfit region and the acceptance step confirms rather than filters. Add gating is
relative, not absolute: a round accepts the best add only if its marginal also clears a
fraction (~25%) of the round's strongest marginal, which stops the "eleven conditions on a
big cohort" failure an absolute floor causes.

## Step 4 - coverage repair: what greedy search misses

- **Pairs that only pay together**: after the loop stabilizes, pin the strongest selected
  condition and re-screen the rejected near-misses under it (bounded synergy rescue).
- **Path dependence**: run the search twice - from the empty set and from the seed rule's
  conditions - and keep the better result. Agreement is free confirmation; disagreement is
  itself a finding about the cohort.

## Step 5 - value tuning: plateau AND fold-consistency, survivors only

Fine-grid only the surviving 3-6 conditions, several values each, bands refined in 2-D.
A value is accepted when:

- its neighbours score similarly (**plateau** - a lone peak is a fitted accident), and
- it holds across folds (**time** - the check plateau cannot make: `nonvol_net` 0.45 sits
  on a smooth in-sample plateau and still goes negative out of sample; 0.5 holds).

Adjacent values scoring identically means the condition rarely binds: drop it and keep the
rule smaller.

## Step 6 - acceptance: the only quotable numbers

The finished rule, untouched by further tuning, is scored on the **holdout** through
**simulate** (the authority; the sweep only ranks). Four columns on identical data:

| Column | Question it answers |
| --- | --- |
| the new rule | the result |
| the incumbent rule (if any) | is this an improvement |
| the seed rule (if different) | did the search beat its starting point |
| **no rule at all** | do the conditions do anything |

Two simulate runs per finalist, at the live notional:

1. **Authority**: worst fill + `pumpfun_impact`. The only quotable number.
2. **Optimistic**: `first` fill + `fee_only`. The ratio authority/optimistic is the
   **fill-spread penalty** - how much of the edge is an execution bet - and it enters the
   ranking, because the objective is live-replicable SOL, not backtest SOL.

## Step 7 - verdict and shipping

`USE / MARGINAL / DO NOT USE`, computed from thresholds, weakest check deciding:

| Check | Demotes when |
| --- | --- |
| holdout total SOL (authority pricing) | not positive |
| holdout beats no-rule | false |
| **holdout beats the incumbent** | false - a rule can pass every other check and still be worse than what is already running |
| fold sign consistency of the final rule | fewer than 3 of 4 folds positive |
| holdout fired trades | under `max(20, 5% of holdout tokens)` |
| fill-spread ratio | worse than ~4x demotes to MARGINAL; both ends must be positive |

**A fit whose own edge is negative is a refusal, not a rule.** When the best surviving candidate
scores below zero on the fitting window, the output for that period is *no rule*, not the
least-bad combination. An abstained period earns zero, and it is reported as a period, never
dropped - dropping it grades the method only on the periods it felt confident.

Ship via `POST /api/strategy-rules`, `is_active: false`, paper first. Every rejected
candidate is parked under `params.disabled` with the marginal that rejected it, and the
full search report (every decision plus its evidence) is persisted. That report is what
makes the next run cheap.

## Performance rules (the method rations width, never honesty)

Where time goes: corpus load (once, cached), metric precompute (once, shared), and
metric-exit scans (~15x a TP/SL scan - the one cost that scales with search width). Folds
and bootstrap re-slice computed outcomes and are effectively free.

1. **Pay fixed costs once.** One scoped corpus load, one precompute over the union of every
   metric any round touches, selection byte-identical across the whole run.
2. **Cheapest-first ordering.** Regime check (seconds) before search; the entry stage runs
   under the neutral exit, the cheap scan shape; expensive metric-exit scans start only
   after an entry exists.
3. **"No edge" is a fast answer.** Most fingerprints screen out; the rejection path is the
   throughput path. Step 0 kills a broken cohort in under a minute; a first screen round
   where nothing beats the luck floor kills in 3-5 minutes with `DO NOT USE`.
4. **Coarse while deciding, fine for survivors.** 2-3 values per candidate during
   selection; fine grids, 2-D band refinement and plateau mapping only over the 3-6
   survivors; alternation capped at 3 passes.
5. **A time budget narrows width, never honesty.** Under budget pressure the run trims
   value menus, rescue depth and alternation - never folds, the luck floor, or the holdout.
   A faster run explores less; it never lies more.

## Scoring the method itself - walk-forward over the re-tune cadence

Step 6's holdout scores one rule. It cannot say whether *re-tuning on a cadence* is worth
doing, because it never re-fits. That question needs a walk-forward: fit, score the next 7 days
the fit never saw, step, re-fit, repeat, and sum the untouched periods. Compare the refitting
method, the incumbent, and **no rule at all** on identical periods. "No rule" is not an empty
axis list - that yields no combo at all - it is one axis pinned to an unreachable
`take_profit`.

Two results this already produced, both worth keeping:

- **Judge a candidate method by reproduction, not by its backtest.** Run it on a cohort whose
  rule is already rated good and ask whether it recovers that rule's shape. A method that
  cannot recover a rule known to be good is broken however well it backtests.
- **More fit data is not automatically better fit data.** Doubling the fit window does not
  recover a missing band, and a longer window can span a regime change and score worse.
  Diagnose a weak entry as a selection-method problem before treating it as a sample-size one.
  Evidence: [../../history/2026-08-12-marginal-ranking-misses-bands.md](../../history/2026-08-12-marginal-ranking-misses-bands.md).

**Full search vs re-tune.** A fingerprint with a persisted report re-tunes instead of
re-searching: verify the selected conditions still pass the decision test on fresh data,
re-screen last time's near-misses, tune values locally. Minutes, not a full search - and
the weekly re-tune is the common case across a portfolio.

Wall-clock targets (estimates until the pilot measures them): barren fingerprint 3-5 min,
full search 15-30 min, re-tune under 5.

## Validating the method - pilot protocol

The check that the method is actually useful: run it by hand, as the driver would, on one
fingerprint that already carries a strong incumbent rule, and compare.

**Cohort choice.** A benchmark cohort with a known incumbent (g3 `d5b5c6f3`, 554 matched
tokens, incumbent v2 at +1.307 SOL / PF 1.56 under authority pricing) makes the result
readable: the method must produce a rule at least as good on data neither has seen, plus
an honest verdict, inside the wall-clock target.

**Prerequisites.** Fresh `lake-export --include-today`; lab running `--release`; scoped
sweeps re-run rather than read from rows stored before 2026-08-11 (token-cap truncation).

**Execution mapping** - each step through existing surfaces, no new code:

| Step | Surface |
| --- | --- |
| 0 - folds, holdout, regime | SQL over `tokens` + `trades` (the playbook's Stage 0 query, split by fold) |
| 1 - menus | percentile SQL per metric, or a metric-discovery run's generated menus |
| 2 - staged search | grouped sweeps scoped by `fingerprint_id`, batched **inside retention** (6 axes x 2 values = 64 combos) so persisted marginals stay readable; fitting window = span minus holdout via `created_before` |
| 3 - fold consistency | re-run the accepted candidate's on/off pair per fold window (a 2-combo run per fold - cheap; batch all rounds of one fold together so the corpus cache is evicted once per fold, not once per round) |
| 3 - luck floor | export the on/off configs' per-trade results from simulate, resample token-level outcomes in a local script |
| 5 - fine values | one refine sweep over survivors only |
| 6 - acceptance | simulate battery on the holdout window: authority + optimistic pricing, four comparison columns |
| 7 - verdict | computed from the table above, by hand |

**What the pilot must show** before the driver is worth building:

1. The derived rule beats or matches the incumbent on the holdout under authority pricing.
2. At least one decision differs from what the old workflow would have chosen, and the
   fold/luck evidence for that decision is legible - the method must demonstrably decide
   things, not just re-derive the incumbent.
3. The verdict thresholds fire correctly (no hand-waving at the end).
4. Measured wall-clock per phase, to calibrate the driver's budget knob and the targets
   above.

The pilot costs local compute only - no Helius, no live trading. Its per-phase timings and
any decision the manual mapping cannot express cleanly become the driver's requirements
list.

## Driver requirements (confirmed by the 2026-08-12 g3 pilot)

The pilot validates the method end-to-end (derived rule `d57f17d2`, verdict USE; full
evidence in `docs/history/2026-08-12-rule-search-method-pilot-g3.md`) and pins these
requirements:

1. **Marginals come from full in-RAM aggregates.** Persisted-row reads lose fold cells to
   retention even at 8-combo grids.
2. **Every accepted entry condition is simulate-confirmed** before it enters `S` - sweep
   entry semantics produce phantom marginals (a condition that fires on 99% of tokens can
   still read 4/4 fold-positive in sweep rows).
3. **Alternation runs at least two full passes** (entries under exits AND exits under
   entries). The pilot's winning entry pair is invisible to any single greedy pass.
4. **The luck floor switches form by fire fraction**: permutation of selected subsets when
   k/n is small; test of the excluded set when the gate fires on most tokens.
5. **The driver owns per-position outcomes in-process** - the HTTP result pager subsamples
   and the RAM working set evicts older draft runs.
6. Engine runs are cheap (sweep 9-14 s, simulate 3-9 s warm); the manual procedure's hour
   is orchestration, not compute. The 15-30 min full-search target is realistic.
