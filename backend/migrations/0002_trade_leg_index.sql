-- Allow multiple pump trades in the same transaction (multi-leg txs).
ALTER TABLE trades ADD COLUMN IF NOT EXISTS leg_index INTEGER NOT NULL DEFAULT 0;

ALTER TABLE trades DROP CONSTRAINT IF EXISTS trades_tx_signature_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_tx_leg
    ON trades (tx_signature, leg_index);
