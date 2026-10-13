-- Who wrote the row. 0 = live LaserStream ingest (the default, including every
-- row written before this column). 1 = token-sync. Same meaning as
-- `raw_txs.source`.
--
-- Live `block_time` is the receive clock and carries a fractional second.
-- Token-sync writes the chain `blockTime`, a whole second. The primary key
-- includes `block_time`, so the two writers do not collide; `source` is how a
-- reader tells them apart.
--
-- A row stored before this column stays 0 until classified. A whole-second
-- `block_time` is the chain second token-sync writes.

ALTER TABLE trades
    ADD COLUMN IF NOT EXISTS source SMALLINT NOT NULL DEFAULT 0;

COMMENT ON COLUMN trades.source IS
    '0 = live LaserStream ingest. 1 = token-sync. Live block_time is receive time; sync block_time is the chain second.';
