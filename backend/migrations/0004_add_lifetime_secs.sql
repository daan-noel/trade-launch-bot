-- Add lifetime_secs to tokens_info: seconds from token creation to the last
-- meaningful trade (sol_amount >= DEAD_MEANINGFUL_TRADE_SOL). Populated only when
-- is_dead = true; NULL means the token is still alive or has never had a meaningful
-- trade. Written at final metrics flush (trade ingest) and at cache eviction.
ALTER TABLE tokens_info
    ADD COLUMN IF NOT EXISTS lifetime_secs BIGINT;
