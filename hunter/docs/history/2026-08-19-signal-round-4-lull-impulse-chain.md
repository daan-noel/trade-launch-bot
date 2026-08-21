# Round 4 - the D->L->I chain: a real timing signal that does not clear the cost bar

2026-08-19. First round to build the operator's named ideas as **event detectors at slot
resolution** rather than trailing aggregates. The chain separates against a same-token
matched control by +7.70pp and produces the body-driven payoff shape the program has been
hunting since round 1, and it still loses money after costs. The reason is now located
precisely and is new, so the round closes with a mechanism rather than another dead feature.

## What was built

`iv.ev` (7.76M rows, one per active `(mint, slot)`) carries the slot-resolution primitives.
A lull is a **gap in slot numbers** - `iv.sp` holds only active slots, so silence is
directly measurable as `gap1 = slot - lag(slot)` and needs no dense grid.

The chain fires at a single slot when all three hold:

| stage | meaning | v1 threshold |
| --- | --- | --- |
| D - sell-deceleration | the last sell slot is smaller than the one before, into a dip | `s_l1 < s_l2` and `dip <= -0.10` |
| L - lull | silence immediately before this slot | `gap1 >= 3` |
| I - impulse | this slot's buy volume against the token's own recent per-slot mean | `imp >= 2.0` |

`dip = px / max(px over the previous 30 s) - 1`. Entry fills at `dp.e_px`, which was verified
to equal the **next buy-active slot's median price** for 100.0% of 2,854,440 decision points -
the measured p50 latency fill, already charged. Cost is `2.53% + 4*sqrt(F/vsol)` at
`F = 0.000225`, about 3.45% at the chain's median `vsol` of 42.8.

## Pipeline calibration passed first

Per-token blind 60 s hold over 92,066 mints: **-6.20% gross**, which is **-8.73% net** of the
2.53% fixed fee, against the documented -8.6%. The forward-return path does not flatter.

## The chain is a real timing signal

Stages stack monotonically, one trade per token, on median and win rate as well as mean:

| variant | tokens | mean | median | win | median MFE | P(MFE>=8%) |
| --- | --- | --- | --- | --- | --- | --- |
| baseline (all first decision points) | 92,066 | -6.20% | -17.43% | 22.5% | 10.84% | 54.4% |
| D only | 39,497 | -7.19% | -19.51% | 26.5% | 17.93% | 65.4% |
| L only | 55,822 | -6.02% | -13.88% | 27.8% | 11.69% | 56.5% |
| I only | 48,457 | -8.00% | -22.39% | 26.6% | 16.71% | 63.7% |
| D+L+I | 13,808 | **-3.98%** | **-9.27%** | **34.4%** | **13.72%** | **60.5%** |

That baseline row is not a fair comparison - the first decision point of a mint is its creation
slot, median age 0. The honest test is a **same-mint matched control**: for each firing, the
nearest non-firing decision point on the same token 5-20 s away, 12,376 pairs.

| | fire | control | diff |
| --- | --- | --- | --- |
| mean 60 s return | -3.83% | -11.53% | **+7.70pp** |
| median | -10.07% | -18.01% | +7.94pp |
| win rate | 34.9% | 29.8% | +5.1pp |
| mean MFE | 31.19% | 25.12% | +6.07pp |
| P(MFE >= 8%) | 63.4% | 56.9% | +6.5pp |

The edge survives splitting by direction, which is the trap this test exists to catch: a
control drawn **before** the trigger sits above the dip and would manufacture an edge. Control
after: +7.84pp. Control before: +7.60pp. The fire beats its control in 66% of pairs either way.
This is the same magnitude as `64hP`'s +6.67pp, and it is the first event-conditioned trigger
in this program to pass a matched control.

## It produces the target payoff shape

Under the armed trail (arm 8% / trail 4% / no take profit / 300 s), filling at the next print
past the trigger, on 13,786 tokens: **median net +5.67%, win rate 59.7%** - positive median,
body-driven, the shape section "A signal is not a screen" names as the target. The blind 300 s
hold on the same entries is -10.36% net at a -29.26% median, so the trail is worth about 6.8pp.
`63ot`'s fixed TP+17/SL-28 bracket on the same entries is **-8.93%**, far worse than the trail,
and the stop loss contributes almost nothing - consistent with the standing finding that a
price stop is inert here.

## Why it still loses: the arm/never-arm decomposition

Mean net is **-3.54%**. Splitting on whether the position ever reached the 8% arm threshold:

