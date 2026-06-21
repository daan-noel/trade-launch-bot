-- Rename the position size columns to make explicit that they hold TOKEN amounts,
-- never SOL. SOL is derived at display as `price × token_amount` (see
-- tpsl2-paper-realism plan #4). The four position tables share the rename via the
-- common `Position` model; tpsl1 has no target_* columns.
--
-- Behaviour change is scoped to tpsl2 paper/backtest (the writes now store token
-- counts); tpsl1 keeps writing the same values into the renamed columns.

ALTER TABLE tpsl2_real_positions
    RENAME COLUMN target_amount TO target_token_amount;
ALTER TABLE tpsl2_real_positions
    RENAME COLUMN entry_amount TO entry_token_amount;
ALTER TABLE tpsl2_real_positions
    RENAME COLUMN exit_amount TO exit_token_amount;

ALTER TABLE tpsl2_paper_positions
    RENAME COLUMN target_amount TO target_token_amount;
ALTER TABLE tpsl2_paper_positions
    RENAME COLUMN entry_amount TO entry_token_amount;
ALTER TABLE tpsl2_paper_positions
    RENAME COLUMN exit_amount TO exit_token_amount;

ALTER TABLE tpsl1_real_positions
    RENAME COLUMN entry_amount TO entry_token_amount;
ALTER TABLE tpsl1_real_positions
    RENAME COLUMN exit_amount TO exit_token_amount;

ALTER TABLE tpsl1_paper_positions
    RENAME COLUMN entry_amount TO entry_token_amount;
ALTER TABLE tpsl1_paper_positions
    RENAME COLUMN exit_amount TO exit_token_amount;
