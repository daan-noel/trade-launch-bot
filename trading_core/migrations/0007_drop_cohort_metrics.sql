-- ===========================================================================
-- strategy_run_metrics.n_exit_cohort — drop the E5 launch-cohort exit counter.
--
-- The launch-cohort feature (entry gate `p_entry_max_cohort_held` + exit ladder
-- rung E5 `p_exit_cohort_ratio`/`ExitReason::CohortExit`) has been removed from
-- the tpsl_sniper_2 strategy (see cohort-removal-plan.md). Stale
-- `strategy_rules.params` JSONB keys are left in place (inert — nothing
-- deserializes them into a live field anymore); this migration only drops the
-- rolled-up metrics column that mirrored the removed exit reason.
-- ===========================================================================

ALTER TABLE strategy_run_metrics
    DROP COLUMN IF EXISTS n_exit_cohort;
