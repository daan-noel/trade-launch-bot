-- =========================================================================
-- TPSL2 target (trigger-trade) columns
--   Records the scalp-entry *trigger* trade as the "target" point, distinct
--   from the actual entry fill recorded in `entry_*`. Storing both lets us
--   later derive the gap (price slippage / time latency / size delta) between
--   the targeted point and the real entry. All nullable, no backfill: existing
--   rows and not-yet-armed positions stay NULL. `target_tx` is intentionally
--   NOT unique — the trigger is someone else's trade and can repeat across
--   positions.
-- =========================================================================

ALTER TABLE tpsl2_real_positions
    ADD COLUMN IF NOT EXISTS target_price  DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS target_amount DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS target_time   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS target_tx     TEXT;

ALTER TABLE tpsl2_paper_positions
    ADD COLUMN IF NOT EXISTS target_price  DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS target_amount DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS target_time   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS target_tx     TEXT;
