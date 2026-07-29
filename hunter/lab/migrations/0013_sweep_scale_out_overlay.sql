-- 0013_sweep_scale_out_overlay.sql — Pass-2 fixed scale_out ladder on grouped sweeps.
--
-- When set, each group's top-K combos are re-scored with this ladder after the
-- cheap axes pass (docs/roadmap/scale-out-sweep-overlay-plan.md). NULL on
-- legacy / no-overlay runs. Promote merges this into the draft params.
-- `IF NOT EXISTS` keeps this idempotent.

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS scale_out JSONB;

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS scale_out_top_k INT;
