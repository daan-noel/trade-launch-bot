-- A paused wallet takes no new entry; its open positions still exit. Per box:
-- wallet-sync does not copy it.
ALTER TABLE trader_wallets ADD COLUMN IF NOT EXISTS paused BOOLEAN NOT NULL DEFAULT false;
