-- Add `n_exit_dead` — the analysis-only death-close exit counter — to every
-- `_results` table. When the exit ladder never fires but a token is provably
-- dead (liquidity gone + gone silent), the backtest/grouped-sweep now closes the
-- bag at the last meaningful trade (`ExitCode::Dead`) instead of leaving it
-- `Open` marked to a stale price. Live never produces this reason (it closes
-- silent tokens via its 1s clock sweep), so 0 on the live rollup path.
--
-- Same append-a-counter pattern as 0002 (`n_exit_next_kill`): DEFAULT 0 so the
-- existing rows read back as "no death-closes" without a backfill. Lab-only.
ALTER TABLE tpsl1_grouped_sweep_results
    ADD COLUMN IF NOT EXISTS n_exit_dead INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tpsl2_grouped_sweep_results
    ADD COLUMN IF NOT EXISTS n_exit_dead INTEGER NOT NULL DEFAULT 0;
ALTER TABLE swing_1_grouped_sweep_results
    ADD COLUMN IF NOT EXISTS n_exit_dead INTEGER NOT NULL DEFAULT 0;