| | share | mean net | median | win | contribution |
| --- | --- | --- | --- | --- | --- |
| arms (reaches +8%) | 65.2% | **+13.00%** | +13.66% | 78.4% | +8.49pp |
| never arms | 34.8% | **-36.04%** | -34.95% | 0.5% | -12.54pp |

The whole trade is one number: **P(arm)**. Break-even sits at `36.04/(13.00+36.04) = 73.5%`,
and the chain delivers 65-73%. Two attacks failed:

**A time bail on the un-armed state does not recover it.** Un-armed positions otherwise ride
the full 300 s with no exit condition at all, which is a design hole, but closing it only
trades less: bail at 120/60/30/15/6/3 s moves net from -3.85% to -3.84/-3.79/-3.60/-3.26/
-2.57/-2.05% while the median falls from +6.22% to -2.38% and the win rate from 63.0% to 40.1%.
Gross at the tightest bail is +1.38% against a 3.43% cost - the "converges on do not trade"
pattern. The collapse is too fast for a clock, the same reason a price stop fails.

**Raising P(arm) degrades the payoff in lockstep.** `vsol >= 50` lifts the arm rate to 78.1%,
above the 73.5% break-even, and still nets **-3.94%**: the armed mean falls to +10.82% and the
un-armed loss deepens to -56.61%, moving break-even to 84%. Every axis behaves this way. Risk
and return on a single axis was previously inferred from filter results; here it is
demonstrated directly on the mechanism.

## The one axis that moves both sides

Lull length is the exception. Longer silence raises the armed payoff **and** shrinks the
un-armed loss at the same time:

| `gap1` | tokens | arm % | armed net | un-armed net | net |
| --- | --- | --- | --- | --- | --- |
| 3-5 | 9,520 | 67.3% | +11.89% | -39.05% | -4.75% |
| 6-10 | 2,889 | 61.8% | +13.45% | -32.83% | -4.24% |
| 11-15 | 690 | 58.0% | +19.52% | -27.48% | -0.23% |
| 16-20 | 320 | 57.5% | +16.93% | -24.94% | -0.86% |
| 26-30 | 83 | 60.2% | +14.40% | -22.30% | -0.19% |

Note the arm rate **falls** while net improves - further evidence that arm rate alone is the
wrong objective.

## Where it lands, honestly

Stacking the survivors - `gap1 >= 11`, `dip <= -0.10`, `imp >= 2`, `is_cashback_enabled` off -
gives 1,122 tokens over 8 days. A 12-configuration exit grid tuned on that selection:

| exit | gross | net | median | win | days positive | IS | OOS |
| --- | --- | --- | --- | --- | --- | --- | --- |
| arm 8 / trail 4 / 300 s | +3.01% | -0.40% | -0.22% | 49.5% | 4/8 | -1.03% | +0.42% |
| arm 8 / trail 4 / 120 s | +3.65% | **+0.24%** | -0.98% | 47.3% | **5/8** | +0.08% | +0.44% |
| arm 15 / trail 8 / 300 s | +4.25% | +0.84% | -1.40% | 47.2% | 3/8 | +0.85% | +0.83% |
| arm 20 / trail 10 / 300 s | +4.28% | **+0.88%** | -3.04% | 46.3% | 3/8 | +1.31% | +0.31% |

Nothing clears the bar with day consistency. The best mean is 3 of 8 days positive on 140
tokens per day; the best day count is 5 of 8 at +0.24%. Adding `dip <= -0.20` reaches +1.55%
mean on 445 tokens, and it is the best of roughly twenty-four cells inspected, with a negative
median - a tie-break gate candidate, not a result.

**Verdict: the mechanism is confirmed and the rule is refuted.** The chain is not another dead
feature; it separates against the strongest control available and reshapes the payoff. It sits
roughly 1pp of gross short, in the same place `63ot` sits at -0.08% net - which is now the
second independent measurement putting this family of trade just under the fee.

## Second half: attacking the un-armed third at entry

The decomposition names one target - separate, at entry, the third that never bounces. Two
things were built for it.

**Seller inventory exhaustion (`e_left`), the stock form of sell-deceleration.** `iv.wc` carries
a running per-`(mint, wallet)` position, so for the wallets that sold in the 75 slots before the
trigger, the share of what they bought that they still hold is O(1) to read. The mechanism: a
seller who is already flat has no more supply to hit the bounce with. This is the first stock
feature built against the bounce rather than against terminal return, and it is a **strong,
clean, monotone predictor of it** - and the sign is opposite to the hypothesis. Sellers who are
already empty mark an abandoned token, not a finished dump.

