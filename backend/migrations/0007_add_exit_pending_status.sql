-- Add ExitPending to the positions status check constraint.
-- The original constraint only allowed ('Holding', 'End'); ExitPending was added
-- to the init SQL after the database was already created, so a migration is needed.
ALTER TABLE positions DROP CONSTRAINT IF EXISTS positions_status_check;
ALTER TABLE positions ADD CONSTRAINT positions_status_check
    CHECK (status IN ('Holding', 'ExitPending', 'End'));
