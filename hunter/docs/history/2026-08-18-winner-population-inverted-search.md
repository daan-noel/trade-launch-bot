# The winning population, found by inverted search (2026-08-18)

First study in this codebase that starts from realized money instead of a hypothesis.
Method: rank every wallet by actual cash extracted, keep the ones that win *every day*,
then ask what they do. Roster 08-01..08-08, walk-forward test 08-09..08-16, honest fills
(`mf.pfirst` of the next printing slot), 3.3% round trip.

**Bottom line.** A consistent winning population exists and is large. Its edge is **entry
price**, not token selection and not exit timing. Its token picks are measurably **worse
than random**. Copying it at reachable fills loses money in and out of sample. The premise
that good rules rest on good token filtering does not survive contact with the people who
actually make money.

---

## 1. The population

Cash flow only - `sum(sell lamports) - sum(buy lamports)` per wallet per day. No marks, no
assumptions. Total across 447,106 wallets over 8 days: **-394,083 SOL**.

| gate | wallets | cash |
| --- | --- | --- |
| all | 447,106 | -394,083 |
| cash > 0 | 111,256 | +371,979 |
| trades >= 6 days, **positive every day**, cash > 5 SOL, >= 50 legs | **876** | **+143,145** |

876 wallets, 0.2% of the population, take 36% of everyone else's losses and never have a
down day.

**Half of that is launch operators.** 50.8% of winner cash comes from tokens the winner
created; 161 winners create 50+ tokens each. Excluding every wallet that ever creates a
token leaves **599 pure traders** holding **+69,129 SOL**, median +49.9 SOL on 143 tokens
and 242 SOL of turnover, a median **+14.64% return on turnover**, positive every day.

## 2. What they do

| | control | pure-trader winners |
| --- | --- | --- |
| token age at entry (p50) | 80 slots | **13 slots (~5s)** |
| age p25 | 17 | **2** |
| buyers already in (p50) | 80 | **16** |
| buy size (p50) | 0.293 SOL | **0.667 SOL** |
| entries at age <= 10 slots | 19.5% | **46.4%** |
| hold time (p50) | - | **12 slots (~5s)**, 64.7% under 10s |

They enter within seconds of launch, near the front of the buyer queue, in larger size,
and they are gone in five seconds.

## 3. Their token selection is worse than random

Blind hold on the same entries, honest fills, 433,717 winner entries against 444,281
control entries:

| scope | group | n | 60s | 300s | median 60s | win |
| --- | --- | --- | --- | --- | --- | --- |
| all entries | control | 444,281 | **-8.16%** | -15.72% | -10.68 | 34.1% |
| all entries | **winners** | 433,717 | **-9.03%** | -14.33% | **-21.20** | **25.9%** |
| age <= 10 slots | control | 85,048 | -11.11% | -18.55% | -28.64 | 20.9% |
| age <= 10 slots | winners | 201,262 | -11.64% | -17.54% | -31.53 | 20.6% |
| first 5 buyers | control | 59,876 | -10.62% | -16.13% | -20.09 | 17.0% |
| first 5 buyers | winners | 132,691 | -12.91% | -17.94% | -28.84 | 18.4% |

**The most consistently profitable traders in the market pick worse tokens than a coin
flip over their own universe.** Their 5-second horizon is no better: -2.02% at 5s against
-2.66% for the control, both below the 3.3% cost.

## 4. The edge is the entry price, and it is unreachable

Comparing each winner's actual fill (`buy lamports / buy tokens`) to the reachable fill a
tape reactor gets:

| measure | value |
| --- | --- |
| their gross return (own buy to own sell) | **+6.23%** |
| reachable fill worse than their fill by | **+5.81%** |
| median gap | +2.55% |
| positions where the reachable fill is >2% worse | 54.3% |

**93% of their gross return is the price they pay, not what they buy or when they sell.**

By token age at entry, with the exit held at their exact chosen moment:

