# 2026-08-23 - Depth-band graduation thesis refuted, and a look-ahead in the LagMs analysis fill

Two results from one campaign. The second matters more than the first.

## 1. `LagMs` prices the fill from a trade that landed AFTER the fill

`vsol` on a trade row is the reserve **after** that trade. Measured on 2,081,639
consecutive same-mint pairs (2026-08-14 cohort):

| test | result |
| --- | --- |
| `sign(dvsol)` agrees with the LATER trade's direction (post-trade) | 99.98% |
| `sign(dvsol)` agrees with the EARLIER trade's direction (pre-trade) | 68.88% |
| median `abs(dvsol) / sol_amount[i]` | 1.0000 |
| first print of a mint reads exactly 30.0 | 0.06% |

So the pool state a transaction executes against at wall-clock `T` is the state left by
the **last** print at or before `T`.

`find_paper_entry_at` / `find_paper_exit_at` with `FillModel::LagMs(ms)` instead select
`position(|t| priced(t) && t.block_time() >= floor)` - the **first** trade at or after
`fire_time + ms` - and price the fill from that trade. That trade landed after us, so its
own price impact cannot have touched our fill.

Measured cost of the difference on the depth-band shape, both legs charged at 115 ms,
same episode set, spot-priced: **+8 to +12 pp per trade in our favour**. It flatters exits
into strength specifically, because a take-profit fires on a rise and the next print is
usually another buy.

Scope: `LagMs` is an analysis-only model. Live paper and sweep run `WorstCase`, which is
adverse and unaffected. Every `LagMs` fill gate run before this date is optimistic by
roughly the amount above.

The correct baseline is cheap: fill at the last print with `block_time <= fire_time + lag`.

## 2. A second, self-inflicted error: model-dependent episode dropping

Each fill model skipped episodes whose fill index ran past the end of the mint tape.
Those skips are not random - they are the tokens that graduate or die immediately after
the trigger. On band 67.5 / TP+75 the same rule scored `+7.03%` with per-model dropping
and `-5.27%` on a fixed episode set. Never drop an episode; clamp the fill index to the
tape end instead.

## 3. The depth-band graduation thesis

Thesis under test: `price = vsol^2 / k` with a hard graduation wall at `vsol` 115.01
(confirmed - 2.686% of mints reach it, nothing exceeds 116). Entering at depth `V` fixes
the ceiling at `(115/V)^2`, so a take-profit bracket should be positive EV wherever
`P(reach 115)` is high enough. That is wallet 63ot's shape, generalised to the market.

Base rates by first crossing into a depth band (FIT, 124,072 mints):

| entry `vsol` | P(reach 115) | mean max multiple |
| --- | --- | --- |
| 32.5 | 3.20% | 2.33 |
| 47.5 | 11.14% | 2.29 |
| 62.5 | 25.50% | 2.06 |
| 72.5 | 36.91% | 1.84 |
| 82.5 | 50.72% | 1.64 |
| 107.5 | 89.84% | 1.18 |

Expected peak is nearly flat across depth: the higher ceiling is cancelled by the lower
probability. There is no free lunch on the depth axis.

Refuted at every gate, spot-priced, both legs charged, no episode dropped:

- **7 entries x 5 bands x 2 cohorts = 70 cells, all negative at 115 ms, 0/5 days.**
  Entries tested: first crossing, random-in-band, dip 10% and 20% off the 30 s high,
  entry on a sell print, negative 2 s flow, dip-and-hot.
- **210 exit cells** (stop in {15, 28, 45, 70, none} x TP in {50, 75, 100, 150, 250, 400,
  none}, plus a graduation hold) all negative on FIT and OOS, and monotone the wrong way:
  tighter stops score better and holding to graduation scores worst.
- The stop is the dominant cost line. A -28% barrier realises **-34%** even at zero exit
  latency, because the tape gaps through it.

### The latency cliff

Net %/trade against entry latency, TP+100 / stop -15, correct fill model:

| cohort | band | 0 ms | 10 ms | 20 ms | 40 ms | 115 ms | 400 ms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| FIT | 67.5 | +6.68 | -1.43 | -1.76 | -2.10 | -2.98 | -4.27 |
| OOS | 67.5 | +7.41 | -1.62 | -2.01 | -2.43 | -3.35 | -5.29 |

Days-positive, OOS band 67.5: `0 ms: 5/5`, every other lag `0/5`.

**10 ms costs 8 pp; the following 390 ms costs 3 pp.** That is not a latency curve, it is
an intra-print artifact. "0 ms" means transacting at the state of the print that triggered
us - being inside that transaction. The prints that follow within a fraction of a
millisecond are same-slot companions, consistent with the earlier finding that 95% of the
money sits in same-slot pairs ~0.49 ms apart.

There is no achievable speed at which this shape pays. Latency reduction does not open it.

## Law confirmed

Every weakness entry carried a **positive** latency tax (dip 20%: +3.4 to +4.5 pp; dip
10%: +1.8 to +3.5 pp; sell print: +1.0 to +2.1 pp) while every momentum entry carried a
negative one (first crossing: -3.7 to -18.1 pp). Latency pays you to buy a dip. It does
not rescue the trade: the dip entries start 8 to 14 pp worse.

## What this does not touch

Wallet 3Xk2 was not examined. The exit-level finding for 63ot (TP+17 -> TP+75) was derived
under `LagMs` and inherits the look-ahead above; it needs re-running before it is trusted.
