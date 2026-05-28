-- Migration: Add token_program_id to tokens and positions
-- Created: 2026-05-28

BEGIN;

-- Add token_program_id to tokens (nullable for backfill)
ALTER TABLE tokens
ADD COLUMN IF NOT EXISTS token_program_id TEXT;

-- Add token_program_id to positions (nullable)
ALTER TABLE positions
ADD COLUMN IF NOT EXISTS token_program_id TEXT;

-- Optional: create index to speed lookups by token_program_id
CREATE INDEX IF NOT EXISTS idx_tokens_token_program_id ON tokens(token_program_id);
CREATE INDEX IF NOT EXISTS idx_positions_token_program_id ON positions(token_program_id);

COMMIT;
