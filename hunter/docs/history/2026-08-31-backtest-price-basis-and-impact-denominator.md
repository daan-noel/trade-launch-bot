# 2026-08-31 — simulate priced the wrong reserve and the wrong basis

Two independent pricing defects in the lab backtest, found while grading the
crowd-island rule against the SQL derivation it came from. Both are fixed. Every
`sim_results` run stored before this date is priced under the old model and does
not compare — the same trap the 2026-07-28 cost-model constants left behind.

## 1. Impact charged on the real reserve

`lab/src/strategies/replay.rs` cached `trade.reserve_sol` as the denominator the
cost model divides notional by. That field is the **real** reserve (`vsol - 30` on
the curve) because `liquidity` and the deadness verdict mean real deposited SOL.
Price impact is `B / vsol`, so the denominator must be `priced_reserve_sol`.

`TradeLite::priced_reserve_sol` already said so in its own doc, and the grouped
sweep already read the priced one — so simulate and the sweep disagreed about the
same rule at the same size, each reading the other as wrong.

Overcharge factor is `vsol / (vsol - 30)`: 1.6x at `liquidity 50`, 11x at
`liquidity 3`. Worst exactly where the shallow-pool rules trade.

Measured on crowd-island Rule A, 5,208 entries at 0.10 SOL:

| | mean/trade | SOL |
| --- | ---: | ---: |
| fee only (no impact) | +6.28 % | 32.70 |
| impact on the real reserve | +1.66 % | 8.63 |
| impact on the priced reserve | +5.73 % | 29.85 |

A 4.62 pp charge where the correct one is 0.55 pp.

`replay::impact_denominator_guard::impact_is_charged_on_the_priced_reserve` pins
it, and names the numbers so a regression reads as a defect rather than a result.

## 2. Fills priced off a counterparty's execution price

`TradeLite::price` and `Fill::price` were the fill print's `price_per_token` — what
**that** trader paid, averaged along their own segment of the curve. For a buy that
sits below the post-trade spot they left behind; for a sell, above it. So pricing an
entry off a buy print and an exit off a sell print flatters both legs by that
trader's own impact.

What a later trade lands into is the reserve pair they left. The cost model then
charges our impact on top (`notional / vsol`), which is precisely the conversion
from a spot basis to an average paid — so the basis has to be spot, or the impact
term is applied to the wrong number.

The fix routes every price the engine folds through `TradeRow::fill_basis`
(`chart_spot_price`, the reserve-pair spot already canonical for the chart), on all
three adapters and in `paper_fill`. A REAL fill (`exec_real`) keeps its execution
price: that one is our own transaction and is what we actually paid.

Measured on the same fire set, re-booking the derivation on each series:

| basis | mean/trade | SOL |
| --- | ---: | ---: |
| reserve-pair spot | +2.57 % | 12.02 |
| counterparty execution price | +4.52 % | 21.13 |

1.95 pp a round trip, all of it optimistic.

## What it cost to find

The two defects push in opposite directions, so the engine's bottom line looked
plausible while both were live. Only a trade-by-trade comparison against an
independent book exposed them: on the entries the engine and the derivation shared
at the same instant, the engine read **+0.07 %** against the derivation's +3.03 %.
After both fixes the same subset correlates at **r = 0.979** and agrees on the sign
of 96.9 % of trades.

A bottom line that looks reasonable is not evidence. Two errors of opposite sign
hide each other, and only a per-trade comparison separates them.

## Fixture fallout

Six `CorpusTrade` fixtures set a `price_per_token` alongside a reserve pair that did
not imply it, so under a spot basis their prices never moved. They now derive
`reserve_token` from `reserve_sol / price`. A fixture whose reserve pair contradicts
its own price is not a simplification — it is a trade that cannot exist.
