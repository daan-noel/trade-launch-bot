-- Authored token-metadata content (name/symbol/description/socials + a pinned
-- image), resolved to a hosted off-chain JSON `uri` via Pinata/IPFS. This is the
-- SAME `uri` shape `launch_templates.params.uri` / a per-launch `LaunchRequest.uri`
-- override consumes (see launcher::service::PumpfunTemplateParams) — a template
-- here is a reusable content preset, not itself part of the create-tx path.
--
-- Nothing is written to this table unless BOTH the image and the metadata JSON
-- are pinned (launcher::metadata_upload::create_metadata_template) — a partial
-- upload isn't a useful row, so there's no draft/pending status to track.

CREATE TABLE IF NOT EXISTS metadata_templates (
    id            UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_name TEXT         NOT NULL,
    name          TEXT         NOT NULL,
    symbol        TEXT         NOT NULL,
    description   TEXT,
    twitter       TEXT,
    telegram      TEXT,
    website       TEXT,
    image_uri     TEXT         NOT NULL,   -- ipfs://<cid> — the pinned image
    uri           TEXT         NOT NULL,   -- ipfs://<cid> — the pinned metadata JSON
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_metadata_templates_created ON metadata_templates(created_at DESC);
