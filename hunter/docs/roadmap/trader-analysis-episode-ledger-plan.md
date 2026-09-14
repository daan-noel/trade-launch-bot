# Trader Analysis episode ledger - what is still open

Trader Analysis counts a studied wallet's round trips on what the wallet moved: the
episode ledger ([strategies.md](../arch/strategies.md), `wallet_ledger.rs`) over
`TradeRepo::wallet_txs_on`, folded page-side by `walletPnlStats.ts`
([frontend.md](../arch/frontend.md)). Open:

1. **Deploy.** Every flow is `trades.payer_net_lamports`, which the server writes only
   once migration 0019 and the live binary ship ([exact-pnl-plan.md](exact-pnl-plan.md)
   item 1). Until then every episode is `incomplete` (`missing_flow`) and the page shows
   no closed trade; nothing before the deploy can ever be exact.
2. **Check against the chain.** Once flows exist, pick a few closed episodes and compare
   their `sol_in` / `sol_out` with each transaction's balance change from a free public
   RPC `getTransaction` (never Helius).
3. **Shared-transaction test cost.** `wallet_txs_on` tests each exact flow for another
   mint or wallet in the same transaction with a lookup that costs ~3 ms a transaction on
   a compressed chunk. If a long window gets slow once flows exist, stamp the test at
   ingest, where the whole transaction is in hand, instead of reading it back.
4. **Lake.** The ledger reads Postgres only (~30 days). A window older than that needs
   `payer_net_lamports` in the lake export first.
