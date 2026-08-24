# Convexity search: the from-scratch guide

**Read [edge-at-real-latency.md](edge-at-real-latency.md) first.** It states the fill,
cost and exit-shape constraints that decide whether any result here is real, and it
governs this file — where the two disagree, that one wins.

**Otherwise this file stands alone.** It assumes no session history and no prior result.
Read it start to finish and you can run the search that produces tradable rules.

Results produced by this method live elsewhere; nothing here depends on them.

---

## 1. The purpose

**Find the islands in this market, explain each one logically, and turn each into a rule.**

An **island** is a recurring market situation that produces profitable convexity: a
configuration of the tape where buying now has a fat enough right tail to pay for a
negative median.

Three things follow, and they are the whole brief:

- **Entry metric combinations are a language for READING which island you are standing in.**
  They are not a feature set to throw at a model. Every term must mean something you can say
  out loud in one sentence.
- **The exit exists to keep the profit and cut the loss.** Nothing else.
- **The objective is maximum total SOL and stability** - the fraction of days positive.

**The deliverable is a list of islands, each with a plain-language mechanism and a runnable
rule.** A model score is not a deliverable. A PnL table with no mechanism is not a
deliverable.

**Wallet studies are a thermometer, never the target.** Reconstruct a known-profitable
wallet only to check that your pipeline reproduces a book you already believe. Then drop it
and go back to the market. Every hour spent trying to clone a wallet is an hour not spent
finding the island that wallet is standing on.

---

## 2. What you are hunting: the shape of a convexity book

A profitable book in this market looks wrong by every conventional measure:

- **Negative median trade.** Most trades lose a little.
- **Win rate well under 50%**, often near 30%.
- **The top 1% of episodes carries the entire book.** The bottom 90% is a net loss that the
  right tail pays for.

A useful calibration: reconstructed reference operators run 300-500 trades a day at 28-40%
win rates, medians of -4% to -6%, and expectancies of +2% to +8% net. Roughly 2-5% of their
episodes return more than +100%, and those pay for everything else.

**Rules that follow, and they are not negotiable:**

- **Never require a win rate above 50% or a positive median.** You will filter away the
  thing that pays.
- **Never cap the right tail.** A static take-profit has no market meaning and removes the
  band the book depends on. Capping a convexity book at +17% turns a clearly profitable
  wallet into a clearly unprofitable one.
- **But do not remove the exit either.** Hold-to-death is catastrophic. The correct shape is
  a **fast falsification stop** plus a **wide trail**: cut quickly while the thesis is
  unproven, then give the move room.
- **Rank in SOL, and in NET percent.** Never in gross price move (see section 7).

---

## 3. The workflow

Three independent questions. Answer them separately, then combine. Confusing them is the
single most common way to waste a week.

**Order by what each answer depends on.** Some facts hold no matter what else is decided;
others are meaningless until something upstream is fixed. Do the independent ones first.

```
  INDEPENDENT of exit and fill          DEPENDS on the exit
  ----------------------------          -------------------------------
  "this token rugs to -90%"             "is this moment convex?"
  loses under EVERY exit, at            the SAME 16,874 entries price
  EVERY latency                         -65.44 SOL under one exit and
                                        -20.66 SOL under another
          |                                          |
          v                                          v
   1. CUT the poison           ->    2. FIX the exit SHAPE    ->    3. Find the MOMENT
      never-enter blacklist            armed / cause-based            on clean tokens,
      decision-time facts only         (section 5)                    with a working exit
          |                                                                 |
          +-------------------- 4. tune thresholds, re-check the cut once ---+
                                            |
                                    gates, then ship
```

- An island has **two coordinates**: which token you are in, and which moment inside it.
  Searching only the moment leaves most of the available edge on the table, and the token
  axis is the under-searched one.
- **The cut pays first and costs nothing.** Loss avoidance by never entering beats loss
  avoidance by exiting well: no fee, no impact and no adverse fill, because the trade never
  happens.
- **The cut is also what makes the moment search possible.** A greedy search cannot start
  from a deeply negative universe - no single term is positive there, so it reports an empty
  space. Rug and instant-death tokens contaminate every region equally; removing them raises
  the baseline and lifts signal-to-noise in every remaining metric at once.
- They do **not** multiply cleanly. A good token still has ~150 buyable moments and only a
  handful are the right one. Expect the token axis to add ~1.2-1.5x on top of state, not 3x.

### The five phases, in order

0. **Fix cost first.** Size as a fraction of pool depth, because impact is exactly
   `buy / vsol`. Arithmetic, not search - see [execution-costs.md](execution-costs.md).
1. **Build the decision-point extract** (section 4). One pass over the tape; every later
   question becomes an in-memory scan.
