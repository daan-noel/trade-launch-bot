-- ===========================================================================
-- *_grouped_sweep_results.n_exit_cohort — drop the E5 launch-cohort exit
-- counter.
--
-- The launch-cohort feature (entry gate `p_entry_max_cohort_held` + exit
-- ladder rung E5 `p_exit_cohort_ratio`/`ExitReason::CohortExit`) has been
-- removed from the tpsl_sniper_2 strategy (see cohort-removal-plan.md).
-- Mirrors `trading_core/migrations/0007_drop_cohort_metrics.sql`.
-- ===========================================================================

ALTER TABLE tpsl1_grouped_sweep_results
    DROP COLUMN IF EXISTS n_exit_cohort;

ALTER TABLE tpsl2_grouped_sweep_results
    DROP COLUMN IF EXISTS n_exit_cohort;

ALTER TABLE swing_1_grouped_sweep_results
    DROP COLUMN IF EXISTS n_exit_cohort;
