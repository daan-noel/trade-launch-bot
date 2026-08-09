# PnL % — the one definition, and the four places it used to differ

Permanent reference. Read before adding any percent next to a SOL figure.

## The rule

> **A percent shown beside a SOL amount is that SOL amount over the capital that
> produced it.** Never a price ratio, never a mean of other percents.

One function owns it: `hunter_core::strategies::kernel::weighted_return_pct(sum_pnl_sol,
sum_capital_sol)` = `sum_pnl_sol / sum_capital_sol * 100`. Because the denominator is
capital (always positive), **the percent can never disagree in sign with the SOL figure
next to it.** That guarantee is the whole point; anything that breaks it is a bug.

Grain does not change the formula, only the scope of the two sums:

| Surface | Numerator | Denominator | Field |
| --- | --- | --- | --- |
| One position | `realized_pnl_sol` | `entry_sol` | `StrategyPosition::pnl_pct` |
| One rule / run | `Σ realized_pnl_sol` (closed) | `Σ entry_sol` (closed) | `PositionsSummary::return_pct`, `RuleCounters::return_pct` |
| A portfolio window | `Σ realized_pnl_sol` | `Σ closed_entry_sol` | `RulePeriodPnlRow::return_pct`, `PortfolioPerformance::return_pct` |
| Rules TOTAL tile | `Σ total_pnl_sol` across rules | `Σ closed_entry_sol` across rules | derived in `RulesView` |
| Summary hero, realized band | `realized.total_pnl_sol` | `Σ closed_entry_sol` | `bandReturnPct(realized)` |
| Summary hero, MTM band | `Σ realized + Σ open marks` | `Σ total_entry_sol` | `bandReturnPct(mtm)` |

## Both hero ◎ figures carry their own percent

Every run-summary hero — Evidence, Simulate, the sweep drill-in, Console History,
the dry-run card, all of them through `runSummarySections` — prints the return %
inline beside the ◎ it belongs to. One tile, one tone: sign-lock means green ◎ next
to a red % is a bug, so they are rendered as one fact rather than two columns a
reader has to pair up.

**The two hero tiles do not share a denominator, and must not.** `PnL realized`
divides by the closed positions' capital; `PnL incl. open` also carries the open
bags' marks in its numerator, and that capital is deployed too, so it divides by
`total_entry_sol`. The live/paper mapper used to copy `return_pct` onto both bands,
which printed the realized return beside a mark-to-market ◎ — two headline numbers
measuring different cohorts under one label.

`bandReturnPct` is the ONE reader of the capital-weighted-vs-mean choice (hero tile
and band tile below it both go through it, so they cannot print different figures
for the same band). It prefers `return_pct` and falls back to `mean_pnl_pct`, which
is correct rather than approximate: `return_pct` exists only where the notional
varies, and a fixed-notional backtest's equal-weighted mean **is** its
capital-weighted return. The client-side fold (`runSummaryFromRows`) computes a real
`return_pct` whenever its rows carry `entry_sol` — Console History's closes do, the
sweep's `ComboTokenResult` has no entry field at all because there is one notional
for the whole run.

`weightedReturnPct` in `lib/strategy/runSummary` is the TS mirror of the Rust fn and
the only place the division is written on the frontend. It differs deliberately in
one way: no capital deployed returns `null`, not `0` — the backend needs a
total-order `f64` to sort a column by, a tile needs to render `—` rather than assert
an exact break-even nobody measured.

## Why buy size forces this

`strategy_rules.buy_amount_lamports` is editable at any time (`PATCH`), and a run ends
only when its rule goes inactive — so **one run legitimately mixes notionals**. Even
without an edit they vary: `entry_sol` is the *actual* fill, which partial fills and the
revert-retry ladder move off the configured amount.

An equal-weighted mean of per-trade percents is therefore wrong at every aggregate grain:

| Rule | Buy | 10 trades | SOL |
| --- | --- | --- | --- |
| A | 1.00 ◎ | −5 % | **−0.5 ◎** |
| B | 0.05 ◎ | +20 % | **+0.1 ◎** |

Count-weighted: `(−5 + 20)/2 = +7.5 %` green. Capital-weighted: `−0.4 / 10.5 = −3.8 %`
red, matching the −0.4 ◎ actually lost. The Rules TOTAL tile printed the green number
until this landed — which is why `closed_entry_sol` is now on the wire: a per-rule
percent cannot be re-weighted after the fact, only its numerator and denominator can.

