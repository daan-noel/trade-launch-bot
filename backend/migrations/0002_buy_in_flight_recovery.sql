-- ============================================================================
-- Buy-in-flight recovery (Phase 1) — make the buy path crash-safe.
--
-- The buy side had no durable marker between *sending* a snipe buy and
-- *recording* its fill, so a crash in that gap could leave wallet tokens that
-- the bot tracked with nothing (a `PendingEntry` row the stale reaper then
-- deleted). This migration splits that overloaded state and persists the
-- submitted buy signature(s) so a restart can recover the entry:
--
--   * Rename `PendingEntry` -> `Arming` (matched a rule, watching the live feed
--     for the scalp trigger; no buy sent, no SOL committed — reapable as today).
--   * Add `BuySubmitted` (buy signed/sent, awaiting the fill — must be recovered,
--     never reaped). `submitted_buy_signatures` records every attempt's sig.
--
-- The data rewrite runs BEFORE the CHECK swap so existing rows don't violate the
-- new constraint. Idempotent guards (IF EXISTS / IF NOT EXISTS) keep it safe on a
-- fresh DB where 0001 already created the tables.
-- ============================================================================

-- 1. Rewrite existing rows: PendingEntry -> Arming (all four position tables).
UPDATE tpsl1_real_positions  SET status = 'Arming' WHERE status = 'PendingEntry';
UPDATE tpsl1_paper_positions SET status = 'Arming' WHERE status = 'PendingEntry';
UPDATE tpsl2_real_positions  SET status = 'Arming' WHERE status = 'PendingEntry';
UPDATE tpsl2_paper_positions SET status = 'Arming' WHERE status = 'PendingEntry';

-- 2. Swap the status CHECK constraint to the new set on all four position tables.
--    (Postgres auto-names an inline CHECK `<table>_status_check`.)
ALTER TABLE tpsl1_real_positions  DROP CONSTRAINT IF EXISTS tpsl1_real_positions_status_check;
ALTER TABLE tpsl1_real_positions  ADD  CONSTRAINT tpsl1_real_positions_status_check
    CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed'));

ALTER TABLE tpsl1_paper_positions DROP CONSTRAINT IF EXISTS tpsl1_paper_positions_status_check;
ALTER TABLE tpsl1_paper_positions ADD  CONSTRAINT tpsl1_paper_positions_status_check
    CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed'));

ALTER TABLE tpsl2_real_positions  DROP CONSTRAINT IF EXISTS tpsl2_real_positions_status_check;
ALTER TABLE tpsl2_real_positions  ADD  CONSTRAINT tpsl2_real_positions_status_check
    CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed'));

ALTER TABLE tpsl2_paper_positions DROP CONSTRAINT IF EXISTS tpsl2_paper_positions_status_check;
ALTER TABLE tpsl2_paper_positions ADD  CONSTRAINT tpsl2_paper_positions_status_check
    CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed'));

-- 3. Persist submitted buy signatures on the REAL tables only (paper sends no
--    buys). Each buy attempt appends its signature so the recovery reaper can
--    check them all (a durable-nonce tx that lands later is found by its sig).
ALTER TABLE tpsl1_real_positions ADD COLUMN IF NOT EXISTS submitted_buy_signatures TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE tpsl2_real_positions ADD COLUMN IF NOT EXISTS submitted_buy_signatures TEXT[] NOT NULL DEFAULT '{}';

-- Partial index for the recovery reaper's `find_all_buy_submitted` scan.
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_buy_submitted
    ON tpsl1_real_positions(updated_at) WHERE status = 'BuySubmitted';
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_buy_submitted
    ON tpsl2_real_positions(updated_at) WHERE status = 'BuySubmitted';
