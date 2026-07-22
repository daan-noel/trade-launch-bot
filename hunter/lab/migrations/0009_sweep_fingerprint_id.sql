-- Saved-fingerprint scope for a grouped sweep run.
--
-- The sweep page can now scope its corpus to a saved fingerprint the same way
-- Flow discovery does: the run keeps only tokens the engine MATCHES against that
-- fingerprint (`hunter_engine::fingerprint::matches` — exact + bucket axes), the
-- one SSOT the live entry gate uses. That scope is not expressible as the run's
-- `field_filters` (those compare exact values, the engine matches by bucket), so
-- it is persisted on its own column: without it a re-run / token-results reload
-- would silently sweep the UNSCOPED corpus.
--
-- NULL on legacy rows and on manual group-by / filter runs. No FK to
-- `strategy_fingerprints` on purpose — deleting a fingerprint must not delete (or
-- block deleting) the sweep history that was scoped by it; the UI falls back to
-- showing the raw id when the row is gone.

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS fingerprint_id UUID;
