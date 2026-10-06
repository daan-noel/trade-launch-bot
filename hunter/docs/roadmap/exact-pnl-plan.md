# Exact PnL — what is still open

Live real, live paper, simulate, the sweep and the open marks price one formula, and a
real position books what its transactions moved in the wallet
([execution-costs.md](../plans/strategies/execution-costs.md)). Rows closed before the
deploy keep the figures they were booked with; they are not re-booked. Open:

1. **Deploy.** Migration `0019_trade_payer_net.sql` and the live binary ship together;
   until then the server books curve-side amounts.
2. **Study kernel.** `study-kernel/kernel.py` `net_project` mirrors the linear formula
   the engine no longer uses; `net` (the exact curve) is the engine's formula, less
   the per-side fixed cost and the close.
