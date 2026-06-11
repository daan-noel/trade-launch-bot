-- -------------------------------------------------------------------------
-- raw_transactions (WS ingest) — convert to WEEKLY range partitions on
-- received_at with ~2-month retention, mirroring raw_transactions_grpc (0011).
--
-- Unlike the gRPC table (created empty), this one has existing data, so we:
--   1. rename the live table aside (and free its index/constraint NAMES, since
--      a Postgres table rename does NOT rename its pkey/indexes),
--   2. create the partitioned table + maintenance SQL fns + the retained-window
--      partitions,
--   3. copy ONLY rows within the retention window (~9 weeks) into it — older
--      rows are discarded with the old table, per the chosen ~2-month policy,
--   4. drop the old table (which frees the original index names),
--   5. (re)create the secondary indexes on the new parent.
--
-- Dropping UNIQUE(signature): a unique constraint on a partitioned table must
-- include the partition key (received_at), which isn't stable across reconnect
-- re-delivery and so can't dedup anyway. The repo switches to a plain INSERT;
-- rare duplicate rows are tolerated in this write-only replay/analysis table.
-- Dedup, where it matters, is best-effort upstream (the WS pipeline only writes
-- each signature once per session).
--
-- KEEP_WEEKS here must match ingest::maintenance::KEEP_WEEKS (9).
-- -------------------------------------------------------------------------

-- 1. Move the live table + its occupied names aside.
ALTER TABLE raw_transactions RENAME TO raw_transactions_old;
ALTER TABLE raw_transactions_old RENAME CONSTRAINT raw_transactions_pkey TO raw_transactions_old_pkey;
ALTER INDEX idx_raw_tx_signature RENAME TO idx_raw_tx_signature_old;
ALTER INDEX idx_raw_tx_slot      RENAME TO idx_raw_tx_slot_old;
-- (raw_transactions_signature_key, from the old UNIQUE, doesn't collide with
--  anything new and is dropped with the old table — no need to rename it.)

-- 2. Partitioned replacement (same columns; PK now includes the partition key).
CREATE TABLE raw_transactions (
    id          UUID        NOT NULL DEFAULT uuid_generate_v4(),
    signature   TEXT        NOT NULL,
    slot        BIGINT      NOT NULL,
    block_time  TIMESTAMPTZ NOT NULL,
    raw_data    JSONB       NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (received_at, id)
) PARTITION BY RANGE (received_at);

-- Idempotently create the weekly partition (Mon–Mon) containing p_anchor.
CREATE OR REPLACE FUNCTION ensure_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_end   date := v_start + 7;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF raw_transactions FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamptz, v_end::timestamptz
    );
END;
$$;

-- Drop the weekly partition containing p_anchor, if it exists (retention).
CREATE OR REPLACE FUNCTION drop_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- 3. Pre-create the retained window (now-9w .. now+2w) so the copy + first
--    inserts always have a target partition.
DO $$
DECLARE w date := date_trunc('week', (now() - interval '9 weeks'))::date;
BEGIN
    WHILE w <= date_trunc('week', (now() + interval '2 weeks'))::date LOOP
        PERFORM ensure_raw_partition(w);
        w := w + 7;
    END LOOP;
END $$;

-- 4. Carry forward only the retained window; report what was kept vs dropped.
DO $$
DECLARE
    v_cutoff timestamptz := date_trunc('week', (now() - interval '9 weeks'));
    v_total  bigint;
    v_kept   bigint;
BEGIN
    SELECT count(*) INTO v_total FROM raw_transactions_old;

    INSERT INTO raw_transactions (id, signature, slot, block_time, raw_data, received_at)
    SELECT id, signature, slot, block_time, raw_data, received_at
    FROM raw_transactions_old
    WHERE received_at >= v_cutoff;
    GET DIAGNOSTICS v_kept = ROW_COUNT;

    RAISE NOTICE 'raw_transactions partition migration: kept % of % rows (dropped % older than %)',
        v_kept, v_total, v_total - v_kept, v_cutoff;
END $$;

-- 5. Drop the old table (frees the original index/constraint names) and rebuild
--    the secondary indexes on the new parent (cascade to all partitions).
DROP TABLE raw_transactions_old;

CREATE INDEX idx_raw_tx_signature ON raw_transactions (signature);
CREATE INDEX idx_raw_tx_slot      ON raw_transactions (slot DESC);
