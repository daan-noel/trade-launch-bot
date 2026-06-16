-- ============================================================================
-- Switch `trades` and `raw_transactions` from WEEKLY to DAILY range partitions,
-- and set retention to 1 month (30 days) for both.
--
-- Migration 0001 only runs on a fresh DB; this migration carries an existing DB
-- over. It is intentionally NON-DESTRUCTIVE:
--   * The four ensure_*/drop_* helpers are CREATE OR REPLACE'd to the daily
--     definitions (identical to the squashed 0001).
--   * We seed daily partitions across the retained window, but any day already
--     covered by a lingering WEEKLY partition would overlap — Postgres rejects
--     overlapping `PARTITION OF`, so each day is wrapped in its own block and an
--     overlap is silently skipped. Inserts for those days keep landing in the
--     old weekly partition, which the maintenance task drops once its Monday
--     anchor ages past the 28-day cutoff. After ~5-6 weeks the tables are fully
--     daily with zero data loss.
--
-- Retention is enforced by ingest::maintenance (KEEP_DAYS = 30); this file only
-- redefines the partition span and seeds the initial window.
-- ============================================================================

-- trades (partitioned on block_time) ----------------------------------------
CREATE OR REPLACE FUNCTION ensure_trades_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_end   date := v_start + 1;
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
    v_start date := p_anchor;
    v_name  text := format('trades_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- raw_transactions (partitioned on received_at) -----------------------------
CREATE OR REPLACE FUNCTION ensure_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_end   date := v_start + 1;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF raw_transactions FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamptz, v_end::timestamptz
    );
END;
$$;

CREATE OR REPLACE FUNCTION drop_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- Seed daily partitions for now-1mo .. now+2d, skipping any day that still
-- overlaps a lingering weekly partition (handled gracefully per-day).
DO $$
DECLARE
    d date := (now() - interval '1 month')::date;
    d_end date := (now() + interval '2 days')::date;
BEGIN
    WHILE d <= d_end LOOP
        BEGIN
            PERFORM ensure_trades_partition(d);
        EXCEPTION WHEN others THEN
            -- overlaps an existing weekly partition; it will age out — skip.
            NULL;
        END;
        BEGIN
            PERFORM ensure_raw_partition(d);
        EXCEPTION WHEN others THEN
            NULL;
        END;
        d := d + 1;
    END LOOP;
END $$;
