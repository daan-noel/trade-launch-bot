# 2026-08-27 — lake-wide identity screen, and one candidate

Method: [`cohort-screen.md`](../plans/strategies/cohort-screen.md). Corpus 07-28..08-21,
681,687 tokens, 49.5M curve trades, 578 instruction templates. 193 identities clear
300 mints and 15 days; 3,799 `(identity, band, target)` cells.

## Shortlist

Gated on reachability >= 50%, day-stability >= 60%, >= 150 band arrivals, ranked on
margin over break-even:

| identity | mints | band -> target | pass | break-even | margin | reach | days |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 3ix `mc 0.108` | 1,152 | 60 -> 115 | 36.6% | 13.0% | 2.82 | 69.6% | 24/25 |
| **BuyV2 `mc 1.01`** | 10,323 | 60 -> 115 | 36.2% | 13.0% | 2.78 | 77.8% | 22/25 |
| BuyV2 `mc 15.15` (live) | 930 | 50 -> 115 | 22.2% | 8.5% | 2.60 | 94.8% | 20/22 |
| CB-only, no `mc` | 12,420 | 55 -> 115 | 27.2% | 10.6% | 2.56 | 76.0% | 19/22 |
| BuyV2 `mc 7.07` | 469 | 50 -> 115 | 19.5% | 8.5% | 2.28 | 92.1% | 17/21 |

The `x1.0226 BuyV2` tool runs at 1.01 (10,323 mints), 7.07 (469) and 15.15 (930). Only
15.15 is live, and it is the smallest of the three.

## Stage 2, honest 115 ms fills

Band 60 -> target 115, 40% retrace stop, one trade per token:

| identity | n | gap | hit | mean | median | days + |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| BuyV2 `mc 1.01` | 320 | 2.84% | 6.3% | **−4.60%** | −25.96% | 10/25 |
| CB-only, no `mc` | 150 | 3.25% | 8.7% | **−4.23%** | −23.26% | 13/23 |
| BuyV2 `mc 15.15` (live) | 328 | 17.56% | 19.8% | **+4.01%** | −42.46% | 10/25 |
| **BuyV2 `mc 7.07`** | 125 | 6.05% | 20.0% | **+29.30%** | −19.03% | 17/22 |

Stage 1's ranking does not survive stage 2 for `mc 1.01` — its 36.2% *ever-reaches*
collapses to a 6.3% hit once the 40% stop is priced. **Rank on the screen, decide on the
fill.**

## The candidate: BuyV2 `max_cost = 7.07 SOL`

`[SetComputeUnitLimit, SetComputeUnitPrice, Create_v2, CreateIdempotent, BuyV2]`,
dev buy 6.914 SOL. Enter at the first reachable print with `vsol >= 60`
(`liquidity > 30`), exit at graduation (`liquidity >= 85`) or a 40% retrace.

* **Held out:** +34.06% over 07-28..08-13 (n=84, 12/15 days), **+19.56%** over
  08-14..08-21 (n=41, 5/7 days). Both halves positive.
* **Not one day:** dropping the three best days leaves +19.5% mean on 110 trades.
* **Exit-robust:** stop 25/40/55/70% -> +20.4 / +29.3 / +29.1 / +23.7%;
  target 85/100/115 -> +8.9 / +19.6 / +29.3%.
* **Cheap entry:** `gap` 6.05%, against 17.6% on the live `mc 15.15`.

**Open risks.** 125 trades over 22 days (~5–6/day) and 469 mints total — a small,
fat-tailed sample: median −19.03% with a 38.4% win rate, so the mean lives in the right
tail. Position sizing has to survive long negative runs. 08-14..08-21 is a favourable
window for this template family generally (`mc 15.15` also improves there), though 7.07's
fit half is +34% so regime is not the whole story.

Loaded into Postgres as `-- cand buyv2 mc7.07 band60-grad (screen 08-27)`, **inactive**,
paper, tagged `candidate,stage2-passed,held-out-ok`. Next: `simulate` on the real engine,
which is the arbiter — the screen harness models a target-or-retrace exit only.
