-- E1 · Trailing stop — p_trailing_stop_pct.
-- Banks a reversal: exit when price falls X% below the peak-since-entry.
-- Inert by default (0 / NULL = disabled, per the ignore_zero_* convention in
-- backend/src/strategies/tpsl/util.rs).
ALTER TABLE strategy_TPSL_rules ADD COLUMN IF NOT EXISTS p_trailing_stop_pct DOUBLE PRECISION;
