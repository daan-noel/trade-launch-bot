ALTER TABLE tokens
    ADD COLUMN IF NOT EXISTS is_cashback_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_tokens_is_cashback_enabled ON tokens(is_cashback_enabled);
