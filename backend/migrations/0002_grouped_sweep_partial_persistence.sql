-- =========================================================================
-- Phase 4 — incremental / partial-result persistence for grouped sweeps.
--
-- A grouped sweep used to persist its run + every group + every combo row in a
-- single end-of-run transaction, so a cancel or crash mid-sweep threw away all
-- already-computed groups. We now write the run row up front (`status =
-- 'running'`), append each *completed* group in its own transaction, and
-- finalize the status at the end. These two columns carry that lifecycle:
--
--   status      — 'running' (in flight) | 'completed' (full sweep) |
--                 'cancelled' (cancelled or crash-recovered → partial, only the
--                 groups that finished are present).
--   groups_done — how many groups have been persisted so far (bumped per
--                 `append_group`); equals `group_count` for a completed run.
--
-- Additive, idempotent column adds on both per-strategy `_runs` tables. Existing
-- rows predate partial persistence → they are complete by definition, so default
-- `status = 'completed'` and backfill `groups_done = group_count`.
-- =========================================================================

ALTER TABLE tpsl2_grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS status      TEXT    NOT NULL DEFAULT 'completed',
    ADD COLUMN IF NOT EXISTS groups_done INTEGER NOT NULL DEFAULT 0;

ALTER TABLE tpsl1_grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS status      TEXT    NOT NULL DEFAULT 'completed',
    ADD COLUMN IF NOT EXISTS groups_done INTEGER NOT NULL DEFAULT 0;

-- Existing rows are fully-folded sweeps: their persisted group set is complete.
UPDATE tpsl2_grouped_sweep_runs SET groups_done = group_count WHERE groups_done = 0;
UPDATE tpsl1_grouped_sweep_runs SET groups_done = group_count WHERE groups_done = 0;

-- Constrain status to the three lifecycle values (guarded so a re-run is a no-op).
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'tpsl2_grouped_sweep_runs_status_chk') THEN
        ALTER TABLE tpsl2_grouped_sweep_runs
            ADD CONSTRAINT tpsl2_grouped_sweep_runs_status_chk
            CHECK (status IN ('running', 'completed', 'cancelled'));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'tpsl1_grouped_sweep_runs_status_chk') THEN
        ALTER TABLE tpsl1_grouped_sweep_runs
            ADD CONSTRAINT tpsl1_grouped_sweep_runs_status_chk
            CHECK (status IN ('running', 'completed', 'cancelled'));
    END IF;
END $$;