| `e_left` | tokens | arm % | armed net | un-armed net | net |
| --- | --- | --- | --- | --- | --- |
| exactly 0 (sellers flat) | 4,408 | 55.5% | +14.25% | -27.34% | -4.27% |
| < 0.02 | 2,092 | 63.5% | +13.83% | -33.33% | -3.37% |
| 0.02-0.06 | 1,528 | 70.1% | +13.65% | -42.08% | -3.02% |
| 0.06-0.15 | 2,479 | 70.8% | +11.78% | -45.13% | -4.82% |
| 0.15-0.35 | 2,446 | 72.4% | +10.96% | -46.90% | -5.03% |
| >= 0.35 | 836 | 73.9% | +11.42% | -47.20% | -3.87% |

An 18-point swing in the arm rate, and net is negative in all six buckets.

**The general test.** Fifty-four buckets over nine unrelated axes - seller inventory, lull
length, dip depth, curve size, impulse strength, buyer breadth, token age, seller count and
calendar day - each with at least 250 tokens:

| | |
| --- | --- |
| arm rate range across buckets | **22.2% to 81.1%** |
| buckets with positive net | **2 of 54** |
| net range / mean / sd | -6.31% to +0.52% / **-3.88%** / 1.62 |
| correlation of arm rate with net | **-0.39** |

A 59-point swing in the probability of the bounce buys no improvement in net, and the
correlation runs the wrong way. **Entry selection is closed for this trade shape**, now
including the stock space.

**A framing that had to be discarded.** Reading each bucket as "the arm rate the payoffs require
to break even" against "the arm rate delivered" produces a deficit that looks impressively
constant at -7.59pp, sd 3.26, with required tracking delivered at correlation 0.947 and slope
1.067 - the signature of a priced quantity. It is an artifact: `deficit = net / (armed net -
un-armed net)` is an exact algebraic identity, confirmed numerically at correlation 0.99997. The
deficit is a rescaling of net, not independent evidence about pricing. It is recorded here
because it survives every plausibility check and would have published cleanly.

**The lull exception does not survive.** Re-run over the full superset rather than the base
thresholds, where the long-lull buckets carry real sample, the effect is gone: `gap1` 12-17
nets -1.41% (3 of 8 days), 18-29 nets -2.91%, 30-59 nets -4.26%. The earlier positive long-lull
cells depended jointly on `dip <= -0.10` and `imp >= 2` on a few hundred tokens. Lull length
remains the flattest axis; it is not a profitable one.

## What this changes for the next round

- **Rank an entry candidate on P(arm) jointly with the armed and un-armed payoffs, never on
  arm rate alone.** Three of the four cells that raised the arm rate lowered net.
- **A lull is a slot gap, not a low-activity aggregate.** `iv.sp` holds active slots only, so
  the measurement is exact and free. Lull length is the only axis found that improves both
  sides of the payoff, and it is under-sampled above `gap1 >= 11` (1,380 tokens in 8 days).
- **The un-armed third is an entry property, and entry selection is now closed against it.** No
  exit reaches it - not a price stop, not a clock, not a tighter arm - and no entry feature
  does either, across 54 buckets on 9 axes including a purpose-built stock feature. Predicting
  the bounce is not the problem; the payoff moves with the prediction.
- **Predict the bounce and you have predicted nothing.** Rank a candidate on **net**, and treat
  any improvement in arm rate as neutral until net moves with it.
- **Check whether a ratio is an identity before believing it.** The break-even-arm-rate framing
  looked like a pricing discovery and was arithmetic.
- The exit grid prefers a **wider** arm and trail on this selection (arm 15-20 / trail 8-10),
  the opposite direction from the fresh-wallet rule's arm 8 / trail 4. Exits do not transfer
  between selections.
- **Ingest into the workstation is 18.5 h stale** at the time of writing - the latest `trades`
  row is 2026-08-18 21:03 UTC. The 8-day sample cannot be extended until `db-incremental-sync`
  runs, and that sample is the binding limit on everything above.

## Where the data is

Schema `iv`, Postgres `hunter_bot`. `ev` 7.76M slot-resolution primitives; `ch` 2.83M events
joined to outcomes; `mc` 12,376 matched-control pairs; `en2` 94,260 superset firings with
`out` their per-entry trail outcomes; `pw4` the long-lull path set. About 3 GB, droppable.
