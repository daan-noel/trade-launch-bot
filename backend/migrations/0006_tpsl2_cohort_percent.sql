-- Percent-param unification (see percent-params-unify-plan.md).
-- The two cohort params were stored as raw fractions (0.30 / 0.05) while every
-- other percent field is whole-percent. The comparison sites now divide by 100
-- like TP/SL/E1/E4, so existing rows must be rescaled fraction -> percent.
-- 0/NULL ("off" sentinel) is unaffected by *100.
UPDATE tpsl2_strategy_rules
SET p_entry_max_cohort_held = p_entry_max_cohort_held * 100,
    p_exit_cohort_ratio     = p_exit_cohort_ratio     * 100
WHERE p_entry_max_cohort_held IS NOT NULL OR p_exit_cohort_ratio IS NOT NULL;
