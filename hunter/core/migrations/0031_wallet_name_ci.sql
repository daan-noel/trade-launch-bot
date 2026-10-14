-- A wallet name is a free-form label. Two names that differ only in case are
-- the same name, so the label a page shows always picks out one wallet.
CREATE UNIQUE INDEX IF NOT EXISTS trader_wallets_name_ci ON trader_wallets (lower(name));
