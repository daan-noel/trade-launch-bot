-- ============================================================================
-- Partition `trades` by block_time, with 5-week retention.
--
-- Copy-migration: preserves every existing row (no drop & recreate). The old
-- `trades` table is renamed aside, a weekly RANGE-partitioned `trades` is built
-- with the identical column set, the historical rows are copied across, and the
-- old table is dropped.
--
-- Retention is enforced by the app's partition-maintenance task (KEEP_WEEKS = 5,
-- shared with raw_transactions) via ensure_trades_partition / drop_trades_partition.
--
-- Partition-key constraints (Postgres): the partition column must be part of any
-- PRIMARY KEY / UNIQUE index on a partitioned table. So:
--   * PK becomes (block_time, id).
--   * The dedup unique key becomes (tx_signature, leg_index, block_time). block_time
--     is deterministic per tx (chain-assigned), so adding it changes nothing about
--     dedup exactness — re-delivery of the same (tx, leg) still collides.
-- All other indexes are non-unique and recreate unchanged on the parent (they
-- cascade to every partition, present and future).
-- ============================================================================

-- 1. Rename the existing table aside (data preserved). Drop its indexes so their
--    names are free for the new parent's identical index set; trades_old is read
--    once for the copy and then dropped, so it needs no indexes.
ALTER TABLE trades RENAME TO trades_old;

DROP INDEX IF EXISTS idx_trades_mint;
DROP INDEX IF EXISTS idx_trades_wallet;
DROP INDEX IF EXISTS idx_trades_block_time;
DROP INDEX IF EXISTS idx_trades_mint_time;
DROP INDEX IF EXISTS idx_trades_tx_leg;
DROP INDEX IF EXISTS idx_trades_mint_venue_slot;
DROP INDEX IF EXISTS idx_trades_wallet_mint;
DROP INDEX IF EXISTS idx_trades_mint_slot_leg;

-- 2. Partitioned parent — identical columns to the original trades table.
CREATE TABLE trades (
    id                      UUID                NOT NULL DEFAULT uuid_generate_v4(),
    mint_address            TEXT                NOT NULL,
    wallet_address          TEXT                NOT NULL,
    trade_type              TEXT                NOT NULL CHECK (trade_type IN ('buy', 'sell')),
    sol_amount              DOUBLE PRECISION    NOT NULL,
    token_amount            DOUBLE PRECISION    NOT NULL,
    price_per_token         DOUBLE PRECISION    NOT NULL,
    tx_signature            TEXT                NOT NULL,
    slot                    BIGINT              NOT NULL,
    block_time              TIMESTAMPTZ         NOT NULL,
    virtual_sol_reserves    DOUBLE PRECISION,
    virtual_token_reserves  DOUBLE PRECISION,
    real_sol_reserves       DOUBLE PRECISION,
    real_token_reserves     DOUBLE PRECISION,
    ix_type                 TEXT                NOT NULL DEFAULT 'Unknown',
    ix_labels               JSONB               NOT NULL DEFAULT '[]',
    leg_index               INTEGER             NOT NULL DEFAULT 0,
    received_at             TIMESTAMPTZ         NOT NULL,
    venue                   TEXT                NOT NULL DEFAULT 'curve'
                                CHECK (venue IN ('curve', 'amm')),
    PRIMARY KEY (block_time, id)
) PARTITION BY RANGE (block_time);

-- Dedup unique key (partition col included; deterministic per tx ⇒ still exact).
CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_tx_leg
    ON trades (tx_signature, leg_index, block_time);

-- Non-unique indexes on the parent cascade to all (incl. future) partitions.
CREATE INDEX IF NOT EXISTS idx_trades_mint          ON trades(mint_address);
CREATE INDEX IF NOT EXISTS idx_trades_wallet        ON trades(wallet_address);
CREATE INDEX IF NOT EXISTS idx_trades_block_time    ON trades(block_time DESC);
CREATE INDEX IF NOT EXISTS idx_trades_mint_time     ON trades(mint_address, block_time DESC);
CREATE INDEX IF NOT EXISTS idx_trades_mint_venue_slot ON trades(mint_address, venue, slot DESC);
CREATE INDEX IF NOT EXISTS idx_trades_wallet_mint   ON trades(wallet_address, mint_address);
CREATE INDEX IF NOT EXISTS idx_trades_mint_slot_leg ON trades(mint_address, slot, leg_index);

-- 3. Weekly partition helpers (Mon–Mon), analogous to the raw_transactions fns.
CREATE OR REPLACE FUNCTION ensure_trades_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_end   date := v_start + 7;
    v_name  text := format('trades_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF trades FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamptz, v_end::timestamptz
    );
END;
$$;

CREATE OR REPLACE FUNCTION drop_trades_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_name  text := format('trades_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- 4. Pre-create every weekly partition spanning the existing data's range
--    (min(block_time) .. now()+2w) so the copy below always has a target. Rows
--    older than the retention window are reclaimed on the next maintenance tick.
DO $$
DECLARE
    w     date;
    v_end date := date_trunc('week', (now() + interval '2 weeks'))::date;
BEGIN
    SELECT date_trunc('week', min(block_time))::date INTO w FROM trades_old;
    IF w IS NULL THEN
        w := date_trunc('week', (now() - interval '5 weeks'))::date;
    END IF;
    WHILE w <= v_end LOOP
        PERFORM ensure_trades_partition(w);
        w := w + 7;
    END LOOP;
END $$;

-- 5. Copy historical rows into the partitioned table.
INSERT INTO trades
    (id, mint_address, wallet_address, trade_type, sol_amount, token_amount,
     price_per_token, tx_signature, slot, block_time,
     virtual_sol_reserves, virtual_token_reserves, real_sol_reserves,
     real_token_reserves, ix_type, ix_labels, leg_index, received_at, venue)
SELECT
     id, mint_address, wallet_address, trade_type, sol_amount, token_amount,
     price_per_token, tx_signature, slot, block_time,
     virtual_sol_reserves, virtual_token_reserves, real_sol_reserves,
     real_token_reserves, ix_type, ix_labels, leg_index, received_at, venue
FROM trades_old;

-- 6. Drop the old table.
DROP TABLE trades_old;