## The four defects that were fixed

**1. `pnl_pct` was a price ratio** (`(exit_price − entry_price)/entry_price`), so it
charged no execution cost. A round trip pays 125 bps/leg (measured 2026-07-28) + a fixed
tip per leg + our own impact, so break-even is **roughly a +4 % price move**. Every trade
between 0 % and break-even rendered a green % beside a red ◎. Now
`realized_pnl_sol / entry_sol`, which is measured from lamports that actually moved and
therefore already carries every cost. (Migration `0006` for the view; the model and
`PNL_PCT_SQL` alongside it.)

**2. The same ratio read only the last sell leg.** `exit_price` stamps the final leg
while `realized_pnl_sol` sums them all via `realized_exit_sol`. On a scale-out the two
headline numbers described different trades. Both now read the same scale-out-aware
`CASE`.

**3. `PNL_SOL_SQL` was a third definition.** The positions table SORTED and FILTERED on
`exit_price × exit_token_amount − entry_price × entry_token_amount`, which ignores
`exit_sol_lamports_total` entirely — so the column you sorted by disagreed with the cell
you were looking at on any partially-exited row. Both PnL columns are now built from the
one lamports numerator `PNL_LAMPORTS_SQL`, guarded by
`pnl_sql_columns_share_one_numerator`.

**4. The Rules TOTAL tile weighted by trade count** (the table above), and blended paper
with real — while the PnL tile three lines up already refused to blend modes because
"real ◎ and paper ◎ are different currencies". A percent is money over money, so the
same refusal applies: with both modes visible the tile shows the split, not a blend.

`best_pct` / `worst_pct` moved with `pnl_pct` for the same reason — a distribution tail
the PnL% column cannot reproduce is a tail of some other distribution.

## Naming

`avg_pnl_pct` was renamed **`return_pct`** everywhere (model, repo, wire, TS, columns).
The old name read as "average of percents" and described the formula it had already
stopped being; the next person to "fix" the mismatch would have turned it back into a
mean. Label it **"Return %"** in UI, never "Avg %".

Similarly `total_entry_sol` (all entered positions, open ones included) is **not** a
*realized* return denominator — that is `closed_entry_sol`. Dividing by the former
understates the realized return by the open positions' share of capital. It is the right
denominator for exactly one thing: a **mark-to-market** return, whose numerator already
carries those open positions' marks. Both are shipped; match the denominator to what the
numerator counted.

## What is deliberately NOT capital-weighted

`RunMetrics::mean_pnl_pct` (and its `median` / `p90` / `best` / `worst` / `std` siblings,
persisted to `strategy_run_metrics`) stays an **equal-weighted mean of per-trade
returns**. It is honest under a fixed notional — which is what a sweep and a simulate
always use, so backtest numbers are unchanged and comparable — and the interior
quantiles need per-trade percents anyway.

The residual gap: a **live** run whose buy size changed mid-run has varying notionals, so
its stored `mean_pnl_pct` can differ from the capital-weighted `return_pct`. Closing it
needs a per-outcome notional on `TokenOutcome` (a hot `Copy` struct folded per
`combo × token` in the sweep) plus a new `strategy_run_metrics` column. Not worth it
today because **no live surface uses the run-metrics row as its headline** — every live
percent comes from `PositionsSummary` / `RuleCounters`, which are capital-weighted. If
that changes, do the `TokenOutcome` work first.

Two other percents are correct by construction and unrelated: `WalletMintPnl::realized_pnl_pct`
(already `realized_pnl_sol / cost_basis_matched`) and the sweep/sim `pnl_percent`, which
comes straight out of `round_trip_with_costs` and is a cost-inclusive SOL return.

## Backfill

None, and none is possible. The view is derived, so every historical position's percent
is simply recomputed on read — already-closed rows now show a smaller (and for
sub-break-even winners, a **negative**) percent than they did before. That is the
correction, not a regression. `strategy_run_metrics` rows stamped before `0006` keep
their price-based figures: they were computed at rollup time and cannot be re-derived
from a view. A run finalized before the migration is not comparable to one finalized
after, the same way pre-2026-07-28 cost runs are not comparable to later ones.
