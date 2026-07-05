-- Supersede idx_trades_wallet_time (0002) with a covering variant that also
-- carries `trade_type`.
--
-- The Trader Analysis query grew a `COUNT(*) FILTER (WHERE trade_type=…)` for the
-- per-wallet buy/sell columns. `trade_type` was NOT in the 0002 index, so the
-- plan fell back from an index-only scan to a plain index scan + heap fetch for
-- every matched row (~330ms for a busy wallet, worse for a bot wallet with tens
-- of thousands of trades in the window). Appending `trade_type` makes the whole
-- query index-only again (~5ms).
--
-- Still lab-only — zero ingest cost on the EC2 live box. Drop + recreate under
-- the same name so the planner transparently picks up the wider index.
DROP INDEX IF EXISTS idx_trades_wallet_time;
CREATE INDEX IF NOT EXISTS idx_trades_wallet_time
    ON trades (wallet_id, block_time DESC, mint_address, trade_type);
