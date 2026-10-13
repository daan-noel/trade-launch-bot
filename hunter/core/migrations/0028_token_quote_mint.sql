-- Quote asset a coin is priced in. NULL is SOL, including a create that passed
-- wrapped SOL. A row with no quote recorded is SOL. Any other mint is USDC or a
-- pump-coin quote. Non-SOL coins are not inserted while INGEST_NON_SOL_QUOTE
-- is off; the column is the place that switch writes.
ALTER TABLE tokens ADD COLUMN IF NOT EXISTS quote_mint TEXT;
