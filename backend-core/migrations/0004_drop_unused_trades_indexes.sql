-- Drop 5 never-scanned indexes on the high-volume `trades` table.
--
-- On the 2-vCPU/4GB box the ingest DbWriter wedges under the trade firehose:
-- Postgres is disk-IO-bound (DataFileRead waits) and every insert maintains 10
-- indexes. Production `pg_stat_user_indexes` over a full day (trades_2026_06_23)
-- showed these 5 with idx_scan = 0 — pure write amplification, never read:
--
--   idx_trades_wallet           wallet_address
--   idx_trades_mint_venue_slot  (mint_address, venue, slot DESC)
--   idx_trades_wallet_mint      (wallet_address, mint_address)
--   idx_trades_mint_slot_leg    (mint_address, slot, leg_index)
--   idx_trades_wallet_mint_sig  (wallet_address, mint_address, tx_signature)  -- 109 MB/day
--
-- Note: idx_trades_wallet_mint_sig was commented as the hot find_fill_by_signature /
-- sell-confirm index, but production stats prove it's unused — the unique
-- idx_trades_tx_leg (leads with tx_signature, far more selective) serves those
-- lookups instead (32k scans/day). Kept indexes: idx_trades_mint (143k scans),
-- idx_trades_tx_leg (unique, backs ON CONFLICT + signature lookups), and the small
-- idx_trades_block_time / idx_trades_mint_time.
--
-- Indexes live on the partitioned parent `trades`; DROP cascades to every
-- existing and future partition. Non-CONCURRENT is fine here: dropping an index
-- only unlinks metadata + storage (fast) under a brief lock.

DROP INDEX IF EXISTS idx_trades_wallet;
DROP INDEX IF EXISTS idx_trades_mint_venue_slot;
DROP INDEX IF EXISTS idx_trades_wallet_mint;
DROP INDEX IF EXISTS idx_trades_mint_slot_leg;
DROP INDEX IF EXISTS idx_trades_wallet_mint_sig;
