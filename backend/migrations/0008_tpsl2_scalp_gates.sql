-- tpsl_sniper_2 scalp-continuation gates (see @project_plans/@TPSL_plan/
-- tpsl-scalp-continuation-plan.md). New per-rule entry/exit params, all nullable
-- and inert at NULL/0 (the existing ignore_zero convention), so existing tpsl2
-- rules behave exactly as before. tpsl1 is intentionally NOT touched — tpsl2
-- diverges as the scalp experiment.

ALTER TABLE tpsl2_strategy_rules
    -- Entry gates: evaluated over a token's trade window, "buy on the first trade
    -- where ALL configured gates hold".
    ADD COLUMN IF NOT EXISTS p_min_age_secs      BIGINT,           -- skip the launch spike (entry age >= N secs)
    ADD COLUMN IF NOT EXISTS p_min_alive_sol     DOUBLE PRECISION, -- liveness: total SOL traded in the trailing window
    ADD COLUMN IF NOT EXISTS p_min_organic_sol   DOUBLE PRECISION, -- net SOL bought by outside (non-cohort) wallets
    ADD COLUMN IF NOT EXISTS p_pullback_pct      DOUBLE PRECISION, -- higher-low: min dip off the local high to count
    ADD COLUMN IF NOT EXISTS p_higher_low_secs   BIGINT,           -- higher-low: min span the pattern must form over
    ADD COLUMN IF NOT EXISTS p_max_cohort_held   DOUBLE PRECISION, -- launch cohort must have sold down to <= this ratio
    ADD COLUMN IF NOT EXISTS p_min_liquidity_sol DOUBLE PRECISION, -- min real SOL reserves
    ADD COLUMN IF NOT EXISTS p_min_organic_liq   DOUBLE PRECISION, -- min real reserves sourced from outside buyers
    -- Exit gate: E5 cohort-dump, the top-priority exit.
    ADD COLUMN IF NOT EXISTS p_cohort_exit_ratio DOUBLE PRECISION; -- cohort net holdings collapsed to <= this ratio -> exit
