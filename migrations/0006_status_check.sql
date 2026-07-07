-- Pin the launch/bundle lifecycle status vocabularies with CHECK constraints —
-- the SQL half of the SSOT that `platform_core::models::status`
-- (`LaunchStatus` / `BundleStatus`) owns on the Rust side. Before this, both
-- columns were open TEXT (0002_own_launch.sql), so a typo'd status (e.g. a
-- bundle stuck at a misspelled state the confirm watcher never matches) was a
-- silent data bug with no compiler or DB guard. Keep these IN-lists byte-equal
-- to the enums' `as_str()` (guarded by their roundtrip tests).

-- `bundles.status` never actually uses 'pending' — the lifecycle starts at
-- 'planned' (BundleRepo::insert inserts it explicitly). The old DEFAULT 'pending'
-- (0002) was dead and would violate the new CHECK, so realign it and heal any
-- legacy row that landed on the dead default.
UPDATE bundles SET status = 'planned' WHERE status = 'pending';
ALTER TABLE bundles ALTER COLUMN status SET DEFAULT 'planned';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'launches_status_check'
    ) THEN
        ALTER TABLE launches
            ADD CONSTRAINT launches_status_check
            CHECK (status IN ('pending', 'created', 'failed'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'bundles_status_check'
    ) THEN
        ALTER TABLE bundles
            ADD CONSTRAINT bundles_status_check
            CHECK (status IN (
                'planned', 'submitting', 'submitted',
                'landed', 'dropped', 'partial', 'failed'
            ));
    END IF;
END $$;
