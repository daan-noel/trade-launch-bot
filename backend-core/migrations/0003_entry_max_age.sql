-- ============================================================================
-- p_entry_max_age_secs — entry-window ceiling for tpsl2 scalp rules.
--
-- Symmetric to the existing p_entry_min_age_secs floor: the entry window is
-- [min_age, max_age]. NULL/0 = no ceiling (watch until the token dies, capped by
-- the global concurrent-armer limit). Because the bound lives in the shared
-- `find_scalp_entry` gate, sim/paper/real honor the identical window — they can't
-- diverge. Nullable + inert at NULL/0 (the ignore_zero convention), so existing
-- rows default to "until dead", which immediately unblocks real buys.
-- ============================================================================

ALTER TABLE tpsl2_strategy_rules ADD COLUMN IF NOT EXISTS p_entry_max_age_secs BIGINT;
