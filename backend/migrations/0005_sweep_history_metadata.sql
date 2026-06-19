-- =========================================================================
-- Sweep history management — persist the run's full launch config + a label.
--
-- The run header used to drop the corpus filters and the cap knobs after the
-- sweep started: `ix_labels_filter` / per-field `field_filters` were applied
-- in-memory then forgotten, and only the *realized* `token_count`/`combo_count`
-- were stored (never the `token_cap`/`max_combos` the user actually typed). So a
-- saved run couldn't answer "what was this swept for?" and couldn't be re-run
-- faithfully. These columns close that gap, plus a user-editable `label`:
--
--   ix_labels_filter — JSON array of the exact instruction-label set the corpus
--                      was pinned to (NULL = no filter / grouped by ix_labels).
--   field_filters    — JSON map of per-field value filters (cu_price, cashback,
--                      …) the corpus was pinned to (NULL = none).
--   token_cap        — the per-run token cap the form submitted (NULL = legacy /
--                      backend default).
--   max_combos       — the per-group combo-cap override (NULL = backend default).
--   label            — optional user-given name for the run (NULL = unnamed →
--                      UI falls back to timestamp + grouping hint).
--
-- Additive, idempotent, nullable on both per-strategy `_runs` tables. Existing
-- rows predate this → they read NULL and the UI renders "—".
-- =========================================================================

ALTER TABLE tpsl2_grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS ix_labels_filter JSONB,
    ADD COLUMN IF NOT EXISTS field_filters    JSONB,
    ADD COLUMN IF NOT EXISTS token_cap        INTEGER,
    ADD COLUMN IF NOT EXISTS max_combos       INTEGER,
    ADD COLUMN IF NOT EXISTS label            TEXT;

ALTER TABLE tpsl1_grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS ix_labels_filter JSONB,
    ADD COLUMN IF NOT EXISTS field_filters    JSONB,
    ADD COLUMN IF NOT EXISTS token_cap        INTEGER,
    ADD COLUMN IF NOT EXISTS max_combos       INTEGER,
    ADD COLUMN IF NOT EXISTS label            TEXT;
