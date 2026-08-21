# Signal round 3: participation breadth, and the tip verdict was wrong

> **RETRACTED 2026-08-19, same day, by a distribution audit.** `a5_uwshare` is **97.8% exactly
> 1.0** on this candidate pool - young tokens with concentrated buy flow almost always have one
> distinct wallet per print, so the feature has no variance where it is applied. Quintiles 2
> through 5 are all identical, and `pd.qcut` on `rank(method='first')` split the ties by row
> order, which in this file is alphabetical by mint address. The "top two quintiles" filter is
> therefore a pseudo-random 40% draw. Twenty genuine random 40% draws give a mean of 3.88% with
> an sd of 0.46% and a range of 3.07% to 4.72%, so the published +4.32% sits **0.96 sd** inside
> the tie-break spread and 5 of 20 random draws beat it. Replacing the filter with its only real
> form, `a5_uwshare == 1.0`, reproduces the no-filter result exactly (+3.68%).
>
> **Participation breadth is not a signal.** What survives is the fresh-wallet filter alone at
> the re-tuned exit: per token +3.68%, CI95 [+2.01, +5.54], IS +3.34 / OOS +4.21, 8 of 8 days,
> placebo z = 9.46, 4.18 SOL/day - a better book than the retracted two-filter version. The
> ranking-inside-the-pool fix, the exit re-tune, the sizing correction to round 2's tip verdict,
> and the left-tail findings all stand. See
> [fresh-wallet-entry-rule.md](../plans/strategies/fresh-wallet-entry-rule.md) for the rule and
> [signal-search-mandate.md](../plans/strategies/signal-search-mandate.md) for the two gates this
> failure earned: check a feature's tie fraction before ranking it, and beat a random draw of the
> same size rather than zero.

Date: 2026-08-19. Round 2 found the project's first honest positive (fresh-wallet share,
+1.26%/trade) and closed with the verdict that the whole edge sat inside the tip budget and
died at a 0.001 SOL tip. Round 3 built the queued A2-A5 and B2 candidates, found a second
signal that stacks with the first, and **overturned that tip verdict on a sizing error.**

## The correction to round 2

The round-2 tip sweep held position size fixed while raising the fixed per-leg cost. Size is
not fixed: the constant-impact optimum is `B = sqrt(F * vsol)`, so a larger `F` buys a larger
position and the round-trip bar grows as `4*sqrt(F/vsol)`, not as `2F/B` at the old `B`.
Re-pricing with size re-optimised at each `F`:

| tip | round-2 doc (fixed size) | corrected (size re-optimised) |
| --- | --- | --- |
| 0.0002 | +1.26% | +1.26% |
| 0.0005 | +0.61% | +0.76% |
| 0.0010 | **-0.47%** | **+0.18%** |
| 0.0020 | not run | -0.65% |

Break-even moves from about 0.0008 to about 0.0015 SOL/leg. The round-2 signal is marginal at
a 0.001 tip rather than dead. **Always re-solve the sizing optimum before reading a cost
sensitivity** - a cost sweep at fixed size overstates the damage.

## Result

Entry = token age under 30 s, buy flow concentrated in one order, `is_cashback_enabled` off,
**fresh-wallet share in the top quintile of the candidate pool**, **participation breadth in
the top two quintiles**. Exit = an armed trail, **arm 8% / trail 4% / TP 15% / 300 s**, filling
at the next print past the trigger. 2,416 tokens, 462 trades/day, 8 days.

| | round 2 | round 3, inherited exit | **round 3, re-tuned exit** |
| --- | --- | --- | --- |
| per token, day-block bootstrap | +1.71%, CI95 [+0.17, +3.55] | +2.74%, CI95 [+1.01, +4.71] | **+4.32%, CI95 [+2.76, +5.88], P(>0) 100%** |
| IS (08-11..15) / OOS (08-16..18) | +1.20 / +1.35 | +3.13 / +2.90 | +4.08 / **+4.70** |
| days positive (of 8) | - | 7 | **8** |
| trades / day | 2,236 | 434 | 462 |
| SOL / day | 2.74 | 1.20 | **1.86** |
| per token at a 0.001 tip | +0.18% | +1.60% | **+3.15%**, CI95 [+1.49, +4.78] |
| top 1% of tokens, share of PnL | - | 78.5% | **59.0%** |

The exit was inherited from round 2 and tuned on a different population. Re-tuning it on this
selection is worth another 1.6pp per token and takes the day count to 8 of 8. The direction is
**longer and looser**: hold 300 s rather than 30 s, arm the trail at +8% rather than +2%, take
profit at +15% rather than +10%. That is the correct shape for a convexity harvester - the old
exit was cutting the winners that pay for everything.

