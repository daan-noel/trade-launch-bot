-- Bundle re-bid tracking: how many times this bundle has been submitted to Jito.
-- Drives the tip-escalation level (0 = first attempt, climbing p95/p99 on retry)
-- and bounds the confirm watcher's auto re-bid on a `dropped` verdict. A submitted
-- bundle that lost the auction is re-planned + re-executed at level = submit_attempts
-- until it lands or the retry budget is spent.
ALTER TABLE bundles ADD COLUMN IF NOT EXISTS submit_attempts INT NOT NULL DEFAULT 0;
