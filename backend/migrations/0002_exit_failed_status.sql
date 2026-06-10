-- Add 'ExitFailed' as a terminal position status.
--
-- A position now ends one of two ways once an exit is triggered: it exits
-- cleanly ('End') or the exit attempt completes and fails ('ExitFailed' —
-- real: sell retries exhausted without clearing the balance; paper: no
-- confirming exit trade indexed within the poll window). The original inline
-- CHECK only allowed Holding/ExitPending/End, so widen it on both tables.

ALTER TABLE positions DROP CONSTRAINT IF EXISTS positions_status_check;
ALTER TABLE positions ADD CONSTRAINT positions_status_check
    CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed'));

ALTER TABLE paper_positions DROP CONSTRAINT IF EXISTS paper_positions_status_check;
ALTER TABLE paper_positions ADD CONSTRAINT paper_positions_status_check
    CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed'));
