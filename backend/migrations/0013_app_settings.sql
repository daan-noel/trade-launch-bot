-- Global, server-wide tracking policy. A single-row table (enforced by the
-- `id` boolean primary key + CHECK) holding runtime toggles that govern what the
-- live ingest pipeline records. Persisted so a policy set once survives restarts
-- (unlike the in-memory `live_mode` flag, which resets on every boot).
--
--   track_mayhem          — when false, the pipeline stops ingesting Mayhem-mode
--                           tokens (and evicts already-tracked ones from cache).
--   track_post_migration  — when false, the pipeline stops recording AMM trade
--                           histories for migrated tokens (and clears their
--                           subscribed pools).
--
-- Defaults are ON so first boot preserves the prior "track everything" behavior.
CREATE TABLE IF NOT EXISTS app_settings (
    id                   BOOLEAN PRIMARY KEY DEFAULT TRUE,
    track_mayhem         BOOLEAN NOT NULL DEFAULT TRUE,
    track_post_migration BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT app_settings_singleton CHECK (id)
);

INSERT INTO app_settings (id) VALUES (TRUE) ON CONFLICT (id) DO NOTHING;
