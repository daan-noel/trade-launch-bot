-- ===========================================================================
-- 0006  lab: metric system v2 (docs/roadmap/metric-system-v2-plan.md)
-- ===========================================================================
-- A sweep run reads tags (a fingerprint `tags` document) instead of a bare
-- volume-ix pattern list, and Pass 2 searches rule `stages` plans instead of
-- `scale_out` ladders. The columns are renamed here; their contents, and each
-- run's `axes_spec`, are converted by the lab's Rust data migration
-- (`storage::lab_data_migrations`) right after this file, with the engine's own
-- v1 converter, which SQL cannot call.
--
-- Combo `params` and group `best_params` stay as written: every reader parses
-- them through `hunter_engine::v1::parse_params_any`, which converts a v1 rule.
-- Runs are NOT re-scored.
-- ===========================================================================

ALTER TABLE grouped_sweep_runs RENAME COLUMN ix_patterns TO tags;
ALTER TABLE grouped_sweep_runs RENAME COLUMN scale_out TO stage_plans;
ALTER TABLE grouped_sweep_runs RENAME COLUMN scale_out_top_k TO stage_plans_top_k;