| age at entry | n | entry advantage | their gross | reachable gross | reachable median |
| --- | --- | --- | --- | --- | --- |
| 0-1 slot (bundle) | 104,900 | **+9.31%** | +10.85% | +1.94% | -0.95 |
| 2-5 | 48,909 | +4.51% | +5.68% | +1.89% | -1.86 |
| 6-25 | 115,259 | +4.45% | +5.01% | +1.42% | -1.38 |
| 26-150 | 91,079 | +4.23% | +4.56% | +1.15% | -1.60 |
| 150+ | 70,925 | +5.69% | +3.88% | +1.22% | -1.23 |

Reachable gross is +1.15% to +1.94% everywhere, under the 3.3% cost, with a negative
median in every bucket. **Perfect knowledge of their tokens, their entry slot and their
exit moment still loses.**

## 5. Walk-forward, and the one subgroup that looked alive

Roster selected on 08-01..08-08, measured on 08-09..08-16, 419,203 fresh positions:

| measure | OOS value |
| --- | --- |
| entry advantage | +5.05% |
| their gross | +5.61% |
| **oracle copy (their exact exit price), net** | **-1.60%** |
| oracle copy median | -4.55% |
| days positive | **0 of 8** |

61 wallets clear a positive mean *and* positive median copy in-sample. Tested OOS they
hold: 58 wallets, 4,877 positions, **+7.67% net, median +0.46%**.

**They die the moment the exit is charged one slot of latency.** Buying one slot after
their buy and selling one slot after their sell:

| measure | value |
| --- | --- |
| trades | 4,456 |
| net | **-1.24%** |
| median | **-9.86%** |
| win rate | 37.0% |
| days positive | 3 of 8 |

The oracle's +7.67% is entirely the exit *fill*. They sell into buyers who are still
arriving; one slot later those buyers are gone.

## 6. What this means

The winners are not informed, they are early in the block. Entry advantage of 4-9% against
a reachable fill is not a signal anyone can compute from the tape - it is bundle
membership and slot position. This unifies every refutation in this codebase: each one
tested a rule that pays the reactor price, and the reactor price is exactly the 4-9% that
the winners are collecting.

**Standing consequences.**

- **Do not build or tune token filters as the primary lever.** Measured across the entire
  daily-profitable population, selection quality is negative. Filtering is not where the
  money is in this market.
- **Do not copy-trade any wallet from this population.** The full-population oracle copy is
  -1.60% OOS and negative on all 8 days; the best hand-picked subgroup is -1.24% at honest
  fills. Both ceilings are already measured, so no exit rule can rescue a copy.
- The only lever the data leaves is **entry price itself** - being in the launch
  transaction rather than reacting to it. That is the same place
  `hunter/docs/history/2026-08-18-price-action-space-refuted.md` landed, and it is what the
  one replicated positive in this codebase (the `5ix:BuyExactSolIn` creation-bundle band)
  already is.

## 7. Two accounting facts found while doing this

- **The curve constant is exact: `k = vsol * vtok = 3.219e16`** (SOL x raw token units),
  identical on every row of `trades.reserve_lamports * reserve_token`. Liquidating a bag of
  `B` raw tokens at reserve `V` yields `V*B / (k/V + B)` SOL, not `B * price`.
- **Never mark holders' bags at the last curve price.** Doing so books +952,378 SOL of
  profit across the market; using exact curve liquidation still books +473,562. A token only
  ever holds `vend - 30` SOL, and the whole market holds 889,542 SOL above the floor against
  1,020,036 SOL of independently-marked bags. Per-wallet marks double-count because every
  holder cannot exit first. **Use cash flow for any population-level P&L.**

## 8. Data

`wstudy` additions: `we` (4.88M episode P&L), `wd`/`wsum` (per-wallet-day and rolled-up cash
flow, 447,106 wallets), `win` (876 daily-profitable), `pt` (599 pure traders), `wmc`/`wpx`
(winner positions and fill prices), `fv` (final reserve per mint), `cbw` (cumulative distinct
buyers per mint-slot), `en`/`en2`/`enr`/`ens` (881k entries with causal features and forward
prices), `wcap`, `srv`, `oos`, `copy1`.
