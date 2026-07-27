-- Corpus freshness for a grouped sweep run: the newest trade the run actually saw.
--
-- The sweep reads the sealed Parquet lake ONLY; `simulate` splices the fresh PG tail
-- on top of it (`sim_fetch::pg_tail_beyond_lake`). So a stale lake export silently
-- makes the two disagree in the worst possible way: the sweep freezes positions as
-- `Open (est)` at hours-old prices while a simulate over the same rule watches those
-- same tokens die. Nothing in the UI said how old the data was, and the mismatch read
-- as a bug in the engine rather than in the export schedule (2026-07-26 investigation
-- — see docs/plans/sweep/sim-parity.md).
--
-- This is the corpus-wide `max(block_time)`, i.e. the same instant the frozen-tail
-- resolve (D1) anchors its horizon on, captured once at corpus load and stored so the
-- run can say "data through HH:MM" forever after. NULL on rows written before this
-- column existed (and on a trade-less corpus) — the UI shows "unknown" rather than
-- inventing a time.

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS corpus_last_trade_at TIMESTAMPTZ;
