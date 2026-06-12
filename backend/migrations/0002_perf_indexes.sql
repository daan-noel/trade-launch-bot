-- Phase 3.2: indexes for the hot lookup / ordering paths flagged in the backend
-- improvement plan. All IF NOT EXISTS so a re-run (or a partially-applied DB) is
-- a no-op.

-- Wallet+mint net-amount / balance checks (compute_is_rugged's creator-dump and
-- cohort-net queries, manual balance lookups) only had the wallet-only index to
-- fall back on; a composite avoids re-filtering every wallet row by mint.
CREATE INDEX IF NOT EXISTS idx_trades_wallet_mint ON trades(wallet_address, mint_address);

-- Chronological-by-slot ordering within a single mint (multi-leg aware). The
-- existing (mint, venue, slot) index doesn't serve slot ordering without a venue
-- predicate.
CREATE INDEX IF NOT EXISTS idx_trades_mint_slot_leg ON trades(mint_address, slot, leg_index);

-- Analysis-freshness scans (newest-first by computed_at).
CREATE INDEX IF NOT EXISTS idx_analysis_computed_at ON tokens_analysis(computed_at DESC);

-- Creator suspiciousness ranking / threshold filtering.
CREATE INDEX IF NOT EXISTS idx_creator_profiles_suspiciousness
    ON creator_profiles(suspiciousness_score DESC);
