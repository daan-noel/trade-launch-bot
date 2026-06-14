-- Composite (filter, created_at DESC) indexes for the TPSL position list views.
-- find_by_strategy / find_by_rule / find_holding_by_mint / find_holding_by_wallet
-- / find_all_holding all filter then `ORDER BY created_at DESC`; without a
-- composite, Postgres filters then sorts. Mirrored across both strategy tables
-- (tpsl1 + tpsl2 share an identical column set). All IF NOT EXISTS so a re-run
-- or partially-applied DB is a no-op.

-- tpsl1 ----------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_strategy_created
    ON tpsl1_real_positions(strategy, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_rule_created
    ON tpsl1_real_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_mint_status_created
    ON tpsl1_real_positions(mint, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_wallet_status_created
    ON tpsl1_real_positions(wallet, status, created_at DESC);

-- tpsl2 ----------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_strategy_created
    ON tpsl2_real_positions(strategy, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_rule_created
    ON tpsl2_real_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_mint_status_created
    ON tpsl2_real_positions(mint, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_wallet_status_created
    ON tpsl2_real_positions(wallet, status, created_at DESC);
