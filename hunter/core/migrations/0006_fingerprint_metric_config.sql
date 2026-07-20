-- Volume-flow split V0: per-fingerprint metric-group config (not identity).
-- Top-level keys = metric group names; `m_flow_split.volume_ix_patterns` is the
-- first consumer (see hunter/docs/roadmap/volume-flow-split-plan.md).
-- `find_or_create` must NOT match on this column — patterns are configuration.

ALTER TABLE fingerprints
    ADD COLUMN IF NOT EXISTS metric_config JSONB NOT NULL DEFAULT '{}'::jsonb;
