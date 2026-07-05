-- Lab-only supporting index for the Trader Analysis query
-- (`TradeRepo::wallet_traded_mints` → GET /api/wallets/:wallet/tokens).
--
-- That query filters `trades` by an interned `wallet_id` over a `block_time`
-- window and groups by mint. The shared schema indexes `trades` only by
-- `(mint_address, slot, tx_index, leg_index)`, so a wallet-scoped scan falls
-- back to a full hypertable scan and hits the statement timeout.
--
-- This index lives in the **lab-only** migration set on purpose: only `lab`
-- runs this analysis query, and `lab` runs on the workstation. Keeping it out of
-- the shared core migrations means the EC2 `live` box's ingest hot path pays NO
-- extra per-insert index-maintenance cost — the exact write-amplification the
-- perf mandate guards against on the 2vCPU/4GB box.
--
-- Column order: seek by `wallet_id`, range-scan `block_time DESC` (recent-first),
-- with `mint_address` trailing so the GROUP BY is satisfied index-only (all three
-- referenced columns are in the index — no heap fetch). Idempotent.
CREATE INDEX IF NOT EXISTS idx_trades_wallet_time
    ON trades (wallet_id, block_time DESC, mint_address);
