# 2026-08-12 - marginal-ranked selection cannot see a band; a longer fit window does not fix it

A two-stage entry/exit search (entry on per-trade edge against a neutral exit, exits on total
SOL, walk-forward acceptance) was built and run against three cohorts with well-rated
incumbents. It lost to the incumbent on all three, and the first diagnosis was wrong.

## What was measured

Fit 7d, test on the next untouched 7d, 2 test weeks, total SOL, 0.05 SOL notional:

| cohort | procedure | incumbent | no rule |
| --- | --- | --- | --- |
| g3 `d5b5c6f3` | -0.300 | +0.932 (v2) | -3.236 |
| g12 `722411df` | +0.004 | +1.599 (v13) | +0.348 |
| g13 `897353e1` | +0.580 | +0.861 (v14) | -0.129 |

## The refuted hypothesis

The first diagnosis was that the 7-day fit window starved the condition budget: a 7d fit left
27-67 trades, so a budget of `clamp(n/40, 2, 6)` arms allowed 2, while v13 and v14 each carry a
four-arm double band (`time 10-50` plus `liquidity 10-45`).

Re-running with an **expanding** fit window anchored at 2026-07-22 - week 2 fitting on 14d
instead of 7d - refuted it: g3 -0.612 (worse), g12 +0.194 (better), g13 +0.375 (worse). The sum
moved from +0.284 to -0.043 and the incumbent still won 3 of 3. On g3 at 402 fit trades, where
the budget allowed 6 arms, the entry stage still selected **one** condition. The budget was
never binding.

## The actual cause

Selection ranked each candidate by its individual on/off marginal and kept the top 8. The arms
of a good band each carry a small individual marginal, so they lose that pre-screen to looser
single-sided candidates and never reach enumeration together. Marginal ranking cannot see a
pair that only pays jointly.

The fix is a pool change, not a data change: a band is **one** composite candidate scored as a
unit, which is what [../plans/strategies/rule-search-method.md](../plans/strategies/rule-search-method.md)
Step 1 specifies. The g3 pilot of that method independently found the same thing - its winning
entry pair is invisible to any single greedy pass.

Secondary finding: a 14d window on these cohorts spans a regime change, so more history was
actively worse on g3.

## Defects found along the way

- A status poll reading `psql` output without `-t` compares the column header against
  `running`, leaves the wait loop immediately, and reads results the sweep has not written -
  reporting a completed 533-token run as a cohort of 0 trades.
- Matching a scored combo back to its axes by metric presence alone cannot tell two candidates
  on one metric apart, so it mislabels the winner and then mis-scores it.
- The sweep's single-flight gate releases after the run row is marked complete, so a driver
  that polls the row and fires immediately gets a 409. Dropping the batch instead of retrying
  silently hollowed out a pre-screen.
