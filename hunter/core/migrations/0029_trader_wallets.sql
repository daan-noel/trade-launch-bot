-- Hunter live trading wallets. The private key is not here: `key_ref` is the
-- filename of an envelope under WALLET_KEYSTORE. Lab does not read these tables.
-- The database sync script does not copy them; wallet-sync.ps1 does.

CREATE TABLE IF NOT EXISTS trader_wallets (
    pubkey      TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    status      TEXT NOT NULL CHECK (status IN ('active', 'draining', 'archived')),
    key_ref     TEXT NOT NULL,
    is_primary  BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ
);

-- One primary among the rows that claim it. Archived rows are never primary.
CREATE UNIQUE INDEX IF NOT EXISTS trader_wallets_one_primary
    ON trader_wallets ((true)) WHERE is_primary;

CREATE TABLE IF NOT EXISTS trader_nonces (
    pubkey    TEXT PRIMARY KEY,
    authority TEXT NOT NULL REFERENCES trader_wallets(pubkey),
    status    TEXT NOT NULL CHECK (status IN ('live', 'retired'))
);

CREATE INDEX IF NOT EXISTS trader_nonces_authority ON trader_nonces (authority);

CREATE TABLE IF NOT EXISTS wallet_moves (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_wallet TEXT NOT NULL REFERENCES trader_wallets(pubkey),
    to_wallet   TEXT NOT NULL REFERENCES trader_wallets(pubkey),
    step        TEXT NOT NULL,
    error       TEXT,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS wallet_moves_open
    ON wallet_moves (started_at DESC) WHERE finished_at IS NULL;

-- Which active wallet a rule spends. Absent row = the primary at entry time.
-- Its own table so a lab save of strategy_rules and the database sync leave it.
CREATE TABLE IF NOT EXISTS rule_wallets (
    rule_id UUID PRIMARY KEY REFERENCES strategy_rules(id) ON DELETE CASCADE,
    wallet  TEXT NOT NULL REFERENCES trader_wallets(pubkey)
);
