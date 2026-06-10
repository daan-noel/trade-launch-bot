-- Persist the exit reason on each position so the UI can show WHY a position
-- closed: TakeProfit / StopLoss / TrailingStop / Stall / TimeStop / LiquidityExit.
--
-- Previously the displayed reason was inferred from the realized-PnL sign, which
-- can only tell TakeProfit from StopLoss. Once the live exit path gained the
-- E1–E4 exits (trailing / time / stall / liquidity) that heuristic became wrong
-- — e.g. a trailing stop that banks a gain looked like a take-profit — so record
-- the real reason at exit time instead.
--
-- NULL means the position has not exited yet (or is a legacy row whose reason
-- predates this column; those still fall back to the PnL-sign heuristic on read).

ALTER TABLE positions       ADD COLUMN IF NOT EXISTS exit_reason TEXT;
ALTER TABLE paper_positions ADD COLUMN IF NOT EXISTS exit_reason TEXT;
