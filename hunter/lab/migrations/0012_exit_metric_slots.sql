-- Per-combo breakdown of `ExitCode::Metrics` exits by WHICH authored exit
-- condition fired.
--
-- `n_exit_metrics` (added by `0003`) collapses every authored exit condition a
-- rule can carry (`stall > 3`, `retrace >= 5`, `held >= 10`, …) into one bucket,
-- because the per-combo aggregate (`ComboAgg`/`RunAgg`) is a fixed-size streaming
-- accumulator held `combos`-wide in RAM (hundreds of thousands of them per run) —
-- it can't afford a counter per distinct metric label without losing the O(1)-
-- per-combo memory bound the sweep's whole RAM budget rests on
-- (see docs/arch/sweep.md).
--
-- The fix keeps that bound: `n_exit_metrics_by_slot` is a FIXED-SIZE array (see
-- `hunter_lab::sweep::strategy::N_EXIT_METRIC_SLOTS`, currently 8) indexed by the
-- 0-based position of the rule's OWN authored exit reqs (not a global metric id),
-- resolved once per combo at bind time (`BoundCombo::exit_metric_label`) — zero
-- extra per-token work. A rule with more than 8 authored conditions folds the
-- overflow into the last slot, never worse than the old single bucket.
--
-- One `INTEGER[]` column, not 8 scalar ones: `append_group`'s bulk insert already
-- sits close to the 65535 bind-parameter ceiling on its 2000-row chunks (see the
-- comment there) — an array column costs exactly ONE bind per row.

ALTER TABLE grouped_sweep_results
    ADD COLUMN IF NOT EXISTS n_exit_metrics_by_slot INTEGER[] NOT NULL DEFAULT '{}';
