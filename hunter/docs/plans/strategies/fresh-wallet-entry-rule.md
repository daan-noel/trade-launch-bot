# Fresh-wallet entry rule

The one entry rule from the signal search that survives an honest audit. It is a **screen**,
not a timing signal: it raises the density of tail events in a basket, and its median trade
loses money. Read [signal-search-mandate.md](signal-search-mandate.md) before treating it as
anything more.

Backtest window 2026-08-11..18 (8 days). Everything below is measured, not projected.

**Forward status: fund the deep half at 1% of pool; do not fund the rule as written.**
A walk-forward over six held-out days settles what eight rounds of in-sample search could
not. Trading `rule & vsol>=40` fixed - never re-searching - books **+2.34 SOL/day at 1% of
pool, positive on 6 of 6 held-out days and 9 of 10 overall**, with a 0.89 SOL ten-day
drawdown. The rule as written books more money on paper (4.46 held-out) and almost all of it
is one day: drop its best day and 4.46 becomes **0.90**, its top 1% of trades is **108% of
the book**, and **67% of its trades lose**. The deep cut is the first selection in the
program where a majority of trades win. Re-deriving the best filter combination each day
instead of fixing one loses to both (-0.23 SOL/day, 2 of 6 days). Full records:
[round 8](../../history/2026-08-20-round-8-cost-and-entry-depth.md),
[round 9](../../history/2026-08-20-round-9-entry-depth-forward.md) and
[round 10](../../history/2026-08-20-round-10-combination-search.md).

## What it needs

Two things the live path does not compute today.

**`c_fresh1h` — fresh-wallet share.** Of the wallets buying this token in the current window,
the fraction whose first-ever appearance anywhere is under 1 hour old. `wallet_dict` already
carries first-seen, so this is a join, not a new feed. No Helius call.

**A daily cutoff table.** The backtest ranks `c_fresh1h` into quintiles inside the candidate
pool per `day x age band`. Live there is no finished day to rank against, so the rank becomes a
number: take the pool's 80th percentile from the last 7 sealed days and refresh it nightly.

This substitution is not free, and it is the live form that pays. `c_fresh1h` drifts faster
in the older band than a 7-day window tracks: its 80th percentile runs 0.19-0.42 across
08-12..18 and reaches 0.60 on 08-19, so a cutoff of 0.29 selects about 40% of that band
instead of 20%. Measured cost on 08-19 is about 0.64pp (blind -0.34% against own-day +0.30%).
The young band is stable (0.935 sealed, 0.968 on 08-19) and carries most of the trades.

## Entry

All five conditions, at the same decision slot:

| condition | value |
| --- | --- |
| token age | < 75 slots (about 30 s) |
| `f_conc` | >= 0.5 |
| `is_cashback_enabled` | false |
| age band | split at 25 slots (about 10 s) |
| `c_fresh1h` | >= the band's cutoff |

The band split is not cosmetic. The 80th-percentile cutoff is about **0.93** in the 0-10 s band
and about **0.30** in the 10-30 s band, and a single threshold across both bands changes which
tokens are selected. Day-to-day standard deviation of the cutoff is 0.03 in the young band and
0.08 in the older one, so a weekly-refreshed constant tracks it closely enough.

`c_fresh1h` is strongly bimodal in the pool (p25 = 0.00, p50 = 0.06, p75 = 0.83), so the cutoff
lands on a sparse part of the distribution and small threshold changes move the trade count a
lot. Sweep the threshold on the lake before fixing it.

## Execution

Enter at the first slot carrying a buy print in `[decision + 1, decision + 3]`, at that slot's
median buy price. This is the bot's measured p50 latency, so it is what the backtest charges.

