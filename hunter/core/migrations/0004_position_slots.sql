-- ============================================================================
-- 0004 position_slots — `target_slot` / `entry_slot` / `exit_slot` on
-- `strategy_positions`: execution latency in the only unit a bonding curve
-- moves in.
--
-- A curve price changes when a trade LANDS, and trades land in slots. Wall
-- clock cannot express that: `trades.block_time` is the ingest clock, so a
-- timestamp delta measures how fast this process saw the feed, never how many
-- slots of price movement happened between deciding and filling. `entry_slot -
-- target_slot` is that number, and nothing else stored today can produce it.
--
-- The three columns pair with the snapshots already on the row:
--   target_* = the trigger trade that armed the entry (what we knew)
--   entry_*  = the buy that landed            (what we got)
--   exit_*   = the sell that landed           (the same question, exit side)
--
-- NULL is the honest value wherever the slot is unknown — a legacy row, a
-- paper fill (simulated, so it has no on-chain slot), or an adopted fill whose
-- legs never resolved. A slot is a MEASURED quantity, so it uses NULL rather
-- than a 0 sentinel (0 is a real slot number).
--
-- BIGINT, not INTEGER: a Solana slot passes 2^31 and is a `u64` at the source.
-- Signed is fine — the value is nowhere near 2^63 — and keeps it readable from
-- sqlx's `i64` without a cast dance.
--
-- Plan: docs/plans/strategies/execution-latency.md
-- ============================================================================

ALTER TABLE strategy_positions ADD COLUMN IF NOT EXISTS target_slot BIGINT;
ALTER TABLE strategy_positions ADD COLUMN IF NOT EXISTS entry_slot  BIGINT;
ALTER TABLE strategy_positions ADD COLUMN IF NOT EXISTS exit_slot   BIGINT;

COMMENT ON COLUMN strategy_positions.target_slot IS
    'Slot of the trigger trade that armed this entry (pairs with target_price/time/tx). '
    'NULL when the trigger is unknown (legacy row, enter-on-arm with no print yet).';
COMMENT ON COLUMN strategy_positions.entry_slot IS
    'Slot the entry buy landed in (earliest confirmed leg). NULL for paper fills — '
    'a simulated fill has no on-chain slot — and for legacy rows.';
COMMENT ON COLUMN strategy_positions.exit_slot IS
    'Slot the exit sell landed in (latest confirmed leg). NULL for paper fills and '
    'legacy rows. Scale-outs stamp the most recent leg.';

-- The latency read is "real positions that have both ends", ordered recently-first.
-- Partial so the index carries only rows that can answer it: paper fills and
-- unfilled rows are permanently NULL on entry_slot and belong in no histogram.
CREATE INDEX IF NOT EXISTS idx_strategy_positions_latency
    ON strategy_positions(mode, entry_time DESC)
    WHERE target_slot IS NOT NULL AND entry_slot IS NOT NULL;
