# Round 8 (2026-08-20): the cost lever, and entry depth

Queue item 1 - cost - is the one lever the signal search never attacked. Nine rounds
measured shapes against a `2.53% + 4*sqrt(F/vsol)` bar and none of them asked whether
the bar itself, or the sizing that produces it, is where the money is decided.

Three results. The protocol fee is immovable and cashback does not touch it. Sizing is
worth about 3x on money and the program has been reading the wrong objective. And the
depth term in the cost formula turned out to carry a signal much larger than the cost
it prices.

## 1. The protocol fee is 125 bps for everyone, including cashback coins

`trades.amount_lamports` is curve-side and excludes the fee, so a trader who types a
round gross amount lands a curve-side amount of `gross * 10000/(10000+fee_bps)`.
Running that test over every buy in the 8-day window rather than dev buys alone:

| hypothesis | non-cashback | cashback |
| --- | --- | --- |
| 125 bps | **27.7%** | **26.1%** |
| 100 bps | 2.3% | 2.6% |
| no fee (the random-hit null) | 4.6% | 4.7% |

9.6M buys. The 100 bps rate sits *below* the null, so it is not a competing mode. The
fee is a flat 125 bps with no dynamic schedule and no cashback discount - cashback is a
creator-side launch flag (account layout plus creator vault), not a taker rebate. That
fixes 2.53pp of the bar permanently.

`trades.fee_lamports` is **100% NULL across all 19.6M legs**. The column exists from
core migration 0005 but nothing writes it, so `F = 0.000225` remains an assumption read
off `JITO_MIN_TIP_SOL` plus the CU price and cannot be checked against the tape.

## 2. Sizing: the program optimises cost percentage, and the money is elsewhere

`B = sqrt(F*vsol)` minimises the size-dependent *cost percentage*. SOL profit per trade
is a different function:

```
SOL(B) = B*e - 2B^2/vsol - 2F         e = gross round-trip move net of the 125bps fee
```

maximised at `B* = e*vsol/4`, worth `e^2*vsol/8 - 2F`. Both sizes break even at exactly
`e > 4*sqrt(F/vsol)`, so **the 3.45% bar is correct as a break-even test**. What is not
correct is reading a rule's *magnitude* at `sqrt(F*vsol)`, which is roughly 0.25% of the
pool where the money-optimal fraction is near 1%.

Measured through the verified round-7 engine on the fresh-wallet rule, rescaling each
path by its new entry reference so the trail and TP triggers move as they really would:

| size | net %/token | SOL/day | arm | days positive |
| --- | --- | --- | --- | --- |
| `sqrt(F*vsol)`, about 0.25% (current) | +3.67 | 2.56 | 40.7% | 8/8 |
| 0.50% of pool | +3.41 | 5.00 | 40.5% | 8/8 |
| **1.00% of pool** | +2.50 | **7.48** | 39.9% | **8/8** |
| 1.25% of pool | +1.96 | 7.50 | 39.5% | 6/8 |
| 2.00% of pool | +0.49 | 4.03 | 38.8% | 4/8 |
| 3.00% of pool | -1.42 | -9.38 | 37.7% | |

The optimum lands where `e*vsol/4` predicts. 1% is the honest choice over the marginally
higher 1.25% because it keeps all eight days positive.

**This is not a free lunch and it does not rescue anything.** The same sweep on the
candidate pool peaks at 0.5% with 1.93 SOL/day and reaches -430 SOL/day at 5%; on the
fresh day, where the rule does not clear, optimal sizing turns -0.07 SOL into **-2.22
SOL**. Size multiplies the sign of the edge that is there. No refuted shape becomes
viable, because scaling does not change a sign.

Three checks that could have killed it, and did not:

- **Exit impact is charged at entry depth.** Repricing every exit leg at the true
  exit-slot depth moves the result *in our favour* (+0.03pp at 1%), and **0% of exits
  exceed 10% of the exit pool** at any size through 3%. The pool grows during the hold,
  so the entry-depth convention is conservative.
- **We stop being a price-taker.** At 1% our order is 0.32 SOL median: the largest buy
  in its slot **64.3%** of the time, and a median **133% of that slot's entire buy
  volume**. The constant-product impact we charge is still exact for the price we pay,
  but the forward path is no longer obviously unaffected.
