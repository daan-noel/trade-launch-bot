-- Add buy_amount_sol to both sweep-run tables so the per-run notional is stored
-- alongside the other corpus/config metadata. NULL on legacy rows; the server
-- falls back to the SWEEP_BASE_BUY_AMOUNT_SOL default (1.0 SOL) when it reads NULL.
ALTER TABLE tpsl1_grouped_sweep_runs ADD COLUMN IF NOT EXISTS buy_amount_sol REAL;
ALTER TABLE tpsl2_grouped_sweep_runs ADD COLUMN IF NOT EXISTS buy_amount_sol REAL;
