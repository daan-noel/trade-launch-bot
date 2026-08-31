-- ============================================================================
-- ix_pattern_sets — typed lens (exact vs templates) + fee pins on exact rows
--
-- A trader-analysis lens is ONE vocabulary: exact ordered `ix_labels` (optional
-- fee pins, same catch-all-vs-pin rule as a fingerprint's ix_patterns) OR
-- template grain ids (`program|CU|ATA|N|S|F`). The set picker is the switch;
-- kind is set at insert and never updated.
--
-- Existing rows stay `kind = 'exact'` with their current `patterns` JSON.
-- `working_templates` is empty on those rows.
-- ============================================================================

ALTER TABLE ix_pattern_sets
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'exact';

ALTER TABLE ix_pattern_sets
    DROP CONSTRAINT IF EXISTS ix_pattern_sets_kind_check;

ALTER TABLE ix_pattern_sets
    ADD CONSTRAINT ix_pattern_sets_kind_check
    CHECK (kind IN ('exact', 'templates'));

ALTER TABLE ix_pattern_sets
    ADD COLUMN IF NOT EXISTS working_templates JSONB NOT NULL DEFAULT '[]'::jsonb;
