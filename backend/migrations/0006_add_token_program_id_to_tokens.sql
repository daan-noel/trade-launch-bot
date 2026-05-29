-- 0006_add_token_program_id_to_tokens.sql
-- Add token_program_id column to tokens table

ALTER TABLE tokens ADD COLUMN IF NOT EXISTS token_program_id TEXT;