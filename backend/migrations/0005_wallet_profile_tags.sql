-- Global tag library
CREATE TABLE IF NOT EXISTS wallet_profile_tags (
    id         UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name       TEXT        NOT NULL UNIQUE,
    color      TEXT        NOT NULL DEFAULT '#6366f1',
    comment    TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Add tag_ids array column to wallet_profiles
ALTER TABLE wallet_profiles
    ADD COLUMN IF NOT EXISTS tag_ids UUID[] NOT NULL DEFAULT '{}';
