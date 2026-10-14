-- What was done to each trading wallet: created, imported, nonces provisioned
-- or attached, SOL sent, renamed, archived, restored, moved. One row per action,
-- with the transaction signatures it landed when it sent any. The Wallets page
-- reads it per wallet; nothing decides on it.

CREATE TABLE IF NOT EXISTS wallet_activity (
    id         BIGSERIAL PRIMARY KEY,
    wallet     TEXT        NOT NULL REFERENCES trader_wallets(pubkey),
    kind       TEXT        NOT NULL,
    detail     JSONB       NOT NULL DEFAULT '{}'::jsonb,
    signatures TEXT[]      NOT NULL DEFAULT '{}'::text[],
    at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS wallet_activity_wallet_at ON wallet_activity (wallet, at DESC);
