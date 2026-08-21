# Round 9 (2026-08-20): entry depth forward-tested on a second later day

Round 8 found that entry depth lifts `P(arm)` and produces the program's first
body-driven shape, and left it explicitly as a lead: one fresh day, a threshold that
looked like a hump rather than a monotone, and 55% of its edge inside a 3-SOL band with
no cohort behind it.

2026-08-20 arrived as tape while that record was being written. It is a genuinely later
day, unseen by every cutoff and every threshold choice. **The depth cut holds on it, and
the specific band that looked fragile is the part that did not survive - which is the
right way round.**

## The day, and what it is not

Ingest is healthy: hourly trade counts on 08-20 track 08-19 within about 10%
(115,568 vs 104,447 at hour 0, 58,508 vs 68,741 at hour 10) with no gap. The tape ends
14:59:59 UTC, so **08-20 covers 00-14 UTC only** and misses 15-23, which are the busiest
hours of every other day. Entries are capped at 14:50 so every path gets its full 300
seconds.

That partial coverage is a real confound - round 7 found the 00-05 UTC block runs hotter
than the rest - so **every cross-day comparison below is hour-matched to 00-14 UTC**, on
all ten days, unless it says otherwise.

The pipeline is rebuilt for the day exactly as round 7 built 08-19: `iv.sp10` (warm-up
from 08-19 00:00), `iv.dp10`, `iv.wide10`, and `iv.q10x` through the same verified
engine. `iv.wfs` needed a second backfill - **16,180 wallets whose first-ever trade falls
on 08-20** were absent and would have read as not-fresh. The resulting pool is 1,801
tokens with `c_fresh1h` complete on every row and 26.3% deep at the decision slot,
against 26.5% over the sealed days.

Cutoffs are the live recipe applied blind: the 80th percentile per age band over the
last seven sealed days, now 08-13..08-19.

## 08-20 on its own

| | n | `P(arm)` | net | median | win |
| --- | --- | --- | --- | --- | --- |
| pool | 1,754 | 35.3% | **-2.25%** | -4.67 | 29.5% |
| rule, all | 375 | 38.7% | **+1.42%** | -4.33 | 32.8% |
| rule, shallow | 312 | 29.5% | +0.41% | -5.14 | 25.3% |
| rule, **deep** | 63 | **84.1%** | **+6.41%** | **+16.46** | **69.8%** |
| pool, deep | 244 | 67.6% | -4.46% | +2.99 | 53.3% |

The rule clears on 08-20: +1.42% against a -2.25% pool is a **+3.67pp edge**, inside the
eight-day range (mean +2.84, minimum +1.31) that 08-19 fell below. One bad day and one
good day is what a 32%-win lottery looks like; it is not a regime break.

## The depth cut, ten days, hour-matched

| day | n | rule & deep | rule, all | pool | `P(arm)` deep |
| --- | --- | --- | --- | --- | --- |
| 08-11 | 50 | -0.72 | +3.89 | +0.77 | 68.0 |
| 08-12 | 60 | +21.67 | +7.06 | +2.69 | 71.7 |
| 08-13 | 59 | +29.40 | +6.63 | +2.12 | 81.4 |
| 08-14 | 68 | -4.69 | +0.58 | -0.49 | 54.4 |
| 08-15 | 51 | +22.03 | +4.56 | -0.35 | 62.7 |
| 08-16 | 45 | +7.88 | +2.61 | -1.35 | 66.7 |
| 08-17 | 47 | +1.35 | +1.58 | -0.42 | 80.9 |
| 08-18 | 49 | +11.74 | +1.35 | -1.60 | 91.8 |
| **08-19** | 93 | **+4.72** | -0.29 | -0.61 | 81.7 |
| **08-20** | 63 | **+6.41** | +1.42 | -2.25 | 84.1 |

**8 of 10 days positive, and both forward days positive.** Pooled: n=585, net **+9.64%**,
median +13.88, win 65.6%, `P(arm)` 74.5%. Day-block bootstrap CI95 **[+3.38, +16.45]**,
P(>0) 100%. Placebo against 1,000 same-size draws from the rule with day counts
preserved: **z = 4.94**, beats 1000 of 1000.

