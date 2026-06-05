-- Drop old wallets table (was auto-populated from trades; no longer needed)
DROP TABLE IF EXISTS wallets;

-- -------------------------------------------------------------------------
-- wallet_profiles
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wallet_profiles (
    id          UUID    PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        TEXT    NOT NULL,
    type        TEXT    NOT NULL CHECK (type IN ('mine', 'trader', 'whale', 'dev')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- -------------------------------------------------------------------------
-- wallets  (manually managed; one wallet belongs to exactly one profile)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wallets (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    profile_id  UUID        NOT NULL REFERENCES wallet_profiles(id) ON DELETE CASCADE,
    address     TEXT        UNIQUE NOT NULL,
    is_tracked  BOOLEAN     NOT NULL DEFAULT TRUE,
    comment     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wallets_profile_id  ON wallets(profile_id);
CREATE INDEX IF NOT EXISTS idx_wallets_address     ON wallets(address);
CREATE INDEX IF NOT EXISTS idx_wallets_is_tracked  ON wallets(is_tracked);