2. **Cut the poison tokens.** Build the never-enter blacklist from decision-time facts
   alone. This is the only step whose answer does not move when the exit changes, which is
   why it is the only one that can be settled before the exit.
3. **Settle the exit (section 5) BEFORE the moment search, not after.** Choose the shape
   from its mechanism, then tune thresholds on the cut population. Entry edge dies at
   latency and exit edge does not, so this is where the money is.
4. **Partition the remaining space** (section 6) into readable leaves, and price every leaf.
5. **Gate everything that survives** (section 8), then tune thresholds and re-check the cut
   once - the cut and the exit are coupled, the moment is separable.
6. **Name the mechanism in one sentence per island.** If you cannot, you have not found an
   island - you have found a fit.

---

## 4. Building the decision-point extract

A grouped sweep re-walks the tape once per grid cell and never finishes. Two extract passes
make every later question a scan instead.

**Cohort by token CREATION day**, not by trade day, so lifetime metrics and forward paths
are both complete. A token created on day D needs day D and day D+1 of tape to be whole.

**Sort prints by `(mint, block_time, tx_index, leg_index)`.** Then the row index is
ascending within a mint and across mints, which makes almost everything below a vectorized
`searchsorted` with no per-token loop.

**Address windows through a synthetic monotone key.** With `blk` the mint's block number and
`t_first` its first print time:

```
key = blk * BIG + (t - t_first)          # BIG larger than any mint's lifetime in us
lo  = searchsorted(key, key - window)    # clamps to the mint's own block automatically
```

Prefix-sum the flow columns once (`cumsum` of buy SOL, sell SOL, trade count) and every
window is two lookups.

**Rolling high/low from a sparse table over `log(price)` in float32.** Logs keep the ratio
precision that raw 1e-14 prices lose, and one table answers every window size.

**Two decision points per slot: the FIRST and the LAST print.** The first is what a reactor
sees arriving; the last is what a rule whose trigger accumulates through the slot sees.
Taking only one loses half the triggers.

**The price series is the execution price** (`sol / token`), because that is what the
engine's metric fold reads. Reserve-pair spot is right for a chart or an ATH and disagrees
with what every rule actually sees.

**Gate the population** to what is tradable: real reserves above some floor (3 SOL is
reasonable) and an age past the opening scramble (5 s).

### The metric vocabulary

Per decision point, compute at several windows so the search can pick its own timescale:

| family | windows | why |
| --- | --- | --- |
| `net_flow`, `gross_flow`, `trade_count`, `buy_share` (by SOL and by count) | 0.4s, 2s, 10s, 30s, 60s, lifetime | 0.4s is about one slot; 60s is sustained pressure |
| `rise` (up from window low), `trail` (down from window high) | 3s, 10s, 30s, 60s, lifetime | has the move already happened |
| `liquidity` (real SOL reserves), `age`, `stall` (seconds since last print) | point | where in the token's life you are |
| creation-time fields: instruction count, instruction labels, first-slot buy SOL | point | the token axis; cannot look ahead |

**Buy share by SOL and buy share by trade count are different signals.** High SOL share with
low count share means a few large buys absorbing many small sells. Compute both.

### The second pass: barrier crossings

Record, on one forward walk, the first-crossing time and realized return for every take
profit, stop and hold cap you might want. Then any `(tp, sl, hold)` combination prices from
stored crossings with no further walking.

**Record the return AT the crossing print, never at the barrier level.** The first print past
a threshold has already gapped; filling at the threshold is worth about 2.5 percentage
points of pure fiction.

---

## 5. The exit: fix it a priori

**Do not select an exit by its own measured performance.** This fails reliably: a shape
chosen because it scored best in-sample flips sign out of sample.

Choose the shape from mechanism — and the mechanism is set by the fill, not by the
price path. A stop or trail waits for adverse movement, so it is adversely selected at its
own fill and cannot be modelled as filling at its level. See
[edge-at-real-latency.md](edge-at-real-latency.md) §2.

```
EXIT   arm_above_pct A + retrace >= T      # armed: trails only from strength
       OR a cause-based leg                # flow reversal, before price turns
```

- **Arming is required, not optional.** An unarmed `retrace >= N` is a hard -N% stop:
  `PositionCtx::at_fill` seeds `peak_price = entry_price`, so retrace measures
  drop-from-entry until price rises. Unarmed trails turn 21% of winners into losers and no
  width from 2-20 rescues it. See [armed-trailing-stop.md](armed-trailing-stop.md).
- **The stop is a falsification leg.** The thesis was "demand is arriving". If price
  immediately goes the other way, the thesis is wrong. Small: a few percent — and priced at
  the honest fill, ~3.5% past its own level.
- **Trail width is an empirical question, not a prior.** Tighter beats wider on some
  populations once the fill is charged. Report the surface, not the best cell: **if it is
  not a plateau, it is not real.**
