-- ============================================================================
-- 0003 arm_end_detail — `strategy_arms.end_detail`: what an entry was short of.
--
-- `end_reason` buckets an episode; only `unsatisfiable` fails to explain itself.
-- It means a monotonic entry bound was permanently crossed, and since `time` is
-- the only monotonic metric that always reads "the token aged past the entry
-- window" — never which condition it was still failing when the clock ran out.
--
-- The fold captures that set at the disarm instant (`CompiledRule::entry_blockers`)
-- and it lands here. Reconstructing it later is not equivalent: replay is bounded
-- by the trade-retention window, and the armed path has no `params_snapshot`, so a
-- rule edited afterwards would redraw thresholds that never applied to the episode.
--
-- Plan: docs/plans/strategies/arm-ledger.md
-- ============================================================================

ALTER TABLE strategy_arms ADD COLUMN IF NOT EXISTS end_detail JSONB;

COMMENT ON COLUMN strategy_arms.end_detail IS
    'Entry blockers at the disarm instant; set only with end_reason = ''unsatisfiable''. '
    '{blocked_by, killed_by:{metric,threshold,operator}, unmet:[{metric,window_size_sec,'
    'value,conditions}]}. Written by live''s entry_blockers_json (engine SSOT).';

-- The `Blocked by` column's filter/sort and the summary breakdown, both of which
-- read the one extracted key rather than the document. Partial: every row that
-- ended any other way has a NULL detail and belongs in no bucket.
CREATE INDEX IF NOT EXISTS idx_strategy_arms_blocked_by
    ON strategy_arms((end_detail ->> 'blocked_by'), armed_at DESC)
    WHERE end_detail IS NOT NULL;