Treating the two forward days as one out-of-sample block: n=156, net **+5.40%**, median
**+15.76**, `P(arm)` **82.7%**, against +0.51% for the rule on the same two days -
placebo **z = 2.29**. (A bootstrap over two day-blocks is not a real interval and is not
quoted.)

On the nine complete days with **all hours**, the picture is the same and larger:
`rule & deep` n=971 (108/day), `P(arm)` 75.9%, net **+7.24%**, median +12.99, win 67.9%,
**8 of 9 days positive** (only 08-14 negative, at -2.40). On 08-19 all-hours it books
+2.49% where the rule as a whole books -0.34%.

### Both halves are required, and that now holds forward

On the two forward days:

| | n | `P(arm)` | net | median |
| --- | --- | --- | --- | --- |
| pool only, deep | 354 | 65.3% | **-3.75%** | +2.74 |
| pool only, shallow | 2,363 | 32.2% | -1.74% | -4.69 |
| rule, shallow | 646 | 26.9% | **-0.67%** | -6.00 |
| rule, **deep** | 156 | **82.7%** | **+5.40%** | +15.76 |

Depth without the fresh-wallet screen loses. The screen without depth loses. Only the
intersection pays, and **the rule's entire forward value is in its deep half** - the
shallow 80% of its trades books -0.67% over the two days.

### The band was noise; the threshold was not

Round 8's sharpest worry was that 55% of the edge sat in [42,45) with no mass point in
the `vsol` histogram behind it. The forward days answer it:

| band | n (10d) | net | n (fwd) | **net, forward** |
| --- | --- | --- | --- | --- |
| [30,35) | 2,732 | +1.42 | 595 | **-0.45** |
| [35,40) | 263 | +1.03 | 51 | **-3.34** |
| [40,42) | 76 | +9.37 | 30 | **+12.64** |
| [42,45) | 156 | +25.77 | 25 | **+8.51** |
| [45,50) | 172 | +0.14 | 30 | **+2.92** |
| [50,+) | 181 | +4.88 | 71 | **+2.29** |

Forward, **every band at or above 40 is positive and both bands below 40 are negative**,
while [42,45) drops from +25.77 to +8.51 and stops being the carrier. The in-sample hump
was noise concentrated by a convex payoff; the sign flip at 40 is the real structure. A
threshold chosen in-sample is confirmed by data that did not choose it.

Concentration on the ten-day deep cell: top token 9.2% of PnL, top three 24.1%, and
dropping the single best token moves +9.64 to **+8.76**. Body-driven, on a larger sample
than round 8 measured.

## What it pays

Size sweep on `rule & deep` across all ten days, hour-matched, paths rescaled and
triggers re-fired:

| size (% of pool) | mean size | net %/token | SOL/day | days positive |
| --- | --- | --- | --- | --- |
| 0.25% (current) | 0.133 | +9.67 | 0.70 | 8/10 |
| 0.50% | 0.266 | +9.35 | 1.34 | 7/10 |
| 1.00% | 0.531 | +8.61 | 2.44 | 7/10 |
| 1.50% | 0.797 | +7.61 | 3.19 | 7/10 |
| 2.00% | 1.063 | +6.59 | 3.62 | 7/10 |
| 3.00% | 1.594 | +4.68 | 3.72 | 6/10 |

Net % decays far more slowly with size here than on the rule as a whole (9.67 -> 8.61 at
1%, against 3.67 -> 2.50), because a bigger `e` moves the money-optimal size out to about
1.5-3% of the pool. The honest zone is **1-1.5%**, where SOL/day is 2.4-3.2 and day
consistency holds at 7 of 10.

Note the trade count: 58 per day hour-matched, 108 per day all-hours. This is a **narrow,
low-frequency cut** - about a sixth of the rule's trades - so its SOL/day is modest even
though its per-trade edge is the largest in the program.

## Crossing the earlier rounds' features with the deep cell

The depth cut was measured against the fresh-wallet screen and nothing else, so the
obvious question is whether any of the seven earlier rounds' features add to it. They were
all measured on a population that is 85% shallow with a 34% arm rate; the deep cell arms
76% of the time, so a refutation there is not a refutation here.

