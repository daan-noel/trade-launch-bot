-- Replace btree index on block_time with BRIN.
-- block_time is append-ordered (new rows always have larger values), so BRIN
-- gives equivalent range-scan support in a few KB instead of the btree's
-- per-insert write amplification.
DROP INDEX IF EXISTS idx_trades_block_time;
CREATE INDEX idx_trades_block_time_brin ON trades USING BRIN (block_time);
