-- tpsl_sniper_1 — give the live strategy its own table prefix so every piece of
-- its storage is separated and obvious at a glance (matching tpsl_sniper_2). The
-- previously generic `positions` / `paper_positions` / `paper_test_runs` tables
-- were only ever used by tpsl1, so they're renamed in place (data preserved; the
-- rules foreign key follows the table rename automatically). New names:
--   strategy_TPSL1_rules -> tpsl1_strategy_rules
--   positions            -> tpsl1_real_positions
--   paper_test_runs      -> tpsl1_paper_test_run
--   paper_positions      -> tpsl1_paper_positions

ALTER TABLE IF EXISTS strategy_TPSL1_rules RENAME TO tpsl1_strategy_rules;
ALTER INDEX IF EXISTS idx_strategy_TPSL1_rules_active RENAME TO idx_tpsl1_strategy_rules_active;

ALTER TABLE IF EXISTS positions RENAME TO tpsl1_real_positions;
ALTER INDEX IF EXISTS idx_positions_mint              RENAME TO idx_tpsl1_real_positions_mint;
ALTER INDEX IF EXISTS idx_positions_wallet            RENAME TO idx_tpsl1_real_positions_wallet;
ALTER INDEX IF EXISTS idx_positions_status            RENAME TO idx_tpsl1_real_positions_status;
ALTER INDEX IF EXISTS idx_positions_strategy          RENAME TO idx_tpsl1_real_positions_strategy;
ALTER INDEX IF EXISTS idx_positions_rule_id           RENAME TO idx_tpsl1_real_positions_rule_id;
ALTER INDEX IF EXISTS idx_positions_mint_status       RENAME TO idx_tpsl1_real_positions_mint_status;
ALTER INDEX IF EXISTS idx_positions_token_program_id  RENAME TO idx_tpsl1_real_positions_token_program_id;

ALTER TABLE IF EXISTS paper_test_runs RENAME TO tpsl1_paper_test_run;
ALTER INDEX IF EXISTS idx_paper_test_runs_rule RENAME TO idx_tpsl1_paper_test_run_rule;

ALTER TABLE IF EXISTS paper_positions RENAME TO tpsl1_paper_positions;
ALTER INDEX IF EXISTS idx_paper_positions_run          RENAME TO idx_tpsl1_paper_positions_run;
ALTER INDEX IF EXISTS idx_paper_positions_mint_status  RENAME TO idx_tpsl1_paper_positions_mint_status;
ALTER INDEX IF EXISTS idx_paper_positions_rule         RENAME TO idx_tpsl1_paper_positions_rule;