One caution the grid teaches: at **TP 30%** the long holds do show the classic overfit
signature (IS +6.20 against OOS +0.71). At TP 15% the same holds improve **both** windows.
A hold-length result is only trustworthy read jointly with the take-profit.

A placebo over 2,000 random selections of the same size from the same candidate pool puts the
observed per-token mean at **z = 6.05**, with zero draws reaching it. That clears a Bonferroni
bar for far more tests than this round ran.

Latency-positive and increasingly so: +4.32 / +4.21 / +4.40 / +5.18 / **+5.83%** at 0 to 4
price prints of delay, with the win rate rising from 36.7% to 42.5%. Nothing here is a race,
and a slow fill beats a fast one.

Cost tolerance is no longer the binding question. Per token: **+4.32%** at the current 0.0002
tip, +3.74% at 0.0005, **+3.15% at 0.0010** (CI95 [+1.49, +4.78], 8 of 8 days), +2.20% at
0.0020, and still +0.94% at a 0.0040 tip. The round-2 reservation is closed on both counts -
the sizing correction above, and an exit that harvests enough to absorb the fee.

Capacity holds at the longer hold: mean concurrency 1.60 and an observed peak of **27**
simultaneous positions, about 2.5 SOL of exposure. Realised PnL is +1.86 SOL/day against a
standard deviation of 2.50, with a peak-to-trough drawdown of 3.61 SOL and an equity curve
that ends at its high.

## The new signal: participation breadth

`a5_uwshare` is the distinct-wallet share of buy prints over a trailing 25 slots (unique
buying wallets per slot, summed, over total buy prints). High means nearly every print is a
different wallet. Low means a handful of wallets fire repeatedly inside single slots.

The mechanism is the family-C thesis reached through a family-A measurement. Splitting one
order across several prints in one slot costs an operator nothing and is what bundlers and
volume bots do; recruiting a distinct wallet per print costs real money. Breadth is therefore
expensive to fake in a way that instruction shape and fee size are not.

It is the cleanest feature the project has measured. Correlation with log age **-0.046**, with
`vsol` **-0.046**, with holder count **0.002**, with market-wide buy volume **-0.012**, with
`c_fresh1h` **0.028**. Not a clock, not a size proxy, not a regime proxy, and not a restatement
of the round-2 winner. Its twin `a5_perwallet` (prints per wallet in the current slot) runs the
opposite way on 0 of 8 days, which confirms the direction.

## Two method rules this round produced

**A solo screen and a marginal contribution are different quantities.** On the 10% trail sample
`a5_uwshare` showed a quintile gap of 0.40pp and looked dead. Inside the actual rule it is the
best stack member there is, lifting +2.08% to +3.04%. Conversely `a5_mxshare` screened at
7.00pp and turned out to be `f_conc` at a correlation of **0.992** - a filter already in the
rule. **Rank candidates by their marginal contribution to the rule, never by their standalone
screen.**

**The round-2 conditioning trick does not generalise.** `a_deficit` worked because ranking a
response inside its own trigger-size bucket removed the trigger. Applying the identical
construction to A2, A3, A4 and A5 produced `a2_res`, `a3_res`, `a4_res`, `a5_res` - all four
dead, at 1 to 2 of 8 days and non-monotone. For A2 the **raw** `a_giveback` beat its own
conditioned form (7 of 8 days, 1.79pp trail gap). Conditioning is a tool for a response
measured against a trigger, not a general purifier.

## The honest shape: this is a lottery ticket

The rule does **not** pick better tokens in the ordinary sense. Against the unfiltered
candidate pool it lifts the median token by **+0.08pp** and the win rate by **+2.7pp**. Most of
the edge is right-tail concentration:

- top 1% of tokens (**24 of 2,416**) = **59.0%** of total PnL
- median token **-4.22%**, win rate **36.7%**
- winsorise token returns at p95 and +4.32% falls to +1.25%; at p90 it is **-0.68%**
- drop the 10 best tokens outright and **+2.88%** survives

Convexity is a property of this market rather than an artifact of the rule - the unfiltered
pool is more tail-dependent still (top 1% = 1381% of its near-zero PnL). The rule raises the
density of tail events; it does not make the typical trade good. That matches the two wallets
that survived the fee bar in the wallet study, both convexity harvesters with negative median
episodes.

