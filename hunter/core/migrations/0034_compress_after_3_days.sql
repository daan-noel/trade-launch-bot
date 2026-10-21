-- `trades` and `strategy_arms` compress after 3 days.
--
-- An uncompressed `trades` day is ~2 GB on the live box and its compressed form
-- ~0.4 GB, so every uncompressed day is the bulk of the disk. 3 days still clears
-- every write that targets a settled chunk: the trade insert is DO NOTHING (a
-- late leg lands in a compressed chunk without decompressing it), an arm episode
-- ends within its token's life (minutes), and the dead-slot sweep deletes legs
-- minutes after finalization.
--
-- `ARM_COMPRESS_AFTER_DAYS` (arm_repo.rs) is pinned to the `strategy_arms`
-- policy below by `compress_horizon_matches_the_migration`.
--
-- Retention stays declared in 0001/0002; a bin with `TRADES_RETENTION_DAYS` set
-- overrides the `trades` horizon at boot (`storage::timescale::apply_trades_retention`).

SELECT remove_compression_policy('trades', if_exists => TRUE);
SELECT add_compression_policy('trades', compress_after => INTERVAL '3 days');

SELECT remove_compression_policy('strategy_arms', if_exists => TRUE);
SELECT add_compression_policy('strategy_arms', compress_after => INTERVAL '3 days');
