-- 0005_retire_legacy_strategies.sql — drop the retired legacy rule table (Phase 7).
--
-- Phase 7 deleted the named tpsl1/tpsl2/swing1 strategy stack from the code (the
-- generic fingerprint+metrics engine now owns everything). `0004` renamed the old
-- `strategy_rules` table to `strategy_rules_legacy` and kept it as a read-only
-- reference while the code still compiled against it. With the last readers
-- re-pointed onto the generic `strategy_rules` table (RuleRepo), nothing reads it
-- anymore, so drop it.
--
-- The old legacy rules were already copied into the generic `strategy_rules` table
-- by the one-off data migration, so no rows of interest are lost. `CASCADE` clears
-- the FK from any `strategy_positions.rule_id` that still pointed at a legacy rule
-- (those columns are `ON DELETE SET NULL`, so the position rows survive).

DROP TABLE IF EXISTS strategy_rules_legacy CASCADE;

-- NOTE (deliberately NOT dropped here): `strategy_positions.strategy_id` and
-- `strategy_runs.strategy_id` still hold the now-constant sentinel `'generic'`.
-- They are harmless and woven through the hot `StrategyPosition`/`StrategyRun`
-- models + several repo queries; dropping them is a model refactor, not a schema
-- tidy, so it is left as a separate follow-up rather than bundled into this drop.