The re-tuned exit materially improves this. Tail share falls from 78.5% to 59.0% of PnL, the
p95-winsorised return goes from +0.06% to +1.25%, and dropping the ten best tokens still leaves
+2.88% where the old exit left +1.42%. It is still a lottery, but a less extreme one, and the
sizing and bankroll question remains the operative one rather than the edge.

## The left tail is not removable: risk and return are the same axis

Rugs (a token ending below -50%) are 4.0% of tokens and cost **-2.52pp** of the +4.32% mean,
so removing them looks like the largest single lever available. Three independent attacks all
fail, and they fail the same way.

**A stop loss does nothing.** At -25% it changes the result by 0.00pp (+4.32% either way); at
-15% it makes things worse (+4.16%); the rug rate barely moves (4.0% to 3.3%) because the
collapse happens inside a print gap and the stop fills below itself. This reconfirms the
earlier finding that the left tail here is an entry property and not an exit one.

**The rug bucket is identifiable, and it is the same bucket as the winners.** `a_deficit` -
round 2's inverted supply-response - separates rugs by a factor of 25: quintiles 1 to 3 rug at
**0.6%**, q4 at 3.1%, q5 at **15.1%**. Since q5 is a fifth of the tokens, it holds about three
quarters of every rug in the sample. Excluding it removes the rugs and **removes the edge with
them**: +4.32% falls to +3.99%, and dropping q4 as well falls to +3.19% for a rug rate of 1.4%.
The mechanism is exactly the round-2 reading. A pop that draws no selling means there is no
real holder base, and a token with no holder base either runs vertically or collapses. It is a
**variance selector, not a direction selector.**

**Liquidity says the same thing in the other direction.** `e_vsol` bottoms at 30, the initial
virtual reserve, so `e_vsol - 30` is real SOL deposited. Filtering to thin curves removes rugs
completely (below 35, a **0.0%** rug rate) and drops the edge to +2.88%. Filtering the other
way, to curves already holding 2 SOL or more, gives per-token **+6.04%** CI95 [+3.52, +8.43],
a win rate of **48.9%**, a median of only -1.61%, and a rug rate of 7.1%.

That last variant is tempting and it does not survive the operator's own currency. Per token it
looks strictly better; in SOL it is worse - **1.53 SOL/day against 1.86**, a higher standard
deviation (2.68 against 2.50), a deeper drawdown (4.28 against 3.61), and an equity curve that
ends 13% below its own peak where the two-filter version ends at its high. The percentage gain
is paid for by losing a third of the trades. **Read a filter in SOL before adopting it; a
better per-trade number can be a worse book.**

The general rule this establishes for the venue: **every left-tail filter found so far is also
an edge filter.** Risk and return sit on one axis, so the lever is position sizing, not
selection.

## Why the trade count fell by a factor of five

The round-2 rule ranked `c_fresh1h` over the whole decision-point universe within
`day x age band`, then intersected with the candidate pool. Young, concentrated launches are
already high on fresh-wallet share, so that top quintile kept **51%** of the pool. Ranking
within the candidate pool instead is the correct construction, keeps 20%, and roughly doubles
the edge on its own (+1.26% to +2.08%) before breadth is added.

## What died

- `a5_mxshare`: `f_conc` under a new name, correlation 0.992.
- `a5_gapcv` (inter-print gap dispersion): 1.89pp on the trail but age correlation 0.326 and
  OOS **-0.77%** against IS +2.32%. A partial clock.
- `a2_res`, `a3_res`, `a4_res`, `a5_res`: the conditioned residuals, 1-2 of 8 days each.
- `a_brk`: breaking the prior 30 s high is **bearish** (1 of 8 days, mean gap -3.35pp),
  consistent with the earlier finding that this venue's momentum signals buy a top.
- `b_ovh` (float already 50% in profit): 0 of 8 days.
- `b_wall` (float that breaks even on a +30% pop, from the holder log-basis moments): stacks to
  +1.40% but its CI spans zero, and breadth beats it.
- `a_giveback`: real on the screen but only 5,770 of 18,499 rows carry a value, and it does not
  survive in combination.

## Where it lands

There is now a two-filter entry and a matched exit with an out-of-sample mean above its
in-sample mean, a bootstrap CI clear of zero on every day of the sample, a placebo z of 6,
latency tolerance that improves with delay, a legible mechanism, and per-token **+3.15%** at a
0.001 SOL tip. The reservation is no longer cost - it is variance. The strategy is a
positive-expectation lottery whose median trade loses money, its left tail cannot be filtered
without filtering the edge, and the decision it needs is a bankroll and drawdown decision
rather than another backtest.

Eight days is the sample. That is the real limit on everything above.