- **So test it.** Matched within mint x slot-buy-volume decile over 55,920 cells
  (123,109 dominant vs 101,826 shared slots), slots where one buy is >=90% of volume are
  followed by **+6.25pp better terminal and +9.00pp better MFE**. Unmatched they look
  worse (-8.80 vs -5.83) purely because dominant slots are quiet slots (0.58 vs 4.43 SOL
  of buying) - the lull confound. The penalty is not in the tape. The effect cannot be
  claimed either: dominant buyers may be informed and we are not.

Cost of the change, on the same 8 days: total 19.68 -> **58.21 SOL**, peak exposure
0.72 -> **2.86 SOL**, peak concurrency 8 positions, max drawdown 1.02 -> **6.17 SOL**.
Drawdown scales worse than size (6.0x for 4x) because a convex payoff concentrates.

### The tip is nearly irrelevant once size is chosen for money

`F` enters `SOL = e^2*vsol/8 - 2F` only through that last term:

| `F` SOL/leg | at `sqrt(F*vsol)` sizing | at 1%-of-pool sizing |
| --- | --- | --- |
| 0.000025 | 1.00 SOL/day | **7.86** |
| 0.000225 (current) | 2.57 | **7.56** |
| 0.001 | 3.83 | 6.40 |

At `sqrt(F*vsol)` sizing SOL/day *rises* with the fee, which earlier rounds recorded as
"a larger `F` buys a larger position". That is a **sizing artifact, not a fee result** -
`F` was setting the size. Once size is set independently, a 40x range of `F` moves the
result by 19% and dropping Jito for a bare priority fee is worth about **+4%**. It is a
real but small lever, and it is not where the cost work should have gone.

## 3. Entry depth is a signal, not a cost cut

The bar falls as `vsol` rises (`4*sqrt(F/vsol)`), which is why the mandate listed
deeper-curve entry as a cost lever. The cost difference between `vsol` 30 and 50 is
about 0.1pp. What is actually there is 40 times larger.

Pump.fun curves start at `vsol = 30`, and the rule caps token age at 75 slots, so
`vsol >= 40` means **the token took 10+ real SOL inside its first 30 seconds**.

On the rule at the current `sqrt(F*vsol)` sizing:

| | n | P(arm) | net |
| --- | --- | --- | --- |
| `vsol < 40` | 4,899 | 34.0% | +2.65% |
| `vsol >= 40` | 1,084 | **75.5%** | **+8.26%** |

It is stronger at base sizing than at 1%, so it is not a sizing artifact.

**`e_vsol` is measured at the entry slot, which the decision precedes by 1-3 slots, so
filtering on it leaks.** Depth at the decision slot correlates 0.9725 with depth at
entry (median gain 0.118 SOL), but 29.6% of the pool is deep at entry against 26.5% at
decision, and the tokens that cross the line in between are exactly the fast movers.
Re-run on decision-slot depth, which is what a live rule can see:

| | n | `P(arm)` | net | IS | OOS | median |
| --- | --- | --- | --- | --- | --- | --- |
| rule, deep at decision | 829 | **75.4%** | **+8.03%** | +8.61 | **+7.02** | +12.95 |
| rule, shallow | 4,961 | 35.4% | +2.82 | +2.20 | +3.74 | -5.44 |
| pool, deep at decision | 2,382 | 66.8% | -2.34 | -2.50 | -2.06 | +6.31 |

The leak is worth 0.23pp (8.26 -> 8.03) and `P(arm)` is unchanged. **Use decision-slot
depth**; every figure below is the entry-slot form and is 0.2pp generous.

**`P(arm)` is monotone in entry depth across the whole pool** - the wall round 7 could
not move with venue state (|r| <= 0.013, a 1-2pp span):

| decile | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mean `vsol` | 30.2 | 30.6 | 31.1 | 31.6 | 32.5 | 33.7 | 35.2 | 37.6 | 41.8 | 55.5 |
| `P(arm)` | 9.9 | 17.3 | 21.0 | 30.4 | 34.1 | 44.1 | 48.4 | 56.4 | 65.1 | **74.5** |

### It also breaks round 4's lockstep law, and relocates the cost

Round 4's diagnosis was that every arm-rate lift degrades the payoff in lockstep
(corr(arm, net) = -0.39). Depth does not:

