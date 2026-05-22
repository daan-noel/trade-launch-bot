-- Full database initialization script for meme-trading backend

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- -------------------------------------------------------------------------
-- tokens
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tokens (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address            TEXT        UNIQUE NOT NULL,
    creator_wallet          TEXT        NOT NULL,
    name                    TEXT        NOT NULL,
    symbol                  TEXT        NOT NULL,
    bonding_curve_address   TEXT,
    initial_supply_token    BIGINT,
    initial_buy_sol         DOUBLE PRECISION,
    cu_limit                BIGINT,
    cu_price                BIGINT,
    ix_labels               JSONB       NOT NULL DEFAULT '[]',
    creation_tx_signature   TEXT        NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at     ON tokens(created_at DESC);

-- -------------------------------------------------------------------------
-- wallets
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wallets (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    address         TEXT        UNIQUE NOT NULL,
    first_seen_at   TIMESTAMPTZ NOT NULL,
    last_seen_at    TIMESTAMPTZ NOT NULL,
    is_flagged      BOOLEAN     NOT NULL DEFAULT FALSE,
    flag_reason     TEXT
);

CREATE INDEX IF NOT EXISTS idx_wallets_flagged ON wallets(is_flagged) WHERE is_flagged = TRUE;

-- -------------------------------------------------------------------------
-- trades
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS trades (
    id                      UUID                PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address            TEXT                NOT NULL,
    wallet_address          TEXT                NOT NULL,
    trade_type              TEXT                NOT NULL CHECK (trade_type IN ('buy', 'sell')),
    sol_amount              DOUBLE PRECISION    NOT NULL,
    token_amount            DOUBLE PRECISION    NOT NULL,
    price_per_token         DOUBLE PRECISION    NOT NULL,
    tx_signature            TEXT                UNIQUE NOT NULL,
    slot                    BIGINT              NOT NULL,
    block_time              TIMESTAMPTZ         NOT NULL,
    virtual_sol_reserves    DOUBLE PRECISION,
    virtual_token_reserves  DOUBLE PRECISION,
    real_sol_reserves       DOUBLE PRECISION,
    real_token_reserves     DOUBLE PRECISION,
    ix_type                 TEXT                NOT NULL DEFAULT 'Unknown',
    ix_labels               JSONB               NOT NULL DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS idx_trades_mint          ON trades(mint_address);
CREATE INDEX IF NOT EXISTS idx_trades_wallet        ON trades(wallet_address);
CREATE INDEX IF NOT EXISTS idx_trades_block_time    ON trades(block_time DESC);
CREATE INDEX IF NOT EXISTS idx_trades_mint_time     ON trades(mint_address, block_time DESC);

-- -------------------------------------------------------------------------
-- raw_transactions  (Helius transaction result, never mutated — replay source)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS raw_transactions (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    signature       TEXT        UNIQUE NOT NULL,
    slot            BIGINT      NOT NULL,
    block_time      TIMESTAMPTZ NOT NULL,
    raw_data        JSONB       NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_raw_tx_signature ON raw_transactions(signature);
CREATE INDEX IF NOT EXISTS idx_raw_tx_slot      ON raw_transactions(slot DESC);

-- -------------------------------------------------------------------------
-- tokens_info
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tokens_info (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address    TEXT        UNIQUE NOT NULL,
    ath_price       DOUBLE PRECISION,
    ath_timestamp   TIMESTAMPTZ,
    age             BIGINT,
    volume          DOUBLE PRECISION DEFAULT 0.0,
    market_cap      DOUBLE PRECISION,
    trade_count     BIGINT      NOT NULL DEFAULT 0,
    last_trade_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tokens_info_mint ON tokens_info(mint_address);

-- -------------------------------------------------------------------------
-- tokens_analysis
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tokens_analysis (
    id              UUID                PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address    TEXT                NOT NULL,
    analyzer_name   TEXT                NOT NULL,
    score           DOUBLE PRECISION    NOT NULL,
    indicators      JSONB               NOT NULL DEFAULT '[]',
    computed_at     TIMESTAMPTZ         NOT NULL,
    UNIQUE (mint_address, analyzer_name)
);

CREATE INDEX IF NOT EXISTS idx_analysis_mint ON tokens_analysis(mint_address);

-- -------------------------------------------------------------------------
-- creator_profiles
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS creator_profiles (
    id                  UUID                PRIMARY KEY DEFAULT uuid_generate_v4(),
    wallet_address      TEXT                UNIQUE NOT NULL,
    tokens_created      INTEGER             NOT NULL DEFAULT 0,
    total_volume_sol    DOUBLE PRECISION    NOT NULL DEFAULT 0.0,
    suspiciousness_score DOUBLE PRECISION   NOT NULL DEFAULT 0.0,
    wash_trade_score    DOUBLE PRECISION    NOT NULL DEFAULT 0.0,
    last_analyzed_at    TIMESTAMPTZ,
    indicators          JSONB               NOT NULL DEFAULT '{}'
);
