-- Global, server-wide settings store. A single-row table (enforced by the `id`
-- boolean primary key + CHECK) holding ALL app settings as one JSONB document.
--
-- The document's shape is owned by the application's `AppSettings` struct, not by
-- this schema: adding a setting is a struct field with a default, never a
-- migration. Missing keys fall back to per-field defaults on read, so old rows
-- stay forward/backward compatible. Persisted so policy survives restarts (unlike
-- the in-memory `live_mode` flag).
--
-- Current keys: track_mayhem (bool), track_post_migration (bool),
-- timezone (string|null), price_unit ("SOL"|"USD"|null). null = unset.
-- `id` is a fixed sentinel pinned to 1: the CHECK + primary key make a second
-- row impossible, so this table always holds exactly one settings document.
CREATE TABLE IF NOT EXISTS app_settings (
    id         INTEGER PRIMARY KEY DEFAULT 1,
    data       JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT app_settings_singleton CHECK (id = 1)
);

INSERT INTO app_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;
