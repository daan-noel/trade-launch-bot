# Rule-search method pilot on g3 (2026-08-12)

First end-to-end run of [rule-search-method.md](../plans/strategies/rule-search-method.md),
executed by hand on fingerprint `d5b5c6f3` (g3, 554 exact-match tokens) to validate the
method before building the automated driver. Data through 2026-08-11 13:20 UTC (the
EC2 sync needs an interactive SSH passphrase, so the freshest ~18 h are absent).

## Setup

- Fitting window 07-23 .. 08-04 (410 tokens, 4 folds of ~103); holdout 08-05 .. 08-11
  (144 tokens), untouched by every search decision. Regime check: no break.
- All quotes at 0.03 SOL notional. Sweep/sim authority pricing = worst fill +
  `pumpfun_impact`; optimistic = `first` + `fee_only`.
- Incumbents: v1 `84919e0e` (active, real), v2 `52e655a1` (inactive; tuned 08-11 on data
  through 08-11, so the holdout is INSIDE v2's own fit window - the comparison is biased
  in v2's favor).

## Result - the derived rule (F2, shipped as `d57f17d2`, inactive, paper)

```json
"stop_loss": 35,
"entry": {"m_flow_window": {"net_flow": [{">=": 5}], "unique_wallets": [{">=": 12}],
          "window_size_sec": 3}},
"exit":  {"m_flow_split_window": {"nonvol_buy": [{">=": 0.9}], "window_size_sec": 2},
          "m_snapshot": {"liquidity": [{">=": 85}]},
          "m_price_lifetime": {"stall": [{">=": 300}]}}
```

Holdout battery (simulate, identical 144-token window, none seen by the F2 search):

| Config | Trades | Win | Net SOL | PF | Optimistic net | Spread |
| --- | --- | --- | --- | --- | --- | --- |
| **F2 (derived)** | 71 | 0.507 | **+0.420** | 1.66 | +0.593 | **1.41x** |
| v2 (incumbent, in-sample here) | 71 | 0.521 | +0.508 | 1.87 | +0.725 | 1.43x |
| v1 (the rule actually live) | 83 | 0.313 | +0.206 | 1.33 | - | - |
| no rule at all | 109 | 0.121 | -0.942 | 0.52 | - | - |

Verdict checks: holdout positive PASS; beats no-rule PASS; folds 3/4 positive PASS
(+0.048 / +0.285 / +0.077 / -0.042, on the ~47% row sample the pager returns); holdout
trades 71 >= 8 PASS; fill spread 1.41x < 4x PASS. **Verdict: USE** - with the honest
note that F2 does not beat v2 on this comparison (0.420 vs 0.508), and the comparison
favors v2 structurally. The clean test is the next fresh week, where both are
out-of-sample.

## What the method demonstrably decided (differently from prior passes)

- **The winning entry is a pair that pays only together.** `unique_wallets >= 12` alone:
  +0.028; under the neutral-exit joint test it is NEGATIVE in-presence and gets dropped;
  `net_flow` alone: below the off row. Together under the real exits: +0.415 vs the
  greedy path's best +0.170. Only the alternation pass (re-probing entries under the
  chosen exits) surfaces this - a single-pass greedy search returns the vol_net rule and
  leaves ~60% of the money.
- **`held >= 240` inverts across entries**: best exit-add under the `vol_net` entry,
  clearly harmful under the pair entry. Same lesson as g3 v1->v2's `stall` inversion;
  the drop-probe half of the loop catches it.
- **`retrace` parks on evidence** (off-row best under the pair entry) - v2 carries
  `retrace >= 35`; the fitting data says it subtracts. Recorded in `params.disabled`.
- **`liquidity > 15` is a phantom in the sweep**: 4/4 fold-positive marginals in sweep
  reads, then simulate shows it fires on 291 of 293 tokens - a no-op. Sweep-read entry
  marginals can be artifacts of the sweep's entry semantics; simulate confirmation of
  every accepted entry condition is mandatory, not optional.
- **`nonvol_buy >= 0.9` (w2) is the single load-bearing exit**: plateau 0.7-1.1, cliff at
  1.3+. `net_flow`'s value sits on a flat 3-12 plateau (the condition matters, the value
  inside the band does not).

## Method lessons -> driver requirements

1. **Retention truncation corrupts decision reads even at 8-combo grids** - fold cells
   for `vol_net` came back with the on-rows dropped twice. The driver must read
   marginals from full in-RAM aggregates, exactly as designed.
2. **The sweep ranks, simulate decides** is not a formality: one of three sweep-approved
   entry conditions was a simulate no-op (the `liquidity > 15` phantom).
3. **Alternation is load-bearing, minimum two full passes.** The pilot's one skipped
   re-probe (exits under the new entry) is precisely where the incumbent's remaining
   edge lived; the C4 pass closed most of the gap.
4. **The permutation luck floor is powerless when the gate fires on ~85% of the null
   set** (k close to n gives a near-zero-variance null). The driver needs the
   complementary form there: test whether the EXCLUDED tokens are worse than a random
   exclusion of the same size.
5. **Per-position row access is the missing primitive.** The result pager returns ~47%
   of fired rows and the RAM working set evicts older draft results, which silently
   degrades fold/bootstrap reads. The driver computes these in-process instead.
6. **The discovery validate layer carries the closed-only trap**: a bare TP100 rule
   "holds" at +2.16 SOL because only winners close; its verdicts are unusable until the
   open mark is added back (its screen + menus are fine and are what the pilot used).
7. **Timings** (release lab, warm corpus cache): grouped sweep 9-14 s, simulate 3-9 s,
   discovery run ~3.5 min. The pilot spent ~35 engine runs; a driver executing the same
   sequence lands well inside the 15-30 min full-search target. The manual orchestration
   around those runs - JSON authoring, SQL reads, retention workarounds - is where the
   hour went, which is the case for the driver in one number.

## Open items

- F2 vs v2 on genuinely fresh data (both out-of-sample) decides the champion; re-run
  the battery after the next sync.
- The `unique_wallets >= 12` verdict here (kept, in-pair) coexists with the fs3-00
  refutation and the neutral-exit in-presence rejection: the metric is cohort- and
  context-conditional, never a portable law.
