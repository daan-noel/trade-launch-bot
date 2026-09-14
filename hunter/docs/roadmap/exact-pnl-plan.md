# Exact PnL — what is still open

Live real, live paper, simulate, the sweep and the open marks price one formula, and a
real position books what its transactions moved in the wallet
([execution-costs.md](../plans/strategies/execution-costs.md)). Real positions closed
from 2026-08-31 are re-booked from the chain. Open:

1. **Deploy.** Migration `0019_trade_payer_net.sql` and the live binary ship together;
   until then the server books curve-side amounts. After the deploy, re-run the chain
   re-book over the real positions closed between the re-book and the deploy (same
   method: each fill's SOL = its transaction's payer net flow, the close fee where the
   token account is closed on chain).
2. **Live paper history.** Paper rows closed before the deploy hold the frictionless
   `B / price` amounts. Re-pricing them needs each fill print's reserve, which the
   `trades` rows carry: `buy_fill` / `sell_proceeds` against the stored print.
3. **`strategy_run_metrics`.** Rows are stamped at rollup, so a finalized run keeps the
   figures it was rolled up with. `StrategyRepo::roll_up_run` re-rolls one run; the
   re-booked runs need it once.
4. **PumpSwap fee.** The kernel prices the curve's 125 bps on every leg; a migrated
   coin trades on PumpSwap's fee. A real row books the wallet flow either way.
5. **Rent stranded in empty token accounts.** The wallet holds 532 empty Token-2022
   accounts (1.14 SOL of rent, measured 2026-09-14), most from before 2026-09-07. A
   sweep that closes every empty account returns it. One path that still strands
   rent: each buy funds a fresh account, but the reclaim closes the per-mint cached
   one (`close_token_account(mint, None)`), so the account a concurrent buy on the same
   mint funded stays open. Closing the account the position funded
   (`FillSigs::token_account`) closes that path.
6. **Study kernel.** `study-kernel/kernel.py` `net_project` mirrors the linear formula
   the engine no longer uses; `net` (the exact curve) is the engine's formula, less
   the per-side fixed cost and the close.
