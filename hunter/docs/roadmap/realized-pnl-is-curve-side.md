# Realized PnL is curve-side, so it is gross of fees

The open-position mark is net of every cost (`mark_open_bag`,
[execution-costs §6](../plans/strategies/execution-costs.md)). The **realized** figure
beside it is not, and the gap is the same ~4 pp. Closing it restates the whole live
book, so it is a decision to take deliberately rather than a defect to patch on sight.

## What is wrong

`StrategyPosition::realized_pnl_sol` is `exit_lamports - entry_lamports`, and both sides
sum `trades.amount_lamports` through `SigLegs`. That column is the **curve-side** amount:
it excludes the 125 bps protocol fee by construction — measured, `|Δreserve_lamports| /
amount_lamports` = 1.00000 at p25/median/p75 over 5.6M legs — and it never carried tip or
priority, which live in `trades.fee_lamports` / `tip_lamports` and are read by nothing.

So a realized number is net of slippage and of our own impact (they are inside the fill
price) but gross of fees. At the measured median real clip of **0.0296 SOL** that is
1.25 pp + 0.77 pp per leg, i.e. ~4 pp per round trip — the same four terms the open mark
now charges.

Three things inherit it:

| surface | reads | consequence |
| --- | --- | --- |
| `pnl_pct`, `PNL_PCT_SQL` | `realized_pnl_sol / entry_sol` | every closed row ~4 pp high |
| `is_win` | `exit_lamports > entry_lamports` | a sub-break-even trade counts as a win |
| `return_pct`, `RuleCounters`, the scoreboard | Σ of the above | keep/kill ranks on a gross book |

The win-rate one is the sharpest: a rule whose trades cluster between 0 % and +4 % scores
as a winner while losing money, which is exactly the band a scalper lives in.

## Why it is not just fixed

Unlike the open mark — a number computed fresh on every read, so correcting it changes
nothing stored — this restates history:

- **Win/loss flips.** Rule comparisons made before and after are not comparable, and the
  scoreboard is what rules are kept or killed on.
- **`strategy_run_metrics` is stamped at rollup**, not derived, so finalized runs keep
  their gross figures and cannot be recomputed from a view (same shape as the pre-2026-07-28
  fee change).
- **Two conventions exist in one column.** The ingest's TradeEvent path records the
  curve-side amount; the `compute_sol_change` fallback records the payer's own lamport
  delta, which already absorbs the fee (`decode/protobuf.rs` says so at the call site).
  The measurement says the fallback is rare, but a fee correction applied blindly
  double-charges those rows.

## What landing it looks like

1. Decide the basis: **modelled** (`CostModel`, symmetric with every backtest and
   available on every row) or **actual** (`fee_lamports` + `tip_lamports`, true but
   populated on a minority of rows, and never carrying the protocol fee at all).
   Modelled is the one that keeps live comparable to the sim.
2. Charge it in the one numerator, `PNL_LAMPORTS_SQL`, so the model, the view and the
   sort expression cannot disagree (guarded today by
   `pnl_sql_columns_share_one_numerator`).
3. Move `is_win` onto the same numerator — the win rule is currently a second definition
   spelled `exit_lamports > entry_lamports`.
4. Mark the cutover in `strategy_run_metrics` so a reader can tell which side of it a run
   was finalized on, and re-baseline the scoreboard.
5. Resolve the two-convention hazard first: either teach the fallback path to record the
   curve-side amount, or flag which convention a row used.

Until then the honest reading is: **open positions are all-in, closed ones are gross of
~4 pp.** [pnl-percent-definition.md](../plans/strategies/pnl-percent-definition.md) says
so at the point it defines the percent.
