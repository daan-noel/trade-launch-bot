-- Add Pump.mayhem mode tracking to tokens
ALTER TABLE tokens
    ADD COLUMN IF NOT EXISTS is_mayhem_mode BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_tokens_is_mayhem_mode ON tokens(is_mayhem_mode);
