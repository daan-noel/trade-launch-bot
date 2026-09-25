-- Metric system v2 (docs/roadmap/metric-system-v2-plan.md).
--
-- A fingerprint's per-group `metric_config` becomes its `tags`: named trade lists.
-- The column is renamed here; the rows are converted by the Rust data migration
-- `storage::data_migrations` right after this file applies (the conversion is the
-- engine's `hunter_engine::v1`, which SQL cannot call), together with every rule's
-- params and every run's params snapshot, in one transaction.

ALTER TABLE fingerprints RENAME COLUMN metric_config TO tags;

-- Rust data migrations: one row per migration applied, so each runs once per database.
CREATE TABLE IF NOT EXISTS _data_migrations (
    name        TEXT PRIMARY KEY,
    applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
