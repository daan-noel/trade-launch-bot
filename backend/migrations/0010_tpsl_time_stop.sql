-- E2 · Time stop / max-hold — p_time_stop_secs.
-- Cuts positions that neither moon nor crash: exit at the first trade whose
-- block_time is at least this many seconds after entry. Inert by default
-- (0 / NULL = disabled, per the ignore_zero_* convention).
ALTER TABLE strategy_TPSL_rules ADD COLUMN IF NOT EXISTS p_time_stop_secs BIGINT;
