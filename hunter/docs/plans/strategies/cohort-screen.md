# Screening a launch cohort for a tradeable group

Four numbers decide whether a launch identity can carry a rule. Compute them for every
identity in the lake, rank, then price the survivors honestly. Calibrated against the
3ix targets the live rules trade (works) and the 6ix cohort (does not,
[`2026-08-27-6ix-fails-the-pass-through-bar.md`](../../history/2026-08-27-6ix-fails-the-pass-through-bar.md)).

## The identity

`(ix_labels, max_cost_lamports)`. `max_cost = dev_buy x slippage_preset`, so the pair is a
**launch-tool signature at one launch size**. The same tool appears at several sizes and
each size is its own population — `x1.0226 BuyV2` runs at 1.01, 7.07 and 15.15 SOL and
they behave differently.

## The four numbers

1. **Peak** — median peak `vsol`. This sets the target. **Derive it per group; never
   borrow another cohort's finish line.** 3ix peaks at 63–115 so graduation is its target;
   6ix peaks at 40–56, where graduation is a 1–2% tail.
2. **Pass-through** — `P(reach target | reached band)`. Against break-even
   `stop / (payoff + stop)` where `payoff = (T/B)^2 - 1`. Report **margin = pass / break-even**.
3. **Reachability** — the share of band arrivals whose first touch is **not** in the
   token's own launch slot. Without this gate the ranking fills with fingerprints that
   graduate inside their launch bundle: `0.101`, `0.001`, `0.0101` read 99–100%
   pass-through on 20/20 days and are untradeable.
4. **Day-stability** — days whose own pass-through clears break-even, over days with
   enough arrivals.

## The pipeline

* **1a `ladder.py`** — one pass over the lake: per token, the time and slot it first
  crossed each `vsol` level, plus peak, trade count, gross. 49.5M curve trades to 576k
  rows. Every later question is a lookup.
* **1b/1c `screen.py`, `rank.py`** — join the ladder to `(ix_labels, max_cost)`, score
  every `(band, target)` pair, gate on reachability and day-stability, rank on margin.
  193 identities clear 300 mints / 15 days; 3,799 cells.
* **2 `stage2.py`** — price the shortlist on the honest fill: entry at the first
  reachable band print, 115 ms both legs, target-or-40%-retrace exit, one trade per token,
  `gap` reported, then a fit/held-out split, a drop-the-best-days check, and exit
  robustness across stops and targets.

Stage 1 ranks on an *ever-reaches* probability, so its pass-through is an upper bound —
stage 2's hit rate is lower wherever the stop fires first. A large gap between the two is
the stop discarding winners, not a bug.

## Calibration

The screen recovers what is already known: the live `mc 0.108` and `0.216` 3ix
fingerprints rank first and near-first on margin, and `mc 15.15` (`g0 c287711`, live)
prices positive in stage 2 at +2.5% to +4.8%. A screen that does not reproduce the
running rules is not ready to propose new ones.
