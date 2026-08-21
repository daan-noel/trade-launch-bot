# Inverted search from tokens: the whole observable universe is 1pp short of the fee

Date: 2026-08-18. Every prior wallet study started from a trader and asked "can I copy
this?". That framing let latency, fills and the wallet's private information bound the
answer. This search inverted it: start from the money, over every token, and ask what was
observable before it moved. No wallet is involved, so nobody else's execution limits the
result.

The answer is a measured negative with a precise size: the best strategy constructible
from every observable in the database earns about **+2.6% gross** per round trip against a
**3.6% cost bar**. It is short by roughly one percentage point, and it is short
consistently in and out of sample.

## Method

Study window 2026-08-11 to 2026-08-18. Earlier days were excluded on ingest grounds, not
market grounds: the `trades` chunks run 100-280 MB/day before 08-11 and 1.7-2.0 GB/day
after, so anything earlier is partial coverage and would have shown up as spurious bag
rates.

Built a slot-level panel (`iv.sp`) of every pump.fun curve token: 7,762,548 slot-rows over
191,235 mints, carrying flow, crowd, router mix, reserves and the executed buy/sell price
percentiles per slot. Slot granularity was deliberate - a grid coarser than one slot
reports "no signal" with full confidence.

Decision points (`iv.dp`): every slot carrying a buy, where a follow-on buy landed within
3 slots so the position was actually fillable. 2,854,440 points over 95,157 mints.

Fills and costs follow the kernel, not a fresh guess: entry at `next_slot_median`, exit at
the last sell print inside the hold window, 125 bps/leg protocol fee, 0.000225 SOL/leg
fixed, and our own `B/vsol` impact on both legs at constant-impact sizing
`B = sqrt(F * vsol)`. That puts the round-trip bar at `2.53% + 4*sqrt(F/vsol)`, about
**3.6%** at vsol 30.

**Validation gate.** The universe blind 60s hold came out at -8.6% mean across the 8 days.
Prior independent work measured -8.23% per token by a completely different route. The
pipeline reproduces a known number before being trusted with an unknown one.

## What the outcome distribution looks like

Only **23.7%** of tokens ever offered a positive reachable 60s trade at any entry in their
life. The median token's single best moment was **-6.7%** after cost - for most tokens
there is no profitable minute, with perfect timing. But 10.8% had a minute worth more than
+50%, roughly 1,000 tokens/day, so the money is real and concentrated.

`net60` is monotone in token age, and so is volatility:

| age | mean net60 | median | green | mean MFE |
| --- | --- | --- | --- | --- |
| <10s | -11.93% | -29.99% | 22.9% | 43.0% |
| 10-30s | -12.51% | -26.66% | 30.7% | 36.1% |
| 30-60s | -10.80% | -15.25% | 36.9% | 30.0% |
| 1-2.5m | -8.75% | -8.06% | 40.5% | 23.6% |
| 2.5-5m | -5.17% | -4.28% | 42.7% | 18.7% |
| 5-10m | -5.08% | -4.14% | 41.0% | 14.2% |
| 10-30m | -3.74% | -3.50% | 39.4% | 10.3% |
| >30m | -2.47% | -3.29% | 34.4% | 6.7% |

Young tokens hold all the movement and all the losses. Old tokens are calm and merely
bleed the fee. Neither end is positive.

## The exit is the biggest lever ever measured here, and it still is not enough

62.2% of entries on tokens aged 10-60s touched +10% at some point inside 60 seconds, and
42.2% touched +25% - while the fixed 60s hold booked -11.8%. The entire gap is exit.

Simulated an armed trailing stop plus take-profit over 281,536 entries on a 10% mint
sample, filling at the **next** print after the trigger rather than at the threshold:

| exit | mean net |
| --- | --- |
| fixed 60s hold | -11.8% |
| arm 5% / trail 10% / TP 30% / 60s | -4.42% |
| arm 2% / trail 4% / TP 10% / 30s | **-3.14%** |

Worth **8.7pp**, the largest single improvement measured in this project. But every
parameter pinned at the tight boundary of the grid, which is the tell: implied **gross** at
that optimum is **+0.19%**. The optimiser was not finding edge, it was minimising time in
the market, and converging on "do not trade". A tight exit alone buys nothing.

