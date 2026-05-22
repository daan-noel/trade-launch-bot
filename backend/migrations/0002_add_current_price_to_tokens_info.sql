-- -------------------------------------------------------------------------
ALTER TABLE tokens_info
    ADD COLUMN IF NOT EXISTS current_price DOUBLE PRECISION;
