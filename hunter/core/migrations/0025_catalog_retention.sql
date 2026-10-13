-- Tokens and wallet_dict follow the trades retention horizon.
--
-- A token older than that horizon is deleted once no remaining trade names
-- its mint. tokens_info and token_sync_state cascade. A wallet_dict id that
-- no remaining trade uses (wallet_id or payer_id) is deleted. The horizon is
-- the trades policy's drop_after, read at run time, so this job cannot carry
-- a second copy of the interval.
--
-- Not a hypertable. One row per mint, one row per address, so drop_chunks
-- does not apply. The job is scheduled here and does not run inside this
-- migration: a first pass over a full catalog is too long to sit on boot.

CREATE OR REPLACE PROCEDURE prune_retained_catalog(job_id integer, config jsonb)
LANGUAGE plpgsql AS $$
DECLARE
    horizon interval;
BEGIN
    SELECT (j.config ->> 'drop_after')::interval
      INTO horizon
    FROM timescaledb_information.jobs j
    WHERE j.hypertable_name = 'trades'
      AND j.proc_name = 'policy_retention';

    IF horizon IS NULL THEN
        RAISE EXCEPTION 'trades retention policy is missing';
    END IF;

    -- Hash anti-join. A nested loop over wallet_dict without it does not finish.
    PERFORM set_config('enable_nestloop', 'off', true);

    CREATE TEMP TABLE keep_mints ON COMMIT DROP AS
    SELECT DISTINCT mint_address FROM trades;
    CREATE INDEX ON keep_mints (mint_address);

    DELETE FROM tokens t
    WHERE t.created_at < now() - horizon
      AND NOT EXISTS (
          SELECT 1 FROM keep_mints m WHERE m.mint_address = t.mint_address
      );

    CREATE TEMP TABLE keep_wallets ON COMMIT DROP AS
    SELECT wallet_id AS id FROM trades
    UNION
    SELECT payer_id FROM trades WHERE payer_id IS NOT NULL;
    CREATE INDEX ON keep_wallets (id);

    DELETE FROM wallet_dict d
    WHERE NOT EXISTS (
        SELECT 1 FROM keep_wallets k WHERE k.id = d.id
    );
END;
$$;

DO $body$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM timescaledb_information.jobs
        WHERE proc_schema = 'public'
          AND proc_name = 'prune_retained_catalog'
    ) THEN
        PERFORM add_job(
            proc => 'public.prune_retained_catalog',
            schedule_interval => INTERVAL '1 day',
            initial_start => now() + INTERVAL '1 day'
        );
    END IF;
END
$body$;
