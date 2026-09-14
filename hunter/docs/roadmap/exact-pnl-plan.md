# Exact PnL — what is still open

Live real, live paper, simulate, the sweep and the open marks price one formula, and a
real position books what its transactions moved in the wallet
([execution-costs.md](../plans/strategies/execution-costs.md)). Rows closed before the
deploy keep the figures they were booked with; they are not re-booked. Open:

1. **Deploy.** Migration `0019_trade_payer_net.sql` and the live binary ship together;
   until then the server books curve-side amounts.
2. **Stored AMM fee.** Neither `trades` nor the lake stores the per-swap PumpSwap fee,
   so a sweep leg on an AMM print and a Trader Analysis mark on a migrated pool price
   the curve's 125 bps; live paper and the live open marks price the pool's own fee.
3. **Rent stranded in empty token accounts.** The wallet holds 532 empty Token-2022
   accounts (1.14 SOL of rent, measured 2026-09-14), most from before 2026-09-07. A
   sweep that closes every empty account returns it. One path that still strands
   rent: each buy funds a fresh account, but the reclaim closes the per-mint cached
   one (`close_token_account(mint, None)`), so the account a concurrent buy on the same
   mint funded stays open. Closing the account the position funded
   (`FillSigs::token_account`) closes that path.
4. **Study kernel.** `study-kernel/kernel.py` `net_project` mirrors the linear formula
   the engine no longer uses; `net` (the exact curve) is the engine's formula, less
   the per-side fixed cost and the close.
