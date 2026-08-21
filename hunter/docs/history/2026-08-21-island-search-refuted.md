# 2026-08-21 — Island search refuted: no multi-window region beats cost

> **Partly retracted the same day.** The exit walk behind every number here had a broken
> running peak, so all three trailing shapes ran as hard stops from entry - see
> [trailing-exit-peak-bug.md](2026-08-21-trailing-exit-peak-bug.md). The
> "exit shape dominates the entry" section below is **void**: it compared a bracket against
> two mislabelled hard stops, with dead booked at -100% on top.
>
> The entry conclusion also does not survive. Re-run on the corrected extract, a two-term
> region - `net_flow(0.4) >= 0.5 AND rise(3) <= 9` - takes a blind -14.37 SOL to +132.37
> over the same 7 days, 7/7 days positive, and clears placebo, same-mint and OOS gates:
> [`@plans/strategies/impulse-inception-island.md`](../plans/strategies/impulse-inception-island.md).
> What stands below is the *method*, the cost surface, and the fill-position finding.

**Hypothesis.** The profitable region is not one blob but several disjoint islands taking
opposing signs on the same axes (63ot buys `trail(30s)>=21` with negative flow; 3Xk2 buys
`trail(30s)~0`, `rise(30s)>=36` with positive flow). A single AND-band rule can sit on at
most one island, so a global band search averages islands whose signs cancel and reports
"no signal" — which would explain the round-10 null without the market being efficient.
Fix: cluster the convexities first, fit one rule per island.

**Verdict: refuted on this feature set at slot resolution.** An interaction-aware model
that can represent every island simultaneously still cannot push any out-of-sample region
above break-even.

## What was run

7 token-creation cohorts, 2026-08-13..08-19, universe-wide, no fingerprint scope.
15.34M prints over 175k mints → **7.76M decision points** (first and last print of each
(mint, slot), gated on `liquidity >= 3` and `age >= 5s`). Fit on 08-13..08-16, confirmed
once on 08-17..08-19. Sized 0.1 SOL, `FirstInWindow`, kernel cost math, dead exits booked
−100% (an unsellable pool has no bid). Errors clustered by mint; episodes collapsed to
non-overlapping holds before every money number.

Per decision point: `rise`/`trail` at 10/30/60/120s, `net`/`gross`/`buy`/`ntr`/`nbuy` at
0.4/2/10/30/60s, lifetime `trail`/`rise`/`stall`/`gross`/`net`, `liquidity`, `age`,
creation-slot buy SOL, and forward outcomes under five reference exits plus a
barrier-crossing table pricing 343 `(tp, sl, hold)` combinations.

## Results

| test | outcome |
| --- | --- |
| Blind entry (buy every decision point) | −2,206 to −9,305 SOL/week depending on exit |
| 32 axes × 10 deciles, marginal net | **0/7 days positive in every one of 320 cells**; best cell −3.4%/trade |
| Gradient-boosted model, 32 leak-free features, 4.4M train rows | OOS ranking is monotone and real (decile 0 −50.7%/trade → decile 9 −0.05%) but **no decile clears zero** |
| Top 1% of that ranking, collapsed, held out | +1.14%/episode, 3/3 days, **t = 1.11** (mint-clustered) |
| Top 0.3% | +3.60%/episode, 3/3 days, **t = 1.90** |
| 343 exits, universe-wide | best is −3.90%/episode OOS, t = −81.7 |
| 343 exits, inside the top-1% region | best-on-search (+12.83%) → **−0.87% OOS, 0/3 days** |

The last row is the refutation in one line: `sl 28` scores +1.14% and `sl 30` scores
−0.87% on the same rows under the same entry. **A 2-point change in the stop flips the
sign**, so the +1.14% is noise, not an edge.

The only region that looks positive is not a new island. Its profile is `liquidity` 63
(vsol ~93, against a 115 graduation ceiling), `rise_life` 804%, creation-slot buy 30 SOL,
`gross(60s)` 95 SOL, and 80.3% of its episodes hit +17% — the already-known,
already-priced graduation-approach trade. The model's ranking is 72% driven by
`liquidity` alone: it is learning the **cost** surface (impact and death rate), not a
directional signal.

## What this does and does not close

**Closed:** entry selection from price + aggregate-flow observables, at one-slot
resolution, over the whole universe. Adding more window sizes or more AND-terms to that
vocabulary cannot help — a model with every interaction available already saturates at
the cost line.

**Not closed, and now the only places left to look:**

- **Wallet identity.** Deliberately out of scope here. The studied wallets stay profitable
  for weeks, so their edge exists; it is not in this feature set, and (below) it is not in
  fill position either. Identity is where it has to be.
- **Sub-slot ordering.** Measurement resolution is one slot; FBvx's edge is already known
  to be consumed inside one.

**Ruled out as the explanation:** fill position. Re-pricing every decision point at the
next print, at the first print of the next slot, and at the signal print itself moves the
universe-wide result by **at most 0.3pp** (−19.85% / −19.83% / −19.58%). The next-print
gap is mean −0.13%, median +0.01%. Execution timing at this granularity is not the lever,
and the `FillModel` choice does not change a conclusion.

## Two findings that do NOT outlive it

Both come from the broken exit walk and are retracted; kept as the record of what was claimed.

**The exit shape dominates the entry.** On *identical* held-out rows: fixed bracket
+1.14%/episode, armed 18% trail −45.8%, wide 25% trail −38.7%. A 40–50pp swing from the
exit alone, an order of magnitude larger than anything entry selection moved. The
mechanism is death: bracket exits carry a 5.9% dead rate against 13–18% for trailing
exits, and booking dead at −100% collapses the trailing ceilings (armed-18 oracle falls
+1,219 → +259 SOL, wide-25 +1,284 → +299, while the bracket only moves +2,088 → +1,839).
A trail holds a token into its own death; a barrier does not.

**`stop_loss: 1` is not a stop, it is a coin flip.** (Retracted - with the peak fixed the incumbent exits 59% on the stop and 39% on the trail, and the tight stop is the left-tail cap a convexity book needs.) On the incumbent
`8dtx-A bundle<5 buy0.10`, that clause fires on **90%** of entries at a 3.6s median hold,
so only the 5–6% of fills that never tick down survive to express the thesis. Its `retrace
>= 7` is also unarmed, and `PositionCtx.peak_price` seeds at the fill, so the trail is live
from the first tick rather than after the +18% run the 8dtx mechanism was derived from.

## Method note

The extract is the reusable part and is worth keeping: one pass over the tape emits the
decision-point table (~80s/cohort-day) and the barrier-crossing table (~75s/cohort-day),
after which every rule, every region and all 343 exits are in-memory scans. A grouped
sweep re-walks a 400MB/day tape per grid cell, which is why one does not finish.
See [../plans/strategies/island-search.md](../plans/strategies/island-search.md).
