-- ============================================================================
-- Initial schema (squashed).
--
-- This single migration represents the full schema after the original
-- 0001–0014 migrations were merged. Tables are ordered so foreign-key targets
-- are created before their referrers.
-- ============================================================================

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
    is_cashback_enabled     BOOLEAN     NOT NULL DEFAULT FALSE,
    token_program_id        TEXT,
    initial_buy_instruction JSONB,
    created_at              TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet     ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at         ON tokens(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_is_mayhem_mode     ON tokens(is_mayhem_mode);
CREATE INDEX IF NOT EXISTS idx_tokens_token_program_id   ON tokens(token_program_id);
CREATE INDEX IF NOT EXISTS idx_tokens_is_cashback_enabled ON tokens(is_cashback_enabled);

-- -------------------------------------------------------------------------
-- trades
--   leg_index            — multiple pump trades in the same tx (multi-leg).
--   received_at          — ingest-time precision (Utc::now) for UI ordering;
--                          block_time stays chain seconds.
--   venue                — bonding-curve vs post-migration PumpSwap (AMM).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS trades (
    id                      UUID                PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address            TEXT                NOT NULL,
    wallet_address          TEXT                NOT NULL,
    trade_type              TEXT                NOT NULL CHECK (trade_type IN ('buy', 'sell')),
    sol_amount              DOUBLE PRECISION    NOT NULL,
    token_amount            DOUBLE PRECISION    NOT NULL,
    price_per_token         DOUBLE PRECISION    NOT NULL,
    tx_signature            TEXT                NOT NULL,
    slot                    BIGINT              NOT NULL,
    block_time              TIMESTAMPTZ         NOT NULL,
    virtual_sol_reserves    DOUBLE PRECISION,
    virtual_token_reserves  DOUBLE PRECISION,
    real_sol_reserves       DOUBLE PRECISION,
    real_token_reserves     DOUBLE PRECISION,
    ix_type                 TEXT                NOT NULL DEFAULT 'Unknown',
    ix_labels               JSONB               NOT NULL DEFAULT '[]',
    leg_index               INTEGER             NOT NULL DEFAULT 0,
    received_at             TIMESTAMPTZ         NOT NULL,
    venue                   TEXT                NOT NULL DEFAULT 'curve'
                                CHECK (venue IN ('curve', 'amm'))
);

CREATE INDEX IF NOT EXISTS idx_trades_mint          ON trades(mint_address);
CREATE INDEX IF NOT EXISTS idx_trades_wallet        ON trades(wallet_address);
CREATE INDEX IF NOT EXISTS idx_trades_block_time    ON trades(block_time DESC);
CREATE INDEX IF NOT EXISTS idx_trades_mint_time     ON trades(mint_address, block_time DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_tx_leg ON trades(tx_signature, leg_index);
-- Supports the incremental "Fetch new" boundary lookup: latest signature per
-- (mint, venue) so each source resumes from its own last saved trade.
CREATE INDEX IF NOT EXISTS idx_trades_mint_venue_slot ON trades(mint_address, venue, slot DESC);

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
--   last_synced_at        — wall-clock time of the last successful sync (display).
--   last_synced_curve_sig — newest bonding-curve signature seen, used as the
--                           `until` boundary for the next incremental sync.
--   last_synced_amm_sig   — same, for post-migration PumpSwap (AMM) signatures.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tokens_info (
    id                    UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address          TEXT        UNIQUE NOT NULL,
    ath_price             DOUBLE PRECISION,
    ath_timestamp         TIMESTAMPTZ,
    age                   BIGINT,
    volume                DOUBLE PRECISION DEFAULT 0.0,
    market_cap            DOUBLE PRECISION,
    trade_count           BIGINT      NOT NULL DEFAULT 0,
    last_trade_at         TIMESTAMPTZ,
    current_price         DOUBLE PRECISION,
    is_rugged             BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated           BOOLEAN     NOT NULL DEFAULT FALSE,
    last_synced_at        TIMESTAMPTZ,
    last_synced_curve_sig TEXT,
    last_synced_amm_sig   TEXT,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
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
--   p_trailing_stop_pct  — exit when price falls X% below peak-since-entry.
--   p_time_stop_secs     — exit at first trade >= N secs after entry.
--   p_stall_secs         — exit when no new higher-high for N secs.
--   p_liquidity_drop_pct — exit once virtual SOL reserves crash X% below peak.
-- All four are inert by default (0 / NULL = disabled, per the ignore_zero_*
-- convention in the tpsl strategy).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS strategy_TPSL_rules (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name               TEXT        NOT NULL,
    p_initial_buy_sol       DOUBLE PRECISION,
    p_cu_limit              BIGINT,
    p_cu_price              BIGINT,
    p_max_sol_cost          DOUBLE PRECISION,
    p_spendable_sol_in      DOUBLE PRECISION,
    p_max_concurrent_tokens BIGINT,
    p_max_total_tokens      BIGINT,
    p_ix_labels             JSONB       NOT NULL DEFAULT '[]',
    p_trailing_stop_pct     DOUBLE PRECISION,
    p_time_stop_secs        BIGINT,
    p_stall_secs            BIGINT,
    p_liquidity_drop_pct    DOUBLE PRECISION,
    buy_amount              DOUBLE PRECISION NOT NULL,
    take_profit             DOUBLE PRECISION NOT NULL,
    stop_loss               DOUBLE PRECISION NOT NULL,
    tolerance_pct           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    trade_mode              TEXT        NOT NULL DEFAULT 'paper',
    is_active               BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_TPSL_rules_active ON strategy_TPSL_rules(is_active);

-- -------------------------------------------------------------------------
-- positions
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    token_program_id    TEXT,
    entry_price         DOUBLE PRECISION NOT NULL,
    entry_amount        DOUBLE PRECISION NOT NULL,
    entry_time          TIMESTAMPTZ,
    entry_tx            TEXT        NOT NULL UNIQUE,
    exit_price          DOUBLE PRECISION,
    exit_amount         DOUBLE PRECISION,
    exit_time           TIMESTAMPTZ,
    exit_tx             TEXT        UNIQUE,
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (rule_id) REFERENCES strategy_TPSL_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_positions_mint              ON positions(mint);
CREATE INDEX IF NOT EXISTS idx_positions_wallet            ON positions(wallet);
CREATE INDEX IF NOT EXISTS idx_positions_status            ON positions(status);
CREATE INDEX IF NOT EXISTS idx_positions_strategy          ON positions(strategy);
CREATE INDEX IF NOT EXISTS idx_positions_rule_id           ON positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_positions_mint_status       ON positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_positions_token_program_id  ON positions(token_program_id);

-- -------------------------------------------------------------------------
-- wallet_profiles
--   tag_ids — references wallet_profile_tags(id) (soft array reference).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wallet_profiles (
    id          UUID    PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        TEXT    NOT NULL,
    type        TEXT    NOT NULL CHECK (type IN ('mine', 'trader', 'whale', 'dev')),
    tag_ids     UUID[]  NOT NULL DEFAULT '{}',
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

-- -------------------------------------------------------------------------
-- wallet_profile_tags  (global tag library)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS wallet_profile_tags (
    id         UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name       TEXT        NOT NULL UNIQUE,
    color      TEXT        NOT NULL DEFAULT '#6366f1',
    comment    TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed default tags for wallet profile classification
INSERT INTO wallet_profile_tags (name, color, comment) VALUES
    -- Performance labels
    ('Whale',           '#6366f1', 'Large position sizes, significant market impact'),
    ('Degen',           '#f43f5e', 'High-risk, high-frequency speculative trades'),
    ('Diamond Hands',   '#22d3ee', 'Holds through volatility, rarely exits early'),
    ('Paper Hands',     '#94a3b8', 'Exits positions quickly on any downturn'),
    ('Flipper',         '#f59e0b', 'Fast in-and-out, targets quick profits'),
    ('Sniper',          '#10b981', 'Enters very early, often at launch or listing'),
    ('Ape',             '#fb923c', 'Buys into hype with little due diligence'),
    ('Smart Money',     '#a78bfa', 'Consistently early on winners, likely informed'),
    ('Bot',             '#64748b', 'Automated trading patterns, likely scripted'),
    ('Bundler',         '#0ea5e9', 'Groups buys to obscure wallet identity'),

    -- Risk profile
    ('High Risk',       '#ef4444', 'Frequently trades micro-cap or low-liquidity tokens'),
    ('Low Risk',        '#84cc16', 'Prefers established tokens with deeper liquidity'),
    ('Leverage',        '#dc2626', 'Uses leveraged positions or perpetuals'),

    -- Behavior
    ('Copy Trader',     '#8b5cf6', 'Mirrors trades of known alpha wallets'),
    ('Insider',         '#f97316', 'Suspicious early buys before announcements'),
    ('Dev Wallet',      '#ec4899', 'Linked to a token team or deployer address'),
    ('MEV',             '#14b8a6', 'Sandwich attacks, frontrunning, or arbitrage activity'),
    ('Wash Trader',     '#78716c', 'Suspected artificial volume between related wallets'),
    ('Accumulator',     '#3b82f6', 'Builds positions gradually over time'),
    ('Distributor',     '#f87171', 'Consistently dumps into strength'),

    -- Community / social
    ('KOL',             '#fbbf24', 'Key Opinion Leader — influencer or CT personality'),
    ('Watchlist',       '#60a5fa', 'Under active monitoring for copy or alpha signals'),
    ('Blacklist',       '#1e293b', 'Known bad actor, scammer, or rug puller')

ON CONFLICT (name) DO NOTHING;

-- -------------------------------------------------------------------------
-- app_settings
-- Global, server-wide settings store. A single-row table (enforced by the `id`
-- + CHECK) holding ALL app settings as one JSONB document. The document's shape
-- is owned by the application's `AppSettings` struct, not by this schema:
-- adding a setting is a struct field with a default, never a migration.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS app_settings (
    id         INTEGER PRIMARY KEY DEFAULT 1,
    data       JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT app_settings_singleton CHECK (id = 1)
);

INSERT INTO app_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;

-- -------------------------------------------------------------------------
-- paper_test_runs
-- Paper-trading runs, isolated from the real `positions` table. A paper "run"
-- begins when a paper rule is activated and ends when its total-token cap is
-- reached (rule auto-deactivates) or it is manually stopped. Only the latest
-- run per rule is retained (starting a new run deletes the prior run, and its
-- positions go with it via ON DELETE CASCADE).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS paper_test_runs (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_id            UUID        NOT NULL REFERENCES strategy_TPSL_rules(id) ON DELETE CASCADE,
    -- Monotonic per-rule run counter (1, 2, 3 …) for display ("run #N").
    run_seq            BIGINT      NOT NULL,
    status             TEXT        NOT NULL CHECK (status IN ('Running', 'Finished', 'Stopped')),
    -- Snapshot of the rule's total-token cap at run start (NULL = unlimited).
    max_total_tokens   BIGINT,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    UNIQUE (rule_id, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_paper_test_runs_rule ON paper_test_runs(rule_id);

-- -------------------------------------------------------------------------
-- paper_positions
-- Mirrors `positions` (minus the UNIQUE constraints on entry_tx / exit_tx — a
-- paper position is seeded with the token's creation tx and the same token may
-- be traded again in a later run) plus a run_id binding each position to its run.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES paper_test_runs(id) ON DELETE CASCADE,
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    token_program_id    TEXT,
    entry_price         DOUBLE PRECISION NOT NULL,
    entry_amount        DOUBLE PRECISION NOT NULL,
    entry_time          TIMESTAMPTZ,
    entry_tx            TEXT        NOT NULL,
    exit_price          DOUBLE PRECISION,
    exit_amount         DOUBLE PRECISION,
    exit_time           TIMESTAMPTZ,
    exit_tx             TEXT,
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_paper_positions_run         ON paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_paper_positions_mint_status ON paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_paper_positions_rule        ON paper_positions(rule_id);
