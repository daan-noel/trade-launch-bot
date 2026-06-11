-- -------------------------------------------------------------------------
-- raw_transactions_grpc — convert to WEEKLY range partitions on received_at,
-- with ~2-month retention (managed by the app's partition-maintenance task).
--
-- The table was created empty in 0009 (gRPC ingest not live yet), so dropping
-- and recreating it partitioned is safe. Partition name = the week's Monday,
-- e.g. raw_transactions_grpc_2026_06_08.
--
-- No UNIQUE(signature): a unique constraint on a partitioned table must include
-- the partition key, and received_at isn't stable across fork re-delivery — so
-- raw rows are deduped best-effort by the from_slot=last+1 replay, not a DB
-- constraint. Duplicate rows are tolerated in this write-only analysis table.
-- -------------------------------------------------------------------------

DROP TABLE IF EXISTS raw_transactions_grpc;

CREATE TABLE raw_transactions_grpc (
    id          UUID        NOT NULL DEFAULT uuid_generate_v4(),
    signature   TEXT        NOT NULL,
    slot        BIGINT      NOT NULL,
    block_time  TIMESTAMPTZ NOT NULL,
    raw_data    JSONB       NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (received_at, id)
) PARTITION BY RANGE (received_at);

-- Non-unique indexes on the parent cascade to all (incl. future) partitions.
CREATE INDEX idx_raw_tx_grpc_signature ON raw_transactions_grpc (signature);
CREATE INDEX idx_raw_tx_grpc_slot      ON raw_transactions_grpc (slot DESC);

-- Idempotently create the weekly partition (Mon–Mon) containing p_anchor.
CREATE OR REPLACE FUNCTION ensure_raw_grpc_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_end   date := v_start + 7;
    v_name  text := format('raw_transactions_grpc_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF raw_transactions_grpc FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamptz, v_end::timestamptz
    );
END;
$$;

-- Drop the weekly partition containing p_anchor, if it exists (retention).
CREATE OR REPLACE FUNCTION drop_raw_grpc_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_name  text := format('raw_transactions_grpc_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- Pre-create partitions from ~2 months back through 2 weeks ahead so inserts
-- never hit a missing partition before the maintenance task first runs.
DO $$
DECLARE w date := date_trunc('week', (now() - interval '9 weeks'))::date;
BEGIN
    WHILE w <= date_trunc('week', (now() + interval '2 weeks'))::date LOOP
        PERFORM ensure_raw_grpc_partition(w);
        w := w + 7;
    END LOOP;
END $$;
