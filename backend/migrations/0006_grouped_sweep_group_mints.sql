-- Option C: store the list of token mints that belong to each sweep group so
-- the token-results drill-in endpoint can load only those N tokens instead of
-- re-loading the entire corpus (which may be 10 k tokens) and filtering down.
--
-- Additive, idempotent. Existing rows have NULL mints → the handler falls back
-- to the full-corpus path for old groups (backward compatible).

ALTER TABLE tpsl2_grouped_sweep_groups
    ADD COLUMN IF NOT EXISTS mints TEXT[];

ALTER TABLE tpsl1_grouped_sweep_groups
    ADD COLUMN IF NOT EXISTS mints TEXT[];
