
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
    is_mayhem_mode          BOOLEAN     NOT NULL DEFAULT FALSE,
    initial_buy_instruction JSONB,
    created_at              TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at     ON tokens(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_is_mayhem_mode ON tokens(is_mayhem_mode);

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
    current_price   DOUBLE PRECISION,
    is_rugged       BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated     BOOLEAN     NOT NULL DEFAULT FALSE,
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

CREATE INDEX IF NOT EXISTS idx_creator_profiles_wallet ON creator_profiles(wallet_address);

-- -------------------------------------------------------------------------
-- strategy_TPSL_rules
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS strategy_TPSL_rules (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name           TEXT        NOT NULL,
    p_initial_buy_sol   DOUBLE PRECISION,
    p_cu_limit          BIGINT,
    p_cu_price          BIGINT,
    p_max_sol_cost      DOUBLE PRECISION,
    p_spendable_sol_in  DOUBLE PRECISION,
    p_ix_labels         JSONB       NOT NULL DEFAULT '[]',
    buy_amount          DOUBLE PRECISION NOT NULL,
    take_profit         DOUBLE PRECISION NOT NULL,
    stop_loss           DOUBLE PRECISION NOT NULL,
    tolerance_pct       DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    is_active           BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_TPSL_rules_active ON strategy_TPSL_rules(is_active);

-- -------------------------------------------------------------------------
-- positions
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    entry_price         DOUBLE PRECISION NOT NULL,
    exit_price          DOUBLE PRECISION,
    entry_tx            TEXT        NOT NULL UNIQUE,
    exit_tx             TEXT        UNIQUE,
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'End')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    entry_amount        DOUBLE PRECISION NOT NULL,
    exit_amount         DOUBLE PRECISION,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    
    FOREIGN KEY (rule_id) REFERENCES strategy_TPSL_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_positions_mint ON positions(mint);
CREATE INDEX IF NOT EXISTS idx_positions_wallet ON positions(wallet);
CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status);
CREATE INDEX IF NOT EXISTS idx_positions_strategy ON positions(strategy);
CREATE INDEX IF NOT EXISTS idx_positions_rule_id ON positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_positions_mint_status ON positions(mint, status);
