-- 0009_tokens_info_write_amplification.sql — cut ingest write amplification on the
-- hot `tokens_info` upsert path.
--
-- WHY. `tokens_info` is upserted once per distinct mint on every ingest flush
-- (~150 ms) — 563k UPDATEs in a 21h window on the live box. Each secondary index
-- on a changing column forces the UPDATE off the HOT path (a HOT tuple can only be
-- created when NO indexed column changes), so every one of these indexes multiplies
-- the write + WAL + autovacuum cost of the hottest write in the system. Measured on
-- prod: HOT-update ratio 0.17%, 1216 autovacuums, ~8x write amplification, and this
-- churn was the chronic I/O pressure that let a single `pool.acquire()` stall snowball
-- into the 2h ingest freeze.
--
-- The seven secondary indexes below all had idx_scan = 0 over that same 21h window
-- (only the PK `tokens_info_pkey` was used — 1.47M scans). The `tokens` list query
-- pages/sorts the full universe and the planner chose a seqscan+sort regardless, so
-- these indexes bought reads nothing while taxing every write.
--
-- REVERSAL. If a token-list sort ever regresses (a single-column ORDER BY on the
-- table that the planner *would* use), recreate the exact index below and re-check
-- `pg_stat_user_indexes.idx_scan` after real traffic — do not restore blind:
--
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_is_dead       ON tokens_info(is_dead);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_volume_sol    ON tokens_info(volume_sol DESC);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_last_trade_at ON tokens_info(last_trade_at DESC);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_trade_count   ON tokens_info(trade_count   DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_current_price ON tokens_info(current_price DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_price     ON tokens_info(ath_price     DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_timestamp ON tokens_info(ath_timestamp DESC NULLS LAST);

DROP INDEX IF EXISTS idx_tokens_info_is_dead;
DROP INDEX IF EXISTS idx_tokens_info_volume_sol;
DROP INDEX IF EXISTS idx_tokens_info_last_trade_at;
DROP INDEX IF EXISTS idx_tokens_info_trade_count;
DROP INDEX IF EXISTS idx_tokens_info_current_price;
DROP INDEX IF EXISTS idx_tokens_info_ath_price;
DROP INDEX IF EXISTS idx_tokens_info_ath_timestamp;

-- fillfactor=80 (C2) — the complement to the drops above. HOT updates also need
-- free space on the row's own page; with the secondary indexes gone the upsert is
-- now HOT-eligible, and reserving ~20% per page gives it room to stay heap-only
-- instead of spilling to a new page. Applies to pages written from here on; existing
-- pages adopt it as they are rewritten. To apply immediately, run once, MANUALLY
-- (VACUUM FULL cannot run inside this migration's transaction, and it briefly locks
-- the table — trivial on this ~5 MB table):
--
--   VACUUM FULL tokens_info;
--
-- C1 (infra, cannot be set from a migration — note for the next server restart):
-- `autovacuum_max_workers` is 10 on the 2-vCPU / 4 GB box. Cap it at 3 in
-- postgresql.conf (needs a restart) so a vacuum burst can't put 10 workers — up to
-- ~2.5 GB of maintenance_work_mem — against 2 cores and one disk.
ALTER TABLE tokens_info SET (fillfactor = 80);
