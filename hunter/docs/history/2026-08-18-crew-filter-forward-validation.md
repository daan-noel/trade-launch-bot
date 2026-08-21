# 2026-08-18 — Crew-share filter: clean forward window, and the per-entry trap a second time

Scores the `crew<20%` copy rule on 08-17..08-18, two days it was never fitted on.
Selection survives decisively. Per-day profitability does not clear zero.

## 1. The windows

| window | dates | what was fitted there |
| --- | --- | --- |
| IS | 08-01..08-08 | independent/crew wallet labels, the 91-wallet `reach` roster |
| fit | 08-09..08-16 | the `crew<20%` threshold |
| forward | 08-17..08-18 | nothing |

`mx2` carries the forward price grid; `ev2`/`evc2` classify forward buyers with the frozen
IS labels. Label coverage holds 9 days out: 41.0% crew, 4.7% independent, 54.3% unknown.

## 2. The per-entry trap, caught a second time

The unfiltered forward book reads +8.41% at 10s against a time-matched control of -8.51%,
which looks like a large edge. It is a weighting artifact. 613 positions sit on 153 tokens:

| entries on one token | tokens |
| --- | --- |
| 1 | 117 |
| 2-9 | 24 |
| 19-23 | 3 |
| 41 | 9 |

Nine tokens carry 369 of the 613 rows. Reach wallets swarm the same token, so per-entry
averaging counts each winner up to 41 times. Per token the same set reads **-4.47%**.

**Rule: one trade per token, always.** A copier gets one entry per token no matter how many
wallets it is copying. The `crew<20%` filter concentrates this — it selects the swarm tokens,
so filtered per-entry reads +22.78% at 96.6% win on what is 20 real trades.

The per-entry threshold curve is monotone (crew<10 +7.52, <15 +5.96, <20 +3.17, <40 -2.42)
and the monotonicity is the artifact. Per token it is not monotone: -1.03 / +3.05 / +4.01 /
+0.54 / -1.71 at the same cuts.

## 3. The rule, one trade per token

Fires at the first `reach`-wallet buy on a token where crew share of prior buyers is under
20%. Entry at `pfirst` of the first printing slot strictly after the signal slot, exit at
`plast` of the last slot within the horizon, 3.3% round trip.

| window | trades | per day | 15s | median | win | 20s | 30s |
| --- | --- | --- | --- | --- | --- | --- | --- |
| fit 08-09..16 | 80 | 10.0 | +2.48% | +13.15% | 58.8% | +5.41% | +1.33% |
| forward 08-17..18 | 20 | 10.0 | +13.03% | +6.00% | 55.0% | +18.05% | +27.60% |

Trade rate is identical across windows. 8 of the 10 days are positive; the two negative days
are 08-13 (-12.69%) and 08-14 (-21.15%).

## 4. Selection is real; per-day profit is not established

Permutation null — draw the same number of trades at random from the same window's token
pool, 3000 draws:

| window | trades | observed | null mean | draws beating observed |
| --- | --- | --- | --- | --- |
| fit | 80 | +2.48% | -3.41% | 2.9% |
| forward | 20 | +13.03% | -4.36% | **0.0%** |

The forward result is unreachable by chance from the tokens the same wallets bought. The
filter picks genuinely better tokens.

Day-block bootstrap over all 10 days, 4000 resamples: mean **+2.10%**, 90% CI
**[-4.03, +7.76]**, 72.5% of resamples positive. Zero sits inside the interval.

Both statements hold. Selection quality and per-day expectancy are different questions, and
only the first one is settled.

## 5. Latency

Filling later returns **more**, in both windows — this is not a race.

| fill | fit | forward |
| --- | --- | --- |
| first print after signal | +2.48% (n=80) | +13.03% (n=20) |
| +1 slot | +2.69% (n=71) | +15.08% (n=17) |
| +3 slots | — | +18.63% (n=13) |

## 6. The binding constraint is the left tail

Pooled 100 fired trades at 15s:

| bucket | n | share | avg | contribution to mean |
| --- | --- | --- | --- | --- |
| < -50% | 13 | 13.0% | -62.7% | **-8.15pp** |
| -50..-20 | 1 | 1.0% | -33.8% | -0.34 |
| -20..0 | 28 | 28.0% | -4.3% | -1.19 |
| 0..+25 | 33 | 33.0% | +16.1% | +5.32 |
| +25..+75 | 24 | 24.0% | +33.6% | +8.05 |
| +75%+ | 1 | 1.0% | +89.6% | +0.90 |

58% of trades land positive. One trade in eight loses about 63% and costs 8.15pp of the
mean — the entire edge. Stops do not repair it: breach and fill land in the same print.

## 7. Status

Reachable, latency-tolerant, 10 trades/day, selection confirmed out of window. Not yet a
positive-expectancy rule. The next lever is the 13% rug bucket, and any filter for it is
derived on 08-01..08-08 and scored on 08-09..08-18 — never on the window it is fitted to.

## 8. Data

`wstudy`: `mx2` (1,890,038 forward slot rows, 48,605 mints), `ev2` (996,412), `evc2`,
`rp2` (613), `rq2`, `mc2`/`mq2` (time-matched control), `t2`, `fire` (100 fired trades),
`poolt`, `pt10`.