## No entry feature survives control

29 features x 10 within-day deciles = 290 cells, all negative under a fixed hold. The
least-bad cell was -1.9%.

Ranking features by within-stratum AUC (strata = day x log-age x vsol x market regime),
every terminal-return separation collapsed to ~0.50. A permutation null over 12 reps put
the noise floor for |separation| at **0.005**, which certifies `f_age` (0.0023),
`f_conc` (0.0019), `f_txi` (0.0018), `f_r10` (0.0015), `f_buyaccel` (0.0006) and `f_r30`
(0.0005) as dead rather than merely unproven. The raw-screen strength of `f_age` and
`f_conc` was entirely the stratification confound.

`f_retail` (share of buys routed through Axiom/GMGN/Photon/BullX/Trojan) separated at
-0.051, ten times the noise floor and statistically real - but it is non-monotone
(mid-retail is the best bucket, low-retail often the worst) and its day-by-day gap flips
sign on 2 of 8 days. Statistically real, practically useless. A rank-based AUC reads
non-monotonicity as separation; check the shape before believing one.

**The one asymmetry that matters:** against **MFE** rather than terminal return, flow does
separate - `b10` +0.062, `f_conc` -0.057, `f_selldecel` +0.043, all far above the noise
floor, and all with the **opposite sign** to their terminal-return effect. Buy flow
predicts that a token will pop while predicting that it will end lower. That is precisely
the structure a trailing exit exists to exploit, and it is why the flow features looked
worthless in the fixed-hold screen.

## New: `is_cashback_enabled` is a real token-level exclusion

46.8% of tokens carry `tokens.is_cashback_enabled`. Tokens with it **off** ran 4.8pp
better on `net60` (-5.52% vs -10.32%) and 1.5pp better under the trail exit. The gap was
positive on **8 of 8 days** (+2.95 to +8.25pp) and present in **every age band**
(+1.86 to +7.74pp). It is a pre-existing binary flag, so no threshold was mined.

## Where it lands

Best constructible combination - cashback off, age under 10s, buy flow concentrated in one
order, tight armed trail:

| | IS 08-11..15 | OOS 08-16..18 |
| --- | --- | --- |
| net | -0.59% | -0.99% |
| gross | +2.94% | +2.56% |
| trades | 936 / 662 mints | 509 / 382 mints |
| green | 28.4% | 25.5% |

IS/OOS consistent, convex (a quarter of trades green with positive gross), about 170
trades/day. And one point short.

The binding constraint is not signal quality. It is the **125 bps/leg protocol fee**: 2.53%
plus about 1.08% in fixed cost and impact at optimal size. A stable +2.6% gross edge exists
and loses to a 3.6% toll. Break-even needs roughly 40% more edge than the entire observable
universe supplies.

This also sizes what the surviving wallets have. `3Xk2` nets +2.62% per turnover, so he
runs near +6% gross - more than double anything constructible here. Consistent with the
model that matched his footprint at AUC 0.862 and still selected losers: whatever separates
his picks is not in this data.

## Two measurement gaps found

- **`trades.fee_lamports` is NULL for all 8 days.** The ingest never decodes it, so
  priority-fee competition is unmeasurable. `tx_index` is the surviving proxy and carries
  no signal (AUC separation 0.0018, below the noise floor).
- **`tokens.meta` holds only `uri`.** The off-chain metadata JSON (name, description,
  socials, image) is never fetched, so no social or narrative feature can be tested
  without an ingest change.

## What this closes

- Token filtering as the lever, from the token side. Prior work closed it from the wallet
  side (winner wallets' selection was negative); this closes it from the token side with
  a permutation-certified null over the full observable feature set.
- A tight exit as a standalone fix. It converges to zero gross.
- `f_retail` and `tx_index` as entry filters.

## What stays open

- The MFE/terminal sign flip is a real, unexploited asymmetry. Flow predicts pops. The
  trail exit tested here was tuned on the full sample and not jointly with a flow entry.
- `fee_lamports` and off-chain metadata are unmeasured, not refuted.
