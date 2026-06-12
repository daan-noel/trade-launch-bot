-- Convert app_settings from a single-row JSONB document into a typed key-value
-- store: one row per setting (key TEXT PK, value JSONB, updated_at). This gives
-- per-key atomic upserts (no whole-document read-modify-write, so concurrent
-- updates of different settings can't clobber each other) and per-key audit via
-- updated_at, while staying migration-free for new settings (a new setting is a
-- new key written by the app, never a schema change).
--
-- The document's keys are remapped to dotted, namespaced keys. The application's
-- `AppSettings` struct owns this mapping (see settings_repo.rs `keys`); absent
-- keys fall back to per-setting defaults at load time, so only keys actually set
-- by an operator are migrated here.

ALTER TABLE app_settings RENAME TO app_settings_old;

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Copy each previously-set key out of the old singleton document into its own
-- row under the new namespaced key. `data ? old_key` skips keys never set, so
-- they keep falling back to the struct default instead of persisting a value.
INSERT INTO app_settings (key, value)
SELECT m.new_key, old.data -> m.old_key
FROM app_settings_old old
CROSS JOIN (VALUES
    ('track_mayhem',         'ingest.track_mayhem'),
    ('track_post_migration', 'ingest.track_post_migration'),
    ('timezone',             'ui.timezone'),
    ('price_unit',           'ui.price_unit'),
    ('slippage_bps',         'trade.slippage_bps'),
    ('live',                 'ingest.live')
) AS m(old_key, new_key)
WHERE old.id = 1 AND old.data ? m.old_key
ON CONFLICT (key) DO NOTHING;

DROP TABLE app_settings_old;
