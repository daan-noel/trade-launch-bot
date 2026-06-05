-- Distinguish bonding-curve trades from post-migration PumpSwap (AMM) trades.
-- Existing rows are all bonding-curve trades, hence the 'curve' default.
ALTER TABLE trades ADD COLUMN IF NOT EXISTS venue TEXT NOT NULL DEFAULT 'curve'
    CHECK (venue IN ('curve', 'amm'));

-- Supports the incremental "Fetch new" boundary lookup: latest signature per
-- (mint, venue) so each source resumes from its own last saved trade.
CREATE INDEX IF NOT EXISTS idx_trades_mint_venue_slot
    ON trades (mint_address, venue, slot DESC);
