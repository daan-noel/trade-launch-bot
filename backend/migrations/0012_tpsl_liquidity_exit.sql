-- E4 · Liquidity-death exit — p_liquidity_drop_pct.
-- Bails when liquidity is being pulled: exit once virtual SOL reserves crash this
-- far below the peak-since-entry (a reserve drop price stops can miss). Inert by
-- default (0 / NULL = disabled, per the ignore_zero_* convention).
ALTER TABLE strategy_TPSL_rules ADD COLUMN IF NOT EXISTS p_liquidity_drop_pct DOUBLE PRECISION;
