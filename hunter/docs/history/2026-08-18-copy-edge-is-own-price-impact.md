# The daily-profit wallets' edge is their own price impact (2026-08-18)

Follow-on to `2026-08-18-real-traders-after-removing-dev-crew.md`. Twelve wallets pass every
crew gate (creation-slot <=2%, slot-1 <=4%, top-creator <=8%, repeat-creator <=25%, and never
sell a token they did not buy) AND are profitable on all eight days of 08-01..08-08. Six of
them are profitable on all sixteen days of 08-01..08-16. This entry derives where their money
comes from and whether a copier can reach it.

## 1. What they actually do

Real book, 2,750 positions, 9,062 sell legs over 08-01..08-16:

| | median hold | median gain per sell leg | legs closed green |
| --- | --- | --- | --- |
| the three largest | 12-13 s | +18 to +19% | 76-88% |
| the fastest three | 0.3-2.2 s | +7 to +18% | 79-87% |
| the two slowest | 297-329 s | +23 to +29% | 79-82% |

One buy per position, up to 9.1 sell legs. They scale OUT, never in. Every prior study in this
repo assumed a 60-second hold; that is 5x their actual holding period and gives the whole move
back.

## 2. Where the money comes from

The price of the first print AFTER their buy sits 10-19% above the price they paid. That gap
is their entire per-leg gain, and it scales with buy size:

| their buy size | own impact predicted as `buy/pool` | slip actually observed |
| --- | --- | --- |
| <0.2 SOL | 0.14% | 0.14% |
| 0.2-1 | 1.54% | 7.24% |
| 1-3 | 4.26% | 8.58% |
| 3-6 | 7.70% | 16.73% |
| 6+ | 14.54% | 21.45% |

On a constant-product curve a buyer pays the VWAP along the curve and leaves the price at the
marginal rate, so `price_after / VWAP = 1 + buy/pool` exactly. The smallest band matches that
prediction to two decimals, which confirms the mechanism. The larger bands run about double
the mechanical value: the extra is other buyers arriving within the same slot.

**Both halves are already in the price before any copier can act.** The gap is not a signal
they detect; roughly half of it is a gap they create.

## 3. Copying them is negative in both windows

Honest fill at `mf.pfirst` of the next printing slot after their buy, blind clock exit,
3.3% round trip, 2,724 positions:

| hold | 08-01..08-08 | 08-09..08-16 |
| --- | --- | --- |
| 10 s | -1.30% | -2.45% |
| 15 s | -2.58% | -1.59% |
| 20 s | -3.11% | -1.88% |
| 30 s | -4.07% | -3.88% |

Gross of cost the signal is +0.72% (IS) and +1.71% (OOS) against a 3.3% round trip. No exit
schedule closes a gap that size. One wallet reads +10.53% OOS in isolation and is -4.97% on
164 in-sample trades, with its OOS gain carried by days holding 3 to 5 trades.

## 4. The screen this yields

`impact_med` = median of `buy_sol / pool_sol` over a wallet's buys is computable from one
aggregate and predicts how much of a book is unreachable. Across 9,964 independent wallets
with 30+ buys:

| median own impact | wallets | in-sample book | out-of-sample book |
| --- | --- | --- | --- |
| <0.5% | 6,277 | -24.16% | -26.02% |
| 0.5-2% | 2,544 | -20.24% | -21.49% |
| 2-5% | 521 | -13.24% | -11.29% |
| 5-10% | 22 | +1.66% | +1.89% |

Book quality rises monotonically with own impact. **Screen every copy candidate on
`impact_med` before simulating it** — a wallet whose book improves in step with the price it
moves itself has nothing transferable.

## 5. What survives: low-impact wallets plus a crew filter

Selecting on in-sample data only — `impact_med < 2%`, spend >= 5 SOL, in-sample book > +15% —
gives 91 wallets averaging 0.50 SOL per buy and 0.86% own impact. Out of sample they hold up
on their own books: +21.40% on spend, 56 of 72 still positive, +1,197 SOL.

Copying them at the honest fill inverts the usual payoff shape: at a 10-second hold the median
is **+7.37%** and 61.7% of trades close green, but the mean is -2.44%. One position in five
loses ~66% inside 15 seconds and contributes -13.64pp to the mean. Honest per-print stops do
not repair it (-3.32% to -2.27% at best) because the breach and the fill land in the same
print.

Buyer composition at the entry slot does separate it. Classifying every prior buyer as known-
independent, known-crew or unknown, and filtering on the crew share:

| crew share of buyers so far | n | return | win rate |
| --- | --- | --- | --- |
| <20% | 1,430 | **+3.17%** | 74.9% |
| 20-40% | 1,179 | -9.20% | 60.2% |
| 40-60% | 745 | -6.49% | 39.5% |
| 60%+ | 597 | -3.31% | 35.7% |

At `crew < 20%`: +3.61% at 10s, +3.17% at 15s, +4.15% at 20s, median +18.96%, six of eight
days positive, and **latency-flat** — filling one printing slot later returns +3.55%, slightly
better than filling immediately. Tightening to `crew < 15%` gives +5.96% on 698 trades and
`crew < 10%` gives +7.52% on 235.

## 6. Status and the honest caveat

The 91-wallet selection is walk-forward. **The crew threshold is not** — it is chosen on the
same 08-09..08-16 window it is scored on. A day-block bootstrap over those eight days gives
+3.45% with a 90% interval of [-2.99, +9.52] and 81.3% of resamples positive. The in-sample
window cannot arbitrate it: the wallet classification is itself built from in-sample behaviour,
so only 4.8% of in-sample positions pass `crew < 20%` against 36% out of sample.

The rule needs a clean forward window before it earns any capital.

## 7. Data

`wstudy` tables: `indepa` (11,487 walk-forward independent wallets), `crewa` (34,330),
`final` (the 12), `pos12`/`sl12` (their real book), `cpall` (honest copy, both windows),
`imp` (own-impact screen), `reach` (the 91), `rpx`/`rq` (copy with composition features),
`pth` (per-print paths), `evc`/`evcI` (buyer composition by window).