- **No take-profit, no hold cap.** Both cap the right tail.

**The exit is worth more than the entry** — on one island the exit alone moves expectancy
by +0.0028 SOL/trade. Settle it on a known population before searching entries; an entry
search wearing a bad exit reports an empty space.

### The bug that will cost you a week

```python
np.maximum(peak[act], yj, out=peak[act])     # WRONG - fancy indexing returns a COPY
peak[act] = np.maximum(peak[act], yj)        # RIGHT
```

The first form writes into a throwaway, so the running peak never advances past the entry
fill and **every trailing exit silently runs as a hard stop**. It invalidates every exit
comparison you make. **Unit-test any walk on a hand-made path before trusting one number
from it:**

```
price 1 -> 2 -> 1.9 with a 5% trail must exit at 1.9 for TRAIL reason, not read as a stop.
price 1 -> 0.96      with a 3% stop  must exit for STOP reason.
price that only rises must reach the end of the block without firing.
```

---

## 6. Finding islands: partition the whole space

**Partition the WHOLE space.** Clustering inside a region a model already liked can only
find islands that model already liked.

Use a decision tree as a **candidate generator only**:

- **Target: a binary runner label**, not the money. A squared-error fit on a fat-tailed money
  target learns the cost surface (it will discover that impact is large when liquidity is
  small) instead of a signal.
- **Runner = realized return under the reference exit >= +50%.** Never MFE: most of a
  favourable excursion lands after the exit fires, so an MFE-based label selects moves you
  cannot harvest.
- Depth 5-7 and a large `min_samples_leaf` so leaves read as rules a human can say out loud.
- **Then score every leaf honestly in SOL** - collapsed to episodes, at the honest fill,
  under the fixed exit, fitted on the search days and read once on the holdout.

**Do not optimise P(runner) alone.** It selects blow-off tops: already vertical, real upside
left, brutal downside. Price both tails - model P(big loss) separately and require the
combination.

**Choose the portfolio on the FIT half only.** Counting how many leaves clear both halves is
already peeking. Select by in-sample SOL; the holdout column is a read, not a filter.

**Check overlap before claiming a list.** Two leaves that fire on the same tokens at the same
moments are one island. Measure overlap twice:

- at the **decision point** (same token AND same instant) - low overlap means genuinely
  different triggers;
- at the **token** level - high overlap here with low overlap above means *same population,
  different moments*, which is the healthy structure.

Then re-score each island with the others removed. If an island collapses when its
neighbours are excluded, it was never separate.

---

## 7. Measurement rules

These are the ones that have each caused a wrong conclusion at least once.

**Rank in SOL and in NET percent.** Net per trade is `total SOL / (trades x size)`, after
fee, impact and fixed cost. The gross price move runs several points higher and flatters
every rule. Noise alone yields roughly 12% gross per trade.

**Collapse to non-overlapping episodes per mint before any money number.** Overlapping
decision points multiply the same outcome. Earliest-first is not a neutral collapse - among
several qualifying moments in one token, the first to cross a threshold is the weakest - so
read a universe-wide collapsed total as a policy, not as a mean.

**Then test re-entry separately.** One episode per token per day is the conservative
baseline; allowing repeated non-overlapping holds in the same token is a different policy
and can be worth a lot. Measure both.

**Cluster errors by mint.** Effective n is tokens, not prints. A region of 15,000 rows can be
338 mints.

**Fill one print late, and check the gap CONDITIONALLY.** The universe-wide next-print gap is
approximately nothing. Conditional on an impulse trigger it is materially positive.
Measuring the average when the conditional binds makes every impulse rule look about two
points worse than it is.

**Price at the honest fill, on BOTH legs.** `FillModel::LagMs(115)` - the measured
decide-to-fill p50 - is the number to believe. The slot-shaped models only bracket it:
`FirstInWindow` assumes an ordering privilege no latency buys, and the `NextSlot*` pair
assumes the signal's own slot is never reached when the live book reaches it ~52.6% of the
time. **A latency correction applied to one leg is not a latency correction** - the exit-leg
artifact is the larger of the two. See
[edge-at-real-latency.md](edge-at-real-latency.md) and
[fill-and-cost-models.md](fill-and-cost-models.md).

**A term that only pays at the next print is buying fill luck, not signal.** Always report
the zero-lag column beside the honest one; the ratio is the artifact size, and some terms
invert.

**Audit every feature for look-ahead.** Time-until-the-next-print is not knowable at the
decision instant. Neither is anything derived from the fill.

**Mark a dead exit at the curve, not at zero.** A pre-migration bonding curve is always its
own counterparty - the SOL sits in the curve and cannot be pulled - so "no trades for 300
seconds" means nobody else traded, not that the position is unsellable. Book the last print
less your own impact. Only a post-migration pool can be worth zero, and migration is rare.

