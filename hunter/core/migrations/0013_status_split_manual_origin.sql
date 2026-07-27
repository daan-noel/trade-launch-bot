-- 0013_status_split_manual_origin.sql — the real-trade Console status redesign (P1).
--
-- WHY. `ExitFailed` overloaded two realities: "the buy never filled" (fill:None —
-- there was never a position) and "the sell gave up, the bag is still held" (an
-- OPEN problem, not history). The split gives each an honest home:
--   * EntryFailed — terminal, no SOL deployed, excluded from realized PnL.
--   * ExitStuck   — open (attention lane), reaper redrive + park still apply
--                   (exit_redrive_count / exit_parked from 0012 carry over).
-- `Arming` was vestigial (the sink always overwrites to BuySubmitted before the
-- first insert) — dropped from the domain.
--
-- Also lays the P2 groundwork (same migration by design): `origin` marks manual
-- buys as first-class positions, `manual_exit` holds their optional TP/SL config.

-- 1. Widen the CHECK first (Postgres CHECK constraints validate immediately, not
-- deferrable like FKs) so the remap UPDATEs below don't fail against a database
-- that actually has ExitFailed/Arming rows — the exact case this migration exists
-- to handle. Transiently allow both the old and new vocabulary; narrowed in step 3.
ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_status_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_status_check
    CHECK (status IN ('Arming','BuySubmitted','Holding','ExitPending',
                      'ExitUnconfirmed','ExitStuck','End','EntryFailed','ExitFailed'));

-- 2. Remap existing rows. ExitFailed with no entry fill was a failed BUY.
UPDATE strategy_positions SET status = 'EntryFailed'
    WHERE status = 'ExitFailed' AND entry_price IS NULL;
UPDATE strategy_positions SET status = 'ExitStuck'
    WHERE status = 'ExitFailed';

-- EntryFailed rows never bought — the old sink stamped a hypothetical exit
-- price/time on them; clear the phantom exit so no surface can read it as a fill.
UPDATE strategy_positions SET exit_price = NULL, exit_time = NULL
    WHERE status = 'EntryFailed';

-- Vestigial Arming rows: an entered one (should not exist) is really Holding;
-- the rest never sent a buy (no SOL, no tokens) — safe to drop.
UPDATE strategy_positions SET status = 'Holding'
    WHERE status = 'Arming' AND entry_price IS NOT NULL;
DELETE FROM strategy_positions WHERE status = 'Arming';

-- 3. New status domain (drop Arming + ExitFailed; add EntryFailed + ExitStuck).
ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_status_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_status_check
    CHECK (status IN ('BuySubmitted','Holding','ExitPending',
                      'ExitUnconfirmed','ExitStuck','End','EntryFailed'));

-- 3. Manual-position groundwork (P2): origin badge/filter + optional per-position
-- exit config ({"tp_pct": .., "sl_pct": ..}; NULL = tracked-only, no auto-exit).
ALTER TABLE strategy_positions
    ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'bot',
    ADD COLUMN IF NOT EXISTS manual_exit JSONB;
ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_origin_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_origin_check CHECK (origin IN ('bot','manual'));
