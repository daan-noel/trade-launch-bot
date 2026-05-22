-- -------------------------------------------------------------------------
ALTER TABLE tokens_info
    ADD COLUMN IF NOT EXISTS is_rugged BOOLEAN NOT NULL DEFAULT FALSE;
