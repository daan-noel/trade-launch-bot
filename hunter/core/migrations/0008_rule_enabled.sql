-- 0008_rule_enabled.sql — soft-archive flag for strategy rules.
--
-- Orthogonal to `is_active` (Active/Idle live arming). Disabled rules stay in
-- the DB for reference but are hidden from default UI lists and cannot be
-- activated until re-enabled.

ALTER TABLE strategy_rules
    ADD COLUMN is_enabled BOOLEAN NOT NULL DEFAULT true;

CREATE INDEX idx_strategy_rules_enabled ON strategy_rules (is_enabled);