Size `B = sqrt(F * vsol)` is the constant-impact optimum, about **0.094 SOL** at the current
`F = 0.000225` SOL/leg - but it minimises the cost *percentage*, not the money. SOL per trade
is `B*e - 2B^2/vsol - 2F`, maximised at `B* = e*vsol/4`, near **1% of the pool**. Both sizes
break even at the same point, so the 3.45% bar stands; only the magnitude changes. At 1% this
rule pays **7.48 SOL/day against 2.56**, 8 of 8 days positive, net %/token falling +3.67 ->
+2.50 - and the drawdown rises **1.02 -> 6.17 SOL** with peak exposure 0.72 -> 2.86. Size
multiplies the sign of the edge: on the fresh day, where the rule loses, 1% sizing turns
-0.07 SOL into -2.22. Re-solve whenever `F` changes.

## Exit

Every field already exists in the rule editor.

**Mark the path on `px_sell_med` only, and drop slots that carry no sell print.** Do not
coalesce to the buy median: a buy-only slot is an up-tick nobody could have sold into, and
marking it raises the running max, fires the trail on a price that never existed, and books
the fill on the buy side. On this rule that one convention is worth 1.7pp of the result.

| field | value |
| --- | --- |
| `arm_above_pct` | 8% |
| trail | 4% |
| take profit | 15% |
| timeout | 300 s |

Fill at the **next print past** the trigger, never at the trigger price. The take profit and the
hold length are one setting, not two: at TP 30% the long holds overfit hard, at TP 15% the same
holds improve both windows.

Do not tighten the take profit. The rule is a convexity harvester and a tight TP caps the only
part that pays.

## Entry depth is the strongest cut on it

Filter on the pool's virtual SOL reserve **at the decision slot** (not the entry slot -
that leaks 0.23pp). Curves start at `vsol = 30` and the rule caps age at 75 slots, so
`vsol >= 40` means the token took 10+ real SOL in its first 30 seconds.

Nine complete days, all hours:

| | n | per day | `P(arm)` | net | median | win |
| --- | --- | --- | --- | --- | --- | --- |
| `vsol >= 40` | 971 | 108 | **75.9%** | **+7.24%** | **+12.99** | 67.9% |
| `vsol < 40` | 5,535 | 615 | 34.4% | +2.42% | -5.58 | 28.5% |

Break-even `P(arm)` on the deep half is 66.0% against 75.9% delivered. **8 of 9 days
positive** (only 08-14 negative). It is the one cut here carried by the body rather than
the tail: the top token is 9.2% of PnL and dropping it costs 0.9pp.

It holds out of sample. Across the two forward days 08-19 and 08-20 it books **+5.40%**
(n=156, `P(arm)` 82.7%, median +15.76, placebo z=2.29) where the shallow half books
-0.67% and depth *without* this screen books -3.75%. **Both halves are required.**
Ten days hour-matched: +9.64%, bootstrap CI95 [+3.38, +16.45], placebo z=4.94 against
1,000 same-size draws.

The threshold is a **sign flip, not an optimum**: forward, every `vsol` band at or above
40 is positive and both bands below it are negative. Round 8's suspicion that the edge
lived in a narrow [42,45) band is answered - that band falls from +25.77 to +8.51 forward
and stops being the carrier.

Sizing on this cut decays slowly, because its bigger gross edge pushes the money-optimal
size out to 1.5-3% of the pool: +9.67% at 0.25%, +8.61% at 1%, +6.59% at 2%. The honest
zone is **1-1.5% of pool**, worth 2.4-3.2 SOL/day at 7 of 10 days positive. It is a
narrow, low-frequency cut - about a sixth of the rule's trades.

Open: the exit here is still the shallow rule's (arm 8 / trail 4), tuned on a 34%-arm
population. This one arms 76-83% of the time and its un-armed branch loses 49% against
11%. A stop does not help at any width from 10% to 35% - the collapse is inside a print
gap - so the space to search is arm height, trail width, and the first-gap exit.


### Size it at 1% of pool, not at the measured optimum

