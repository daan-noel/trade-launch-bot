-- Add PendingEntry to the status CHECK on all 4 position tables and backfill
-- existing unentered rows so they carry the new state.

-- tpsl1_real_positions
ALTER TABLE tpsl1_real_positions DROP CONSTRAINT IF EXISTS tpsl1_real_positions_status_check;
ALTER TABLE tpsl1_real_positions
    ADD CONSTRAINT tpsl1_real_positions_status_check
    CHECK (status IN ('Holding','PendingEntry','ExitPending','End','ExitFailed'));
UPDATE tpsl1_real_positions
    SET status = 'PendingEntry'
    WHERE status = 'Holding' AND entry_price IS NULL;

-- tpsl2_real_positions
ALTER TABLE tpsl2_real_positions DROP CONSTRAINT IF EXISTS tpsl2_real_positions_status_check;
ALTER TABLE tpsl2_real_positions
    ADD CONSTRAINT tpsl2_real_positions_status_check
    CHECK (status IN ('Holding','PendingEntry','ExitPending','End','ExitFailed'));
UPDATE tpsl2_real_positions
    SET status = 'PendingEntry'
    WHERE status = 'Holding' AND entry_price IS NULL;

-- tpsl1_paper_positions
ALTER TABLE tpsl1_paper_positions DROP CONSTRAINT IF EXISTS tpsl1_paper_positions_status_check;
ALTER TABLE tpsl1_paper_positions
    ADD CONSTRAINT tpsl1_paper_positions_status_check
    CHECK (status IN ('Holding','PendingEntry','ExitPending','End','ExitFailed'));
UPDATE tpsl1_paper_positions
    SET status = 'PendingEntry'
    WHERE status = 'Holding' AND (entry_price IS NULL OR entry_price <= 0);

-- tpsl2_paper_positions
ALTER TABLE tpsl2_paper_positions DROP CONSTRAINT IF EXISTS tpsl2_paper_positions_status_check;
ALTER TABLE tpsl2_paper_positions
    ADD CONSTRAINT tpsl2_paper_positions_status_check
    CHECK (status IN ('Holding','PendingEntry','ExitPending','End','ExitFailed'));
UPDATE tpsl2_paper_positions
    SET status = 'PendingEntry'
    WHERE status = 'Holding' AND (entry_price IS NULL OR entry_price <= 0);
