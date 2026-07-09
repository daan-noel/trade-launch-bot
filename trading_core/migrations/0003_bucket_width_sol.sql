-- ===========================================================================
-- 0003 — fingerprint matching: tolerance_pct  ->  bucket_width_sol
-- ===========================================================================
-- The continuous SOL fingerprint axes (initial_buy / max_cost / spendable /
-- first_slot buy+sell) no longer match by a ±% tolerance band; they match by
-- half-open bucket membership at a single per-rule width `bucket_width_sol` (SOL),
-- the SAME width the grouped sweep + creation-stats dashboard bin by. This makes
-- "what I swept = what I run" hold by construction (see
-- bucket-matcher-unify-plan.md).
--
-- The old `tolerance_pct` was a relative percent with no meaning as an absolute
-- SOL width, so every existing rule is reset to the default 0.1-SOL bucket
-- (`trading_core::grouping::SOL_BUCKET_WIDTH`). Idempotent: only touches rows that
-- still carry the old key or lack the new one.

UPDATE strategy_rules
SET params = (params - 'tolerance_pct') || jsonb_build_object('bucket_width_sol', 0.1)
WHERE params ? 'tolerance_pct' OR NOT (params ? 'bucket_width_sol');

-- Frozen activation snapshots carry a copy of the rule's params; keep them in the
-- new vocabulary too so a re-read never surfaces the retired key.
UPDATE strategy_runs
SET params_snapshot = (params_snapshot - 'tolerance_pct') || jsonb_build_object('bucket_width_sol', 0.1)
WHERE params_snapshot ? 'tolerance_pct' OR NOT (params_snapshot ? 'bucket_width_sol');
