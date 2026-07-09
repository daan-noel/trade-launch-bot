-- ============================================================================
-- Consolidated core schema (single-file init). TimescaleDB, local-first.
--
-- This is the squash of the entire core migration chain into one end-state file —
-- it creates the schema exactly as running the full chain on a fresh database
-- would leave it (all renames/adds/drops already folded in). It absorbs the
-- former on-disk files:
--   * 0001_init  — itself the squash of the legacy 0001..0010 chain:
--       - 0001 init                    base tables, hypertables, views, seeds
--       - 0002 token_account           strategy_positions.token_account
--       - 0003 venue-neutral reserves  reserve_sol/reserve_token (already inlined)
--       - 0004 creation_slot/first-slot tokens.creation_slot + first-slot lamports
--       - 0005 token-list pagination   tokens/tokens_info sort indexes
--       - 0007 drop cohort metric      strategy_run_metrics.n_exit_cohort removed
--       - 0009 SOL/lamports naming      *_sol/*_lamports unit-in-name renames
--   * 0002_strategy_positions_mint_address — strategy_positions.mint -> mint_address
--       (folded: the column + its index below already use the mint_address name).
--   * 0003_bucket_width_sol — data-only (rewrote strategy_rules/strategy_runs
--       params tolerance_pct -> bucket_width_sol); a no-op on a fresh DB, so
--       intentionally omitted here.
-- The pure data-backfill migrations (0006 snake_case buy-ix keys, 0008 paper
-- entry backfill, 0010 tpsl2 param cleanup, and 0009's JSONB key rewrite) only
-- rewrote pre-existing rows — no-ops on a fresh DB — so they are intentionally
-- omitted here.
--
-- Naming rule (migration 0009, locked): every column denoting a SOL amount names
-- its unit — `_lamports` = exact BIGINT, `_sol` = human f64. Ratios keep
-- `_price`/`_pct`.
--
-- TimescaleDB note: hypertable creation + compression/retention policies are
-- transaction-safe and live here. Continuous aggregates (trades_ohlcv_*) CANNOT
-- be created inside a transaction, and sqlx 0.6 wraps every migration in one — so
-- the CAggs are created idempotently at boot by `storage::timescale::setup_caggs`.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ===========================================================================
-- Wallet interning dictionary (trades-plan lever 2)
--   trades carry a 4-byte wallet_id instead of a 44-byte base58 address. This
--   dict is the id<->address map; it is a *soft* reference from trades (no FK on
--   the hot insert path — performance budget). Distinct from the wallet
--   *directory* (wallet_profiles/wallets) which is a separate UI feature.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS wallet_dict (
    id      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    address TEXT    NOT NULL UNIQUE
);

-- ===========================================================================
-- tokens — static creation facts (write-once), natural key = mint_address
-- ===========================================================================
CREATE TABLE IF NOT EXISTS tokens (
    mint_address            TEXT        PRIMARY KEY,
    creator_wallet          TEXT        NOT NULL,

    name                    TEXT        NOT NULL,
    symbol                  TEXT        NOT NULL,
    bonding_curve_address   TEXT,
    token_program_id        TEXT,

    initial_supply_token    BIGINT,
    initial_buy_lamports    BIGINT,                 -- lamports (exact; display ÷1e9)

    cu_limit                BIGINT,
    cu_price                BIGINT,

    is_mayhem_mode          BOOLEAN     NOT NULL DEFAULT FALSE,
    is_cashback_enabled     BOOLEAN     NOT NULL DEFAULT FALSE,

    creation_slot           BIGINT,                 -- Solana slot of the create tx
    creation_tx_signature   TEXT        NOT NULL,
    ix_labels               JSONB       NOT NULL DEFAULT '[]',
    initial_buy_instruction JSONB,

    meta                    JSONB       NOT NULL DEFAULT '{}',
    created_at              TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet      ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at          ON tokens(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_token_program_id    ON tokens(token_program_id);
CREATE INDEX IF NOT EXISTS idx_tokens_is_mayhem_mode      ON tokens(is_mayhem_mode);
CREATE INDEX IF NOT EXISTS idx_tokens_is_cashback_enabled ON tokens(is_cashback_enabled);
-- Default token-list order + stable tiebreak (newest-first, mint tiebreak).
CREATE INDEX IF NOT EXISTS idx_tokens_created_mint
    ON tokens (created_at DESC, mint_address DESC);

-- ===========================================================================
-- tokens_info — live market metrics (hot-updated), PK = FK = mint_address (1:1)
-- ===========================================================================
CREATE TABLE IF NOT EXISTS tokens_info (
    mint_address   TEXT        PRIMARY KEY REFERENCES tokens(mint_address) ON DELETE CASCADE,

    current_price  DOUBLE PRECISION,
    ath_price      DOUBLE PRECISION,
    ath_timestamp  TIMESTAMPTZ,
    volume_sol     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    trade_count    BIGINT      NOT NULL DEFAULT 0,
    last_trade_at  TIMESTAMPTZ,

    -- total buy/sell SOL (lamports) across trades in the token's creation slot.
    first_slot_buy_lamports  BIGINT,
    first_slot_sell_lamports BIGINT,

    is_dead        BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated    BOOLEAN     NOT NULL DEFAULT FALSE,
    lifetime_secs  BIGINT,

    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tokens_info_is_dead       ON tokens_info(is_dead);
CREATE INDEX IF NOT EXISTS idx_tokens_info_volume_sol    ON tokens_info(volume_sol DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_info_last_trade_at ON tokens_info(last_trade_at DESC);
-- Sort columns the token table exposes (paged straight from Postgres). NULLS LAST
-- matches the query's `... NULLS LAST` so the index order serves the sort directly.
CREATE INDEX IF NOT EXISTS idx_tokens_info_trade_count
    ON tokens_info (trade_count DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_current_price
    ON tokens_info (current_price DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_price
    ON tokens_info (ath_price DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_timestamp
    ON tokens_info (ath_timestamp DESC NULLS LAST);

-- ===========================================================================
-- token_sync_state — per-venue ingest watermarks (replaces last_synced_* cols)
--   One row per (mint, venue); a new venue is a new row, not new columns.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS token_sync_state (
    mint_address   TEXT        NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    venue          TEXT        NOT NULL CHECK (venue IN ('curve', 'amm')),
    last_sig       TEXT,
    last_slot      BIGINT,
    last_synced_at TIMESTAMPTZ,
    PRIMARY KEY (mint_address, venue)
);

CREATE INDEX IF NOT EXISTS idx_token_sync_state_synced ON token_sync_state(last_synced_at);

-- Full token picture; age / market_cap derived (never stored). LEFT JOIN so
-- freshly-created (pre-sync) tokens still appear with NULL metrics.
-- DROP+CREATE (not CREATE OR REPLACE): the latter can only append output columns,
-- not reorder/rename them, so it breaks re-runs against an older view shape.
DROP VIEW IF EXISTS token_overview;
CREATE VIEW token_overview AS
SELECT
    t.mint_address,
    t.creator_wallet,
    t.name,
    t.symbol,
    t.bonding_curve_address,
    t.token_program_id,
    t.initial_supply_token,
    t.initial_buy_lamports,
    t.cu_limit,
    t.cu_price,
    t.is_mayhem_mode,
    t.is_cashback_enabled,
    t.creation_slot,
    t.creation_tx_signature,
    t.ix_labels,
    t.initial_buy_instruction,
    t.meta,
    t.created_at,
    i.current_price,
    i.ath_price,
    i.ath_timestamp,
    i.volume_sol,
    i.trade_count,
    i.last_trade_at,
    i.first_slot_buy_lamports,
    i.first_slot_sell_lamports,
    i.is_dead,
    i.is_migrated,
    i.lifetime_secs,
    i.updated_at AS metrics_updated_at,
    EXTRACT(EPOCH FROM (now() - t.created_at))::bigint AS age_secs,
    (i.current_price * t.initial_supply_token)         AS market_cap
FROM tokens t
LEFT JOIN tokens_info i USING (mint_address);

-- ===========================================================================
-- raw_txs — source-of-truth feed (full unparsed payload). BYTEA, hypertable,
--   shortest retention in the system. trades is a typed projection of this.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS raw_txs (
    tx_signature  BYTEA       NOT NULL,
    slot          BIGINT      NOT NULL,
    block_time    TIMESTAMPTZ NOT NULL,
    tx_index      INTEGER     NOT NULL,
    payload       BYTEA       NOT NULL,
    source        SMALLINT    NOT NULL DEFAULT 0,   -- 0=live 1=sync
    PRIMARY KEY (block_time, tx_signature)          -- partition col first; IS the dedup key
);

SELECT create_hypertable('raw_txs', by_range('block_time', INTERVAL '1 day'), if_not_exists => TRUE);

ALTER TABLE raw_txs SET (
    timescaledb.compress,
    timescaledb.compress_orderby = 'slot, tx_index'
);
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '2 days', if_not_exists => TRUE);
SELECT add_retention_policy('raw_txs', drop_after => INTERVAL '7 days', if_not_exists => TRUE);

-- ===========================================================================
-- trades — high-volume append-only feed (the LaserStream transport IS this
--   table). Integer base units, BYTEA signature, interned wallet_id, no
--   surrogate key (the dedup key is the PK). Reserves stored as a single
--   venue-neutral pair (reserve_lamports/reserve_token): curve virtual reserves
--   on curve rows, pool real reserves on amm rows. No separate real_*_reserves.
--
--   Ordering key (execution order): (slot, tx_index, leg_index).
--   block_time: wall-clock partition + candle-bucket axis (NOT an order key).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS trades (
    mint_address           TEXT        NOT NULL,
    wallet_id              INTEGER     NOT NULL,   -- soft ref -> wallet_dict(id)
    trade_type             TEXT        NOT NULL CHECK (trade_type IN ('buy','sell')),
    venue                  TEXT        NOT NULL DEFAULT 'curve'
                                            CHECK (venue IN ('curve','amm')),

    -- amounts as integer base units (exact, matches chain u64)
    amount_lamports        BIGINT      NOT NULL,   -- lamports
    token_amount           BIGINT      NOT NULL,   -- raw token units
    -- Reserve pair this row prices from (venue-neutral): curve virtual reserves
    -- on curve rows, PumpSwap pool real reserves on amm rows. spot = sol/token.
    reserve_lamports       BIGINT,
    reserve_token          BIGINT,

    -- ordering key; block_time = bucket/partition axis
    slot                   BIGINT      NOT NULL,
    tx_index               INTEGER     NOT NULL,
    leg_index              SMALLINT    NOT NULL DEFAULT 0,
    block_time             TIMESTAMPTZ NOT NULL,

    tx_signature           BYTEA       NOT NULL,   -- raw 64-byte sig

    PRIMARY KEY (block_time, tx_signature, leg_index)  -- partition col first; IS the dedup key
);

SELECT create_hypertable('trades', by_range('block_time', INTERVAL '1 day'), if_not_exists => TRUE);

-- Per-mint chronological reads with exact intra-block order (recent chunks).
CREATE INDEX IF NOT EXISTS idx_trades_mint_order
    ON trades(mint_address, slot, tx_index, leg_index);

ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'mint_address',
    timescaledb.compress_orderby   = 'slot, tx_index, leg_index'
);
-- compress_after MUST exceed the sync/backfill lookback horizon.
SELECT add_compression_policy('trades', compress_after => INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('trades', drop_after => INTERVAL '30 days', if_not_exists => TRUE);

-- Read view — derived price (SOL per raw token unit; ÷1e9).
DROP VIEW IF EXISTS trades_priced;
CREATE VIEW trades_priced AS
SELECT
    t.*,
    (t.amount_lamports::double precision / 1e9 / NULLIF(t.token_amount, 0)) AS price_per_token
FROM trades t;

-- ===========================================================================
-- strategy_rules — shared, keyed by strategy_id; typed lifecycle + JSONB brain.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_rules (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id             TEXT        NOT NULL,          -- 'tpsl1' | 'tpsl2' | …
    rule_name               TEXT        NOT NULL,

    buy_amount_sol          DOUBLE PRECISION NOT NULL,
    trade_mode              TEXT        NOT NULL DEFAULT 'paper'
                                CHECK (trade_mode IN ('paper', 'real')),
    is_active               BOOLEAN     NOT NULL DEFAULT TRUE,
    max_concurrent_tokens   BIGINT,
    max_total_tokens        BIGINT,

    -- token fingerprint + entry gates + exit gates + tolerance (per-strategy).
    params                  JSONB       NOT NULL DEFAULT '{}',

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_rules_strategy ON strategy_rules(strategy_id);
CREATE INDEX IF NOT EXISTS idx_strategy_rules_active   ON strategy_rules(strategy_id, is_active);

-- ===========================================================================
-- strategy_runs — one activation session of a rule (paper or real).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    rule_id          UUID        REFERENCES strategy_rules(id) ON DELETE SET NULL,
    mode             TEXT        NOT NULL CHECK (mode IN ('real', 'paper')),
    run_seq          BIGINT      NOT NULL,                 -- monotonic per (rule, mode)
    status           TEXT        NOT NULL DEFAULT 'Running'
                         CHECK (status IN ('Running', 'Finished', 'Stopped', 'Cancelled')),

    params_snapshot  JSONB       NOT NULL,                 -- frozen rule at activation
    max_total_tokens BIGINT,

    started_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at      TIMESTAMPTZ,

    UNIQUE (rule_id, mode, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_strategy_runs_rule
    ON strategy_runs(rule_id, mode, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_runs_strategy
    ON strategy_runs(strategy_id, mode, status, started_at DESC);

-- ===========================================================================
-- strategy_run_metrics — finalize-time rollup, 1:1 with strategy_runs.
--   Same metric set as the param-sweep results (live/paper/sweep comparable).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_run_metrics (
    run_id              UUID        PRIMARY KEY REFERENCES strategy_runs(id) ON DELETE CASCADE,
    rolled_up_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    n_fired             INTEGER     NOT NULL,
    n_open              INTEGER     NOT NULL,
    n_closed            INTEGER     NOT NULL,

    win_rate            REAL        NOT NULL,
    total_pnl_sol       REAL        NOT NULL,
    expectancy_sol      REAL        NOT NULL,
    mean_pnl_pct        REAL        NOT NULL,
    median_pnl_pct      REAL        NOT NULL,
    p90_pnl_pct         REAL        NOT NULL,
    best_pnl_pct        REAL        NOT NULL,
    worst_pnl_pct       REAL        NOT NULL,
    std_pnl_pct         REAL        NOT NULL DEFAULT 0,
    profit_factor       REAL,                              -- NULL = no losing trades (∞)
    avg_holding_secs    REAL        NOT NULL,
    median_holding_secs REAL        NOT NULL,

    n_exit_take_profit  INTEGER     NOT NULL,
    n_exit_stop_loss    INTEGER     NOT NULL,
    n_exit_trailing     INTEGER     NOT NULL,
    n_exit_stall        INTEGER     NOT NULL,
    n_exit_time         INTEGER     NOT NULL,
    n_exit_liquidity    INTEGER     NOT NULL,
    n_exit_open         INTEGER     NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_strategy_run_metrics_pnl ON strategy_run_metrics(total_pnl_sol DESC);

-- ===========================================================================
-- strategy_positions — one position the bot opened within a run (real | paper).
--   Volume is bounded (one row per BOT trade) → no partitioning.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id                  UUID        NOT NULL REFERENCES strategy_runs(id) ON DELETE CASCADE,
    strategy_id             TEXT        NOT NULL,
    rule_id                 UUID,
    mode                    TEXT        NOT NULL CHECK (mode IN ('real', 'paper')),

    mint_address            TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,
    -- persisted wallet token account for this mint, so bot buys/sells reuse ONE
    -- account across restarts (nullable + additive; NULL falls back to resolver).
    token_account           TEXT,

    -- optional trigger trade (scalp-style entry arming)
    -- price = SOL per raw token unit (ratio, float); amounts = exact integers:
    -- *_token_amount = raw token units (BIGINT), *_lamports = lamports (BIGINT).
    target_price            DOUBLE PRECISION,
    target_token_amount     BIGINT,                 -- raw token units
    target_time             TIMESTAMPTZ,
    target_tx               TEXT,

    -- entry fill (NULL until the buy lands)
    entry_price             DOUBLE PRECISION,
    entry_token_amount      BIGINT,                 -- raw token units
    entry_lamports          BIGINT,                 -- lamports
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',

    -- exit fill
    exit_price              DOUBLE PRECISION,
    exit_token_amount       BIGINT,                 -- raw token units
    exit_lamports           BIGINT,                 -- lamports
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures      JSONB       NOT NULL DEFAULT '[]',

    submitted_buy_signatures TEXT[]     NOT NULL DEFAULT '{}',
    status                  TEXT        NOT NULL
                                CHECK (status IN ('Arming','BuySubmitted','Holding',
                                                  'ExitPending','End','ExitFailed')),
    exit_reason             TEXT,
    extra                   JSONB       NOT NULL DEFAULT '{}',

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_positions_run            ON strategy_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_strategy_created
    ON strategy_positions(strategy_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_rule_created   ON strategy_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_mint_address_status ON strategy_positions(mint_address, status);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_token_program  ON strategy_positions(token_program_id);

-- In-flight recovery sweep (real mode).
CREATE INDEX IF NOT EXISTS idx_strategy_positions_buy_submitted
    ON strategy_positions(updated_at) WHERE status = 'BuySubmitted';

-- Double-sell / double-buy safety: unique on the first tx leg, REAL MODE ONLY.
CREATE UNIQUE INDEX IF NOT EXISTS uq_strategy_positions_entry_sig0
    ON strategy_positions ((entry_tx_signatures->>0))
    WHERE mode = 'real' AND jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_strategy_positions_exit_sig0
    ON strategy_positions ((exit_tx_signatures->>0))
    WHERE mode = 'real' AND jsonb_array_length(exit_tx_signatures) > 0;

-- Derived per-position PnL (never stored).
DROP VIEW IF EXISTS strategy_position_pnl;
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    -- exit_lamports/entry_lamports are lamports (BIGINT); divide back to human SOL
    -- so realized_pnl_sol matches StrategyPosition::realized_pnl_sol() (f64 SOL).
    ((p.exit_lamports - p.entry_lamports)::float8 / 1e9)               AS realized_pnl_sol,
    CASE WHEN p.entry_price > 0
         THEN (p.exit_price - p.entry_price) / p.entry_price * 100.0 END AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END     AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                     AS is_closed
FROM strategy_positions p;

-- ===========================================================================
-- Wallet directory + global settings (UI features, carried over unchanged)
-- ===========================================================================

CREATE TABLE IF NOT EXISTS wallet_profile_tags (
    id         UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name       TEXT        NOT NULL UNIQUE,
    color      TEXT        NOT NULL DEFAULT '#6366f1',
    comment    TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO wallet_profile_tags (name, color, comment) VALUES
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
    ('High Risk',       '#ef4444', 'Frequently trades micro-cap or low-liquidity tokens'),
    ('Low Risk',        '#84cc16', 'Prefers established tokens with deeper liquidity'),
    ('Leverage',        '#dc2626', 'Uses leveraged positions or perpetuals'),
    ('Copy Trader',     '#8b5cf6', 'Mirrors trades of known alpha wallets'),
    ('Insider',         '#f97316', 'Suspicious early buys before announcements'),
    ('Dev Wallet',      '#ec4899', 'Linked to a token team or deployer address'),
    ('MEV',             '#14b8a6', 'Sandwich attacks, frontrunning, or arbitrage activity'),
    ('Wash Trader',     '#78716c', 'Suspected artificial volume between related wallets'),
    ('Accumulator',     '#3b82f6', 'Builds positions gradually over time'),
    ('Distributor',     '#f87171', 'Consistently dumps into strength'),
    ('KOL',             '#fbbf24', 'Key Opinion Leader — influencer or CT personality'),
    ('Watchlist',       '#60a5fa', 'Under active monitoring for copy or alpha signals'),
    ('Blacklist',       '#1e293b', 'Known bad actor, scammer, or rug puller')
ON CONFLICT (name) DO NOTHING;

CREATE TABLE IF NOT EXISTS wallet_profiles (
    id          UUID    PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        TEXT    NOT NULL,
    type        TEXT    NOT NULL CHECK (type IN ('mine', 'trader', 'whale', 'dev')),
    tag_ids     UUID[]  NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS wallets (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    profile_id   UUID        NOT NULL REFERENCES wallet_profiles(id) ON DELETE CASCADE,
    address      TEXT        UNIQUE NOT NULL,
    is_tracked   BOOLEAN     NOT NULL DEFAULT TRUE,
    comment      TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wallets_profile_id  ON wallets(profile_id);
CREATE INDEX IF NOT EXISTS idx_wallets_address     ON wallets(address);
CREATE INDEX IF NOT EXISTS idx_wallets_is_tracked  ON wallets(is_tracked);

-- Typed key-value store. Key set + defaults owned by `settings_repo`'s
-- `AppSettings`; absent keys fall back to per-setting defaults at load time.
CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
