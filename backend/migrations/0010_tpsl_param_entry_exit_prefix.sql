-- Rename TPSL rule params to self-describing prefixes:
--   p_token_* -> token fingerprint (which token the rule matches at creation)
--   p_entry_* -> entry gates (thresholds evaluated to decide WHEN to buy)
--   p_exit_*  -> exit gates (thresholds that decide when to sell)
-- so the column name alone says what role the param plays.
--
-- Applies to BOTH strategies' rule tables. Sizing/meta params
-- (buy_amount, tolerance_pct, p_max_concurrent_tokens, p_max_total_tokens,
-- trade_mode, rule_name, is_active) are intentionally left unprefixed —
-- they are none of the three.
--
-- RENAME COLUMN preserves data, types, NOT NULL and defaults, so existing
-- rows are untouched.

-- ── tpsl1_strategy_rules ─────────────────────────────────────────────────
-- Token fingerprint params (which token to match)
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_initial_buy_sol   TO p_token_initial_buy_sol;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_cu_limit          TO p_token_cu_limit;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_cu_price          TO p_token_cu_price;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_max_sol_cost      TO p_token_max_sol_cost;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_spendable_sol_in  TO p_token_spendable_sol_in;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_ix_labels         TO p_token_ix_labels;
-- Exit params
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN take_profit         TO p_exit_take_profit;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN stop_loss           TO p_exit_stop_loss;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_trailing_stop_pct TO p_exit_trailing_stop_pct;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_time_stop_secs    TO p_exit_time_stop_secs;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_stall_secs        TO p_exit_stall_secs;
ALTER TABLE tpsl1_strategy_rules RENAME COLUMN p_liquidity_drop_pct TO p_exit_liquidity_drop_pct;

-- ── tpsl2_strategy_rules ─────────────────────────────────────────────────
-- Token fingerprint params (which token to match; shared)
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_initial_buy_sol   TO p_token_initial_buy_sol;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_cu_limit          TO p_token_cu_limit;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_cu_price          TO p_token_cu_price;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_max_sol_cost      TO p_token_max_sol_cost;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_spendable_sol_in  TO p_token_spendable_sol_in;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_ix_labels         TO p_token_ix_labels;
-- Entry params (scalp-continuation gates, tpsl2 only)
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_min_age_secs      TO p_entry_min_age_secs;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_min_alive_sol     TO p_entry_min_alive_sol;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_min_organic_sol   TO p_entry_min_organic_sol;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_pullback_pct      TO p_entry_pullback_pct;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_higher_low_secs   TO p_entry_higher_low_secs;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_max_cohort_held   TO p_entry_max_cohort_held;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_min_liquidity_sol TO p_entry_min_liquidity_sol;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_min_organic_liq   TO p_entry_min_organic_liq;
-- Exit params (shared)
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN take_profit         TO p_exit_take_profit;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN stop_loss           TO p_exit_stop_loss;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_trailing_stop_pct TO p_exit_trailing_stop_pct;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_time_stop_secs    TO p_exit_time_stop_secs;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_stall_secs        TO p_exit_stall_secs;
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_liquidity_drop_pct TO p_exit_liquidity_drop_pct;
-- Exit param (scalp-continuation cohort-dump exit, tpsl2 only).
-- Collapse the now-redundant '_exit_' in the stem -> p_exit_cohort_ratio.
ALTER TABLE tpsl2_strategy_rules RENAME COLUMN p_cohort_exit_ratio TO p_exit_cohort_ratio;