| | n | `P(arm)` | net if armed | net if **not** armed | net | p05 | median |
| --- | --- | --- | --- | --- | --- | --- | --- |
| pool, shallow | 14,176 | 33.3% | +23.01 | -13.99 | -1.68 | -34.9 | -6.1 |
| pool, deep | 3,107 | 69.6% | +20.53 | **-51.48** | -1.37 | -64.4 | +5.2 |
| rule, shallow | 4,763 | 33.7% | +26.57 | -11.39 | +1.40 | -27.2 | -6.7 |
| rule, deep | 1,027 | **75.4%** | +25.36 | **-49.19** | **+7.00** | -65.5 | +11.7 |

Depth lifts `P(arm)` by 42pp and costs 1-2pp of payoff-if-armed. The price is paid in
the un-armed branch instead: -11% becomes -49%. Break-even `P(arm)` for rule-deep is
`49.19/74.55` = **66.0%** against **75.4%** delivered - a 9.4pp margin, where round 4
had 73.5% required against 65-73% delivered.

Depth alone is not enough: pool-deep needs 71.5% and delivers 69.6%, so it books -1.37%.
The fresh-wallet screen adds about 6pp of `P(arm)` and 5pp of payoff on top of depth,
and both halves are needed.

### The shape is body-driven, which nothing else here has been

| `vsol` band | n | net | median | win | top-1 share | net w/o best |
| --- | --- | --- | --- | --- | --- | --- |
| [30,35) | 4,587 | +0.61 | **-6.91** | 23.3% | 24.3% | +0.46 |
| [35,40) | 730 | +3.30 | -6.55 | 44.0% | 11.5% | +2.93 |
| [40,42) | 161 | +10.63 | **+8.77** | 59.0% | 15.6% | +9.03 |
| [42,45) | 182 | +23.10 | **+18.35** | 68.7% | 12.2% | +20.39 |
| [45,50) | 442 | +1.96 | +12.41 | 62.4% | **50.6%** | +0.97 |
| [50,+) | 404 | +2.24 | +11.21 | 67.3% | 18.5% | +1.83 |

The median flips sign at `vsol` about 40: below it the typical trade loses 7% and wins
23-44% of the time, above it the typical trade wins 9-18% and wins 59-69% of the time.
For `vsol >= 40` dropping the single best token moves +6.47 to +6.04 and the top three
are 17.4% of PnL. **Every prior survivor in this program was a convexity harvester with
a negative median carried by the top 1%.** This one is not.

Gates: placebo **z = 4.44** against 500 random same-size draws with day counts preserved
(beats 500/500); fresh-day placebo z = 2.22; day-block bootstrap over 9 days
CI95 [+2.46, +10.54], P(>0) 100%; 8 of 9 days positive.

### What is weak about it

- **The threshold is a hump, not a monotone.** Cumulatively on the rule, net rises
  1.99 / 4.45 / 6.10 / **6.47** from `vsol >= 30` to `>= 40`, then falls to 2.10 at
  `>= 45`. 55% of the `vsol >= 40` edge sits in [42,45), n=182. Strip that band and
  `vsol >= 40` is +3.46% against a +1.99% rule baseline.
- **On the pool the threshold is not even monotone** (35: -0.39, 40: +0.65, 45: -0.53,
  50: +1.48) and the split is IS-negative / OOS-positive. Depth is not a standalone
  signal at these exit settings.
- **There is no cohort behind the band.** The entry-`vsol` histogram decays smoothly
  from 30 (5082, 4126, 2188, ... 435, 401, 380, 266, 262, 348) with no mass point at
  42-45, so [42,45) is a slice of a continuum, not a launch-bundle class.
- The whole thing is measured inside a rule that is itself forward-unproven.

A stop does not help: rule-deep books +8.27 with no stop and 7.95-8.07 at every stop
from 10% to 35%, and p05 stays near -62% even with a 10% stop, because the collapse
happens inside a print gap. (A stop *does* move pool-deep from -0.15 to about +1.5, IS
only, unvalidated.)

## Scratch

`iv.fee1`, `q8x`, `q9x`, `rsel`, `psel`, `rsel9`, `psel9`, `szc`, `fz`, `sz8r`, `sz8p`,
`sx8r`, `tr1`, `tro`, `tro9`, `dm`, `armz`, `stp`, `dv`, and functions `iv.sweep`,
`iv.sweep2`, `iv.sweep9`. All safe to drop.
