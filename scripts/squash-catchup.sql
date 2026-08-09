-- ============================================================================
-- squash-catchup.sql — bring an EXISTING hunter database up to the end state of
-- the squashed `0001_init.sql` files, so its ledger can then be reconciled.
--
-- WHY THIS FILE EXISTS
-- `scripts/consolidate-migration-ledgers.ps1` rewrites the LEDGER and never the
-- SCHEMA: it stamps "version 1 applied" on the assumption that everything the
-- squashed init creates is already present. That assumption holds only for
-- migrations the database actually ran. Any migration folded into the squash that
-- had NOT yet been applied there will never run — sqlx sees version 1 as done and
-- skips the file forever. The column simply stays missing, and the bin fails at
-- query time rather than at boot, which is the worst possible place to find out.
--
-- Run this BEFORE the reconcile script, on every database, EC2 first (see the
-- ordering note below). It is idempotent (`IF NOT EXISTS` / `CREATE OR REPLACE`)
-- and safe to re-run on a database that is already current.
--
-- ORDER
--   1. EC2 live DB   : psql <ec2-url> -f scripts/squash-catchup.sql
--                      .\scripts\consolidate-migration-ledgers.ps1 -DatabaseUrl <ec2-url> -Ledger core -Apply
--   2. redeploy live
--   3. workstation DB: psql <local-url> -f scripts/squash-catchup.sql
--                      .\scripts\consolidate-migration-ledgers.ps1 -DatabaseUrl <local-url> -Apply
--
-- EC2 first because `hunter/scripts/db-incremental-sync.ps1` copies the SERVER's
-- `_sqlx_migrations` rows into the local mirror (ON CONFLICT DO NOTHING) — sync a
-- stale server ledger into a freshly cleaned local one and versions 2..6 come back.
--
-- The `-Ledger core` on EC2 is deliberate: the live box has no `_lab_migrations`.
--
-- SCOPE — the core chain 0002..0006 and the lab chain 0002. Migration 0003
-- (position_fills backfill) is NOT reproduced: it is a pure data rewrite over
-- pre-existing rows, so re-running it on a database that already has a fills
-- ledger would be a no-op anyway, and on one that does not, the chart gap it fixed
-- is cosmetic. Nothing here drops or rewrites data.
-- ============================================================================

BEGIN;

-- --- core 0002 — rule tags -------------------------------------------------
ALTER TABLE strategy_rules
    ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}'::text[];

-- --- core 0004 — generic-engine exit buckets -------------------------------
ALTER TABLE strategy_run_metrics
    ADD COLUMN IF NOT EXISTS n_exit_dead     INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_metrics  INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_manual   INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_migrated INTEGER NOT NULL DEFAULT 0;

-- --- core 0005 — per-tx network fee ----------------------------------------
-- NULLABLE with no DEFAULT: supported on compressed chunks, rewrites nothing.
ALTER TABLE trades
    ADD COLUMN IF NOT EXISTS fee_lamports BIGINT;

-- --- core 0006 — pnl_pct becomes money-over-capital ------------------------
-- Derived view, no data rewrite. Historical rows are recomputed on read and will
-- show a smaller (sometimes negative) percent — that is the correction.
DROP VIEW IF EXISTS strategy_position_pnl;
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8 / 1e9) AS realized_pnl_sol,
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8
     / NULLIF(p.entry_lamports, 0) * 100.0)  AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END     AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                     AS is_closed
FROM strategy_positions p;

-- --- lab 0002 — retire the NextKill counter --------------------------------
-- Lab-only table; skipped automatically on the EC2 live DB, which never has it.
ALTER TABLE IF EXISTS grouped_sweep_results
    DROP COLUMN IF EXISTS n_exit_next_kill;

COMMIT;

-- ============================================================================
-- raw_txs retention (0001_init policy change, 2026-08-09): 2d/7d -> 1d/3d.
--
-- Outside the transaction above because the policy helpers manage background
-- jobs. `add_*_policy(..., if_not_exists => TRUE)` will NOT overwrite an existing
-- policy with different values — it leaves the old one in place — so the old
-- policies must be removed first. Skipped harmlessly if none exist.
--
-- Measured 2026-08-09 on the live box: raw_txs was 21.5 GB (58% of the database)
-- against trades' 13 GB, and compresses only ~18% because `payload` is one opaque
-- BYTEA. See the rationale block in hunter/core/migrations/0001_init.sql.
-- ============================================================================

SELECT remove_compression_policy('raw_txs', if_exists => TRUE);
SELECT remove_retention_policy('raw_txs',   if_exists => TRUE);
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '1 day',  if_not_exists => TRUE);
SELECT add_retention_policy('raw_txs',   drop_after     => INTERVAL '3 days', if_not_exists => TRUE);

-- Reclaim immediately rather than waiting for the retention job's next tick.
SELECT drop_chunks('raw_txs', older_than => INTERVAL '3 days');

-- ---------------------------------------------------------------------------
-- Verify (expect: tags/n_exit_*/fee_lamports present, pnl_pct over entry_lamports,
-- raw_txs policies at 1 day / 3 days).
-- ---------------------------------------------------------------------------
-- \d strategy_rules
-- \d strategy_run_metrics
-- \d trades
-- SELECT pg_get_viewdef('strategy_position_pnl', true);
-- SELECT proc_name, config FROM timescaledb_information.jobs WHERE hypertable_name = 'raw_txs';
-- SELECT pg_size_pretty(sum(total_bytes)) FROM chunks_detailed_size('raw_txs');
