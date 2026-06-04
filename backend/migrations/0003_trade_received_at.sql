-- Ingest-time precision (Utc::now) for UI ordering; block_time stays chain seconds.
ALTER TABLE trades ADD COLUMN IF NOT EXISTS received_at TIMESTAMPTZ;

UPDATE trades t
SET received_at = r.received_at
FROM raw_transactions r
WHERE r.signature = t.tx_signature
  AND t.received_at IS NULL;

UPDATE trades SET received_at = block_time WHERE received_at IS NULL;

ALTER TABLE trades ALTER COLUMN received_at SET NOT NULL;