**Run a reproduction gate before trusting any search result.** Rebuild a known-profitable
book from the tape and check your pipeline reproduces its trade count, win rate and
expectancy. This catches aggregation bugs immediately.

---

## 8. The gate checklist

Nothing ships without all of these. Each has killed a plausible result.

| gate | what it asks | failure looks like |
| --- | --- | --- |
| **Tie fraction** | is the axis real, or 99% one value | a "signal" that is a constant |
| **Perturbation** | move every threshold a step | sign flips on a 2-point change |
| **Same-mint control** | a random moment in the SAME token | the edge was token selection, not timing |
| **Placebo** | same tokens, entry shifted +30s and +120s | edge survives a time shift, so it is not momentary |
| **Out of sample** | fit early days, read later days once | in-sample only |
| **Days positive** | per-day sign, not just the total | one day carries the week |
| **Fill model** | price at next-slot as well as next-print | term only pays on same-slot fills |
| **Beat random** | compare against ~20 random draws of equal size | the region is not special |
| **Runner rate** | does `>= +50%` frequency SURVIVE the cut | expectancy rose by deleting the right tail |
| **Decision-time only** | is every cut term knowable BEFORE entry | the filter reads the outcome it predicts |

**A plateau, not a peak.** Report the whole surface around a chosen threshold. If neighbours
are negative, it is noise.

**Latency ladder.** Price at 0 / 50 / 115 / 200 ms on both legs. A real edge decays
gradually. An edge whose sign is already gone by 50 ms is a same-block race, not a
reaction, and no hardware wins it.

**Concurrency cap.** Unlimited simultaneous positions is not a live setting. Re-run with a
cap; the cap binds on trade count first and tells you the real operating point.

---

## 9. Cost - get this exactly right

Cost decides everything. A 3.5% round trip means a rule needs a large gross edge to clear.

| component | value |
| --- | --- |
| protocol fee | 125 bps per leg, 2.5% round trip, immovable |
| fixed cost per leg | tip plus priority, on the order of 0.000225 SOL |
| own impact per leg | `B / vsol`, where `vsol = real_reserves + 30` |

Round trip at 0.1 SOL is about **3.37%**. Percentage-optimal size is `sqrt(F x vsol)`,
around **0.11 SOL**; both smaller and larger cost more per unit.

**Charge impact on the VIRTUAL reserve.** On a constant-product curve, spending `B` gives an
average price of `(vsol + B) / vtok`, exactly `1 + B/vsol` times the pre-trade spot. Charging
`B / real_reserves` instead overcharges by `vsol / (vsol - 30)`: 1.6x at 50 SOL of real
liquidity and **11x at 3 SOL**. If your engine does this, every backtest is pessimistic,
most where the pool is thinnest - which is exactly where these islands live.

**Useful curve facts:** `liquidity` is real SOL reserves, equal to `vsol - 30` on the curve.
Graduation sits near `vsol` 115, so real liquidity near 85. Price grows roughly with the
square of the pool, which is why entry liquidity sets the maximum loss and why a token past
about 3/4 of the way to graduation has no room left for a +50% move.

---

## 10. Pitfalls

Every one of these has actually happened.

- **Capping the right tail with a static take-profit.** Structurally cannot work on a
  convexity book.
- **Selecting the exit by its own measured performance.** Fails every time.
- **Training a selector on one exit's outcome and applying a different exit.** One selector
  per exit shape, trained on that shape's own outcome.
- **Clustering inside a pre-filtered region.** Partition the whole space.
- **Squared-error loss on a fat-tailed target.** Use a binary target.
- **Optimising P(runner) alone.** Price the loss side too.
- **Measuring an average when the conditional binds.**
- **Reporting the gross price move as "per trade".** Report net.
- **Selecting on the holdout** - including "how many leaves clear both halves".
- **Look-ahead features.**
- **The fancy-index `out=` peak bug** (section 5).
- **Drifting into wallet studies.** Check the thermometer, then leave it.
- **Reporting model diagnostics instead of the deliverable**, or skipping a promised step.

---

## 11. Definition of done

An island is finished when you can state, in this order:

1. **The mechanism, in one sentence** a trader would recognise. "A few large buys are
   absorbing many small sells" is a mechanism. "Feature 12 above 0.83" is not.
2. **The rule**, in metric terms that the engine can evaluate.
3. **The money**: total SOL, trades per day, net per trade, win rate, median, runner rate.
4. **Stability**: days positive, in-sample and out-of-sample totals separately.
5. **The gates it clears** (section 8), each with its number.
6. **The operating point**: size, concurrency cap, and the latency at which it still pays.
7. **What it does not claim.** Say the limits out loud.

If any of the seven is missing, it is not ready to trade.
