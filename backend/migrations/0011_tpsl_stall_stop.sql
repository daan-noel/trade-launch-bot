-- E3 · Stall / momentum-death — p_stall_secs.
-- Sells into the flatline: exit when no new higher-high has printed for at least
-- this many seconds (price keeps failing to exceed the peak-since-entry). Inert
-- by default (0 / NULL = disabled, per the ignore_zero_* convention).
ALTER TABLE strategy_TPSL_rules ADD COLUMN IF NOT EXISTS p_stall_secs BIGINT;
