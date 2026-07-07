-- Metadata single-source-of-truth cutover. Before this, token identity
-- (name/symbol/uri) was inlined in `launch_templates.params` AND authored as
-- `metadata_templates` rows AND overridable per-launch — three copies that could
-- drift, with the `metadata_templates` table never actually read at launch. Now
-- `launch_templates` REFERENCES a `metadata_templates` row and the launcher
-- resolves name/symbol/uri from it (launcher::service::execute_launch); the
-- inlined `params.{name,symbol,uri}` are removed.

-- A preset authored outside the pin flow (e.g. one backfilled from a legacy
-- launch template below) has no separately-pinned image — the image is embedded
-- in the metadata JSON at `uri`. Relax the NOT NULL so those rows are valid.
ALTER TABLE metadata_templates ALTER COLUMN image_uri DROP NOT NULL;

-- The FK. ON DELETE SET NULL: deleting a metadata preset unlinks templates
-- (they become non-launchable until relinked) rather than cascading a delete.
ALTER TABLE launch_templates
    ADD COLUMN IF NOT EXISTS metadata_template_id UUID
    REFERENCES metadata_templates(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_launch_templates_metadata
    ON launch_templates(metadata_template_id);

-- Backfill: every existing launch template that inlined a name becomes its own
-- metadata_templates row, linked back. symbol/uri COALESCE to '' only to satisfy
-- NOT NULL on a malformed legacy row; a well-formed template carries all three.
DO $$
DECLARE
    r        RECORD;
    new_id   UUID;
BEGIN
    FOR r IN
        SELECT id, template_name, params
        FROM launch_templates
        WHERE metadata_template_id IS NULL
          AND params->>'name' IS NOT NULL
    LOOP
        INSERT INTO metadata_templates (template_name, name, symbol, uri)
        VALUES (
            r.template_name,
            r.params->>'name',
            COALESCE(r.params->>'symbol', ''),
            COALESCE(r.params->>'uri', '')
        )
        RETURNING id INTO new_id;

        UPDATE launch_templates SET metadata_template_id = new_id WHERE id = r.id;
    END LOOP;
END $$;

-- Drop the now-duplicated inlined metadata from the JSONB brain. The remaining
-- params (dev_buy_quote, slippage_bps, bundle_*, leg_structures) still parse into
-- launcher::service::PumpfunTemplateParams.
UPDATE launch_templates
SET params = params - 'name' - 'symbol' - 'uri'
WHERE params ?| array['name', 'symbol', 'uri'];
