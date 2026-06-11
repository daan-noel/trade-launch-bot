-- Rename the original "tpsl" strategy to "tpsl1" for symmetry with tpsl_sniper_2
-- (0004). Data-preserving: the rules table is renamed in place (its foreign keys
-- from positions / paper_test_runs follow the rename automatically), and the
-- strategy tag on existing position rows is updated "TPSL" -> "TPSL1". The shared
-- positions / paper_positions / paper_test_runs tables are NOT renamed — they
-- stay general (tpsl_sniper_1 keeps using them; tpsl_sniper_2 has its own).

ALTER TABLE IF EXISTS strategy_TPSL_rules RENAME TO strategy_TPSL1_rules;
ALTER INDEX IF EXISTS idx_strategy_TPSL_rules_active RENAME TO idx_strategy_TPSL1_rules_active;

UPDATE positions       SET strategy = 'TPSL1' WHERE strategy = 'TPSL';
UPDATE paper_positions SET strategy = 'TPSL1' WHERE strategy = 'TPSL';