Money per trade at the optimal size is `e^2 * vsol / 8 - 2F`, quadratic in the edge, so a
better per-trade number buys a larger position as well as a better one. That is why the deep
cut is worth far more than a fixed-size comparison shows - but the measured optimum is itself
fitted. In selection the deep cells peak at 2.5-3% of pool; forward they peak at **1-1.5%**,
and a cell sized at its own in-sample optimum flips sign (+5.62 SOL/day at 3% in selection,
**-0.29 forward at the same 3%**, +1.27 forward at 1%).

**Size at 1% of pool.** Held-out SOL/day across the size grid:

| pool fraction | 0.5% | 0.75% | **1.0%** | 1.25% | 1.5% | 2.0% |
| --- | --- | --- | --- | --- | --- | --- |
| `rule & vsol>=40` | 1.43 | 1.96 | **2.34** | 2.58 | 2.69 | 2.49 |
| `rule` | 3.28 | 4.17 | 4.46 | 4.16 | 3.27 | -0.26 |

At 1% of a 45-SOL pool that is about 0.45 SOL per entry and roughly 100 entries a day. With
holds capped at 300 s that is well under 1 SOL of working capital, so capital is not the
binding constraint - tape is.

### Adding more filters is a risk knob, not an edge

An exhaustive search of every 1-, 2- and 3-way combination of 55 predicates found nothing
that adds money to this cut out of sample (round 10). Round 9's `nb30 <= 3` costs money at
fixed size - 1.86 SOL/day held-out against 2.34 - and buys risk reduction instead: worst day
-0.10 against -0.89, top 1% concentration 25% against 43%, trades 46/day against 100. Take it
only if the drawdown matters more than the return.

## What it does

| | |
| --- | --- |
| per token, day-block bootstrap | **+3.68%**, CI95 [+2.01, +5.54], P(>0) 100% |
| IS (08-11..15) / OOS (08-16..18) | +3.34% / **+4.21%** |
| days positive | **8 of 8** |
| placebo vs the candidate pool | **z = 9.46**, p < 0.0001 over 2,000 draws |
| tokens / trades per day | 5,982 tokens, 1,124 trades/day |
| SOL / day (sd) | 4.18 (3.86) |
| max drawdown, peak concurrency | 5.53 SOL, **55** positions |

Latency-tolerant and improving with delay: +3.68 / +3.19 / +3.48 / +3.81 / **+5.85%** at 0 to 4
prints late, win rate rising 35.0% to 41.3%. Nothing here is a race.

Cost tolerance: **+3.68%** at the current 0.0002 tip, +3.14% at 0.0005, **+2.51% at 0.0010**
(7 of 8 days), +1.57% at 0.0020 with a CI that touches zero, and +0.33% at 0.0040. Break-even
sits near a 0.0045 SOL/leg fixed cost.

## The shape, stated plainly

The typical trade loses money and the basket is carried by a handful of tokens.

- win rate **35.0%**, median token **-4.41%**
- top 1% of tokens (59 of 5,982) = **64.5%** of total PnL
- winsorise at p95 and +3.68% becomes +0.79%; at p90 it is **-0.96%**
- drop the 10 best tokens and +2.92% survives
- rug rate (token ends below -50%) 3.3%

**The left tail is not filterable.** A stop loss changes the result by 0.00pp because the
collapse happens inside a print gap. `a_deficit` separates rugs 25-fold but excluding its top
bucket removes the edge with them - it selects variance, not direction. Filtering to thin curves
removes rugs entirely and halves the edge. Risk and return sit on one axis here, so the lever is
**position size**, not selection.

## Before running it live

- Paper first, for at least 8 days. A losing first week proves nothing about a rule that loses
  65% of its trades by design.
- Concurrency peaks at 55 positions, about 5 SOL of exposure. Cap it deliberately.
- Daily PnL is +4.18 SOL against a standard deviation of 3.86 and a worst day of -1.31 SOL over
  a max drawdown of 5.53 SOL. The bankroll decision is the real decision.
- 8 days is the entire sample. That is the binding limit on every number above.