All 19 features carried on `iv.dp`/`iv.wide` were split at their within-cell median and
read across IS, OOS and the forward block. **38 tests on n=1,034** - the multiplicity is
real and the survivors are read with that in mind.

Two results are disqualifications rather than findings:

- **`alive` is look-ahead and must never be used.** It is
  `count(*) filter (where n_sell > 0)` over `range between 60 seconds following and 360
  seconds following` - it counts future selling. It reads +19.71% against -4.77%
  consistently in every window because it is an oracle for "the token did not die".
- **`uwb30` is `nb30`** (r = +1.000 on this cell). One signal, not two.

The rest are near-orthogonal (all pairwise |r| < 0.05), and three survive all three
windows in the same direction. **All three say the same thing: inside the deep cell, buy
the quiet ones.**

| selection | n | all | IS | OOS | FWD | median | arm |
| --- | --- | --- | --- | --- | --- | --- | --- |
| deep (baseline) | 1,034 | +7.19 | +8.62 | +7.03 | +3.70 | +13.16 | 76.4% |
| deep & `nb30` <= 3 | 527 | +11.73 | +15.23 | +9.41 | +5.08 | +15.95 | 74.8% |
| deep & `f_impulse` <= med | 517 | +9.14 | +11.48 | +7.46 | +5.19 | +14.70 | 74.1% |
| deep & `f_r10` <= 0 | 796 | +8.27 | +10.17 | +7.24 | +3.99 | +13.48 | 76.8% |
| **deep & `nb30` & `f_r10`** | **450** | **+12.91** | +16.52 | +8.71 | **+7.15** | **+16.51** | 74.9% |
| deep & `nb30` & `f_impulse` | 429 | +11.93 | +14.38 | +11.35 | +6.55 | +16.85 | 75.3% |

The stack books **+12.91%**, median +16.51, win 68.7%, **9 of 10 days positive**,
day-block bootstrap CI95 **[+7.56, +18.40]**, and placebo **z = 2.73** against 1,000
same-size draws from the deep cell itself.

**This rehabilitates the lull.** Round 1 found the scalpers buy into silence and that it
never pays universe-wide; round 5 found holder concentration reads backwards. `nb30 <= 3`
- fewer than four buy prints in the trailing 30 seconds - is that same signal, and inside
the deep cell it is worth +4.5pp. It failed before because it was measured on the wrong
population, not because it was wrong.

Two cautions. IS decays to OOS (16.52 -> 8.71), so part of the lift is fitting. And the
trade count falls from 108/day to **45/day**, so at 1%-of-pool sizing SOL/day moves only
2.44 -> about **2.87** even though net per trade nearly doubles - this buys quality, not
throughput.

`a_deficit`, `b_wall`, `b_uwz` and `a5_uwshare` exist only for 08-11..18, so they have no
forward test at all. `b_wall` is the loudest of them (low +16.15 against high -0.07 across
IS and OOS) and is the obvious next thing to rebuild on the later days.

## What is still open

- **Two forward days, 156 trades.** That is the entire out-of-sample sample for the cut.
- **08-20 is 00-14 UTC only.** The busiest third of a day is unmeasured on it.
- The threshold of 40 is confirmed as a **sign flip**, not as an optimum; where exactly it
  sits, and whether it drifts with venue conditions, is unmeasured.
- The exit is still the shallow rule's exit. `P(arm)` is 75-83% here against a 66%
  break-even, and the un-armed branch loses about 49% - re-deriving arm/trail/TP on this
  selection is the obvious next gain and has never been done.
- Depth alone remains negative (-3.75% forward), so nothing here says buy deep curves.

## Scratch

`iv.sp10`, `dp10`, `pa10`, `cf10`, `wide10`, `cut10`, `rsel10`, `psel10`, `q10x`,
`dv9`, `all10`, `dsz`, and function `iv.eng10`. All safe to drop. `iv.wfs` is *modified*,
not scratch - 16,180 rows backfilled for wallets first seen on 08-20.
