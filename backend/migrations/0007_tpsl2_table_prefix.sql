-- tpsl_sniper_2 — align its tables to the exact same prefix convention as
-- tpsl_sniper_1 (0006), so both strategies read identically at a glance. Renamed
-- in place (data preserved; foreign keys follow the table renames). New names:
--   strategy_TPSL2_rules   -> tpsl2_strategy_rules
--   tpsl2_positions        -> tpsl2_real_positions
--   tpsl2_paper_test_runs  -> tpsl2_paper_test_run
-- (tpsl2_paper_positions already matches the convention — left unchanged.)

ALTER TABLE IF EXISTS strategy_TPSL2_rules RENAME TO tpsl2_strategy_rules;
ALTER INDEX IF EXISTS idx_strategy_TPSL2_rules_active RENAME TO idx_tpsl2_strategy_rules_active;

ALTER TABLE IF EXISTS tpsl2_positions RENAME TO tpsl2_real_positions;
ALTER INDEX IF EXISTS idx_tpsl2_positions_mint              RENAME TO idx_tpsl2_real_positions_mint;
ALTER INDEX IF EXISTS idx_tpsl2_positions_wallet            RENAME TO idx_tpsl2_real_positions_wallet;
ALTER INDEX IF EXISTS idx_tpsl2_positions_status            RENAME TO idx_tpsl2_real_positions_status;
ALTER INDEX IF EXISTS idx_tpsl2_positions_strategy          RENAME TO idx_tpsl2_real_positions_strategy;
ALTER INDEX IF EXISTS idx_tpsl2_positions_rule_id           RENAME TO idx_tpsl2_real_positions_rule_id;
ALTER INDEX IF EXISTS idx_tpsl2_positions_mint_status       RENAME TO idx_tpsl2_real_positions_mint_status;
ALTER INDEX IF EXISTS idx_tpsl2_positions_token_program_id  RENAME TO idx_tpsl2_real_positions_token_program_id;

ALTER TABLE IF EXISTS tpsl2_paper_test_runs RENAME TO tpsl2_paper_test_run;
ALTER INDEX IF EXISTS idx_tpsl2_paper_test_runs_rule RENAME TO idx_tpsl2_paper_test_run_rule;
