# Universe-wide signal search: the decision-point method

Read [signal-search-mandate.md](signal-search-mandate.md) first.
[rule-search-method.md](rule-search-method.md) holds the per-fingerprint fitting mechanics.

This file is the **method** for searching the whole universe at once. The region it finds
and the rule built on it live in
[impulse-inception-island.md](impulse-inception-island.md).

The first run against this extract reported no region beating cost. That verdict is
withdrawn: its exit walk pinned the running peak at the entry fill, so every trailing shape
scored as a hard stop
([history](../../history/2026-08-21-trailing-exit-peak-bug.md)). The surviving parts of
that round - the cost surface and the fill-position measurement - are in
[history/2026-08-21-island-search-refuted.md](../../history/2026-08-21-island-search-refuted.md).

## Why a grouped sweep does not finish

A grouped sweep re-walks the tape once per grid cell, and the tape is ~400 MB/day of
Parquet (~2.2M prints per creation-day cohort). Two extract passes make every later
question an in-memory scan instead:

| pass | emits | cost |
| --- | --- | --- |
| decision points | one row per (mint, slot) x {first print, last print} | ~80 s/cohort-day |
| barrier crossings | first-crossing time + realized return for each TP, SL and hold cap | ~75 s/cohort-day |

The second pass is what makes exit search free: record the crossing of every barrier level
on one forward walk, and any `(tp, sl, hold)` combination prices from the stored crossings
with no further walking. Record the return **at the crossing print**, never at the barrier
— the first print past a threshold has already gapped.

## Constructing the table

**Cohort by token creation day**, not by trade day, so lifetime metrics and forward paths
are both complete. Sort prints by `(mint, block_time, tx_index, leg_index)` and address
windows through a synthetic monotone key, `mint_block * BIG + (t - t_first)`: a
`searchsorted` on that key clamps to the mint's own block automatically, so one global
vectorized pass computes per-mint windows with no per-token loop. Rolling window high/low
comes from a sparse table over `log(price)` in float32 — logs keep the ratio precision that
raw 1e-14 prices lose, and the table answers any window size.

**The price series is the execution price** (`sol / token`). That is what the engine's
metric fold reads: `TradeLite.price` is `price_per_token` on the live path
(`producers::trade_lite`), the lab path (`projection::to_trade_lite`) and the readout path
alike. Reserve-pair spot is the right price for a chart or an ATH, and disagrees with what
every rule actually sees.

**Two decision points per slot** — the first and the last print. The first is what a
reactor sees arriving; the last is what a rule whose trigger accumulates through the slot
(`net_flow(0.4)`) sees. One alone loses half the triggers.

## Guards this method must keep

- **Collapse to non-overlapping episodes per mint before any money number.** Overlapping
  decision points multiply the same outcome. Earliest-first is not a neutral collapse — it
  systematically picks each token's youngest, thinnest moment — so read universe-wide
  collapsed totals as a policy, not as a mean.
- **Mark a dead exit at the curve, not at zero.** A pre-migration bonding curve is always
  its own counterparty - the SOL sits in the curve and cannot be pulled - so "no trades for
  `DEAD_QUIET_SECS`" means nobody else traded, not that the position is unsellable. Book the
  last print less our own impact. Only a post-migration AMM pool can be worth zero, and in
  the studied cohort 3 of 107,211 mints ever migrate. Booking dead at -100% is what made a
  working trail look like it holds tokens into their own death; with the peak fixed the dead
  rate on a tight trail is ~1%, so the convention barely binds either way.
- **Cluster errors by mint.** Effective n is tokens, not prints. A region of 15k rows can
  be 338 mints.
- **Audit the feature list for look-ahead.** Time-until-the-next-print is not knowable at
  the decision instant; neither is anything else derived from the fill.
- **Rank in SOL, not percent**, and confirm a shape by perturbing its thresholds, not by
  its own score: an edge that does not survive a 2-point change in one threshold is noise.

## Cost model

Charge fee 125 bps/leg, `fixed_cost_sol_per_leg` (currently 0.000225), and impact
`B / vsol` per leg — the **virtual** reserve. On a constant-product curve, spending `B`
gives an average price of `(vsol + B) / vtok`, exactly `1 + B/vsol` times the pre-trade
spot; measured against 1.006M real curve buys the identity holds to 3e-11.

> The kernel currently charges `B / real_reserve_sol` instead, because `TradeLite` carries
> real reserves for the liquidity metric and the deadness verdict and `leg_impact` reads
> the same field. That overcharges impact by `vsol / (vsol - 30)` — 1.6x at
> `liquidity 50`, 11x at `liquidity 3` — so every backtest is pessimistic, most where the
> pool is thinnest. Charging the correct basis moves a universe-wide decile by ~1pp.
