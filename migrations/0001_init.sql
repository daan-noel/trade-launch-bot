-- ============================================================================
-- 0001_init — data-infrastructure foundation (Domains A–C).
--
-- Venue- and quote-asset-generalized schema. Carries meme-trading's proven data
-- ideas (raw feed + typed projection, integer base units, identity SSOT,
-- interning, hypertables + declarative retention/compression, JSONB "brain",
-- derived-never-stored views) but lifts its two hard-coded assumptions into
-- first-class dimensions:
--   * quote asset  — a `quote_assets` row; native SOL is just is_native w/ 9
--     decimals. Amounts are `amount_quote` / `amount_base` base units (the unit
--     is the referenced asset, NOT a hard-coded lamport).
--   * venue        — split into `launchpad_id` (which platform) + `market_kind`
--     (bonding_curve | amm | clmm | orderbook). A token moves across markets.
--
-- Naming rule (locked): a column denoting an amount names its unit as a SUFFIX —
-- `amount_quote` / `amount_base` (exact BIGINT base units, display ÷10^decimals).
-- Reserves follow suit: `reserve_quote` / `reserve_base`. Ratios keep `_price`.
--
-- Price convention (locked, documented once): every STORED / AGGREGATED price is
-- a RAW RATIO in "quote base units per base base unit" (p = amount_quote /
-- amount_base) — decimals-agnostic; the assets' decimals live in the dimensions
-- and are applied ONLY in derived views (display + USD). This keeps one price
-- convention end-to-end (no baked-in 1e9) and makes cross-quote USD comparison a
-- view join, never a schema change.
--
-- TimescaleDB note: hypertable creation + compression/retention policies are
-- transaction-safe and live here. Continuous aggregates CANNOT be created inside
-- a transaction (sqlx wraps every migration in one) — so the OHLCV CAggs are
-- created idempotently at boot by `platform_core::storage::timescale::setup_caggs`.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ===========================================================================
-- Domain A — Dimensions (small, slow-changing, interned onto hot rows)
-- ===========================================================================

-- Quote assets: SOL, USDC, …  Native SOL is the row whose base unit is the
-- lamport (is_native, decimals 9). usd_rate is the cross-quote numeraire
-- (poller-updated for SOL; USDC pinned ≈ 1) so a SOL-quoted token and a
-- USDC-quoted one are USD-comparable in views.
CREATE TABLE IF NOT EXISTS quote_assets (
    id           SMALLINT     PRIMARY KEY,          -- interned, stamped on hot rows
    mint         TEXT         NOT NULL UNIQUE,       -- WSOL mint for SOL
    symbol       TEXT         NOT NULL,
    decimals     SMALLINT     NOT NULL,             -- 9 SOL, 6 USDC
    is_native    BOOLEAN      NOT NULL,             -- true → lamports + wrap/unwrap path
    usd_rate     DOUBLE PRECISION,                  -- USD per 1 display unit; USDC ≈ 1
    usd_rate_at  TIMESTAMPTZ
);

-- Launchpads: pump.fun, raydium_launchlab, …  A new launchpad is a ROW, not a
-- migration. `meta` holds program ids / fee accounts / curve constants (JSONB).
CREATE TABLE IF NOT EXISTS launchpads (
    id                     SMALLINT     PRIMARY KEY,
    key                    TEXT         NOT NULL UNIQUE,   -- 'pump_fun'
    display_name           TEXT         NOT NULL,
    default_quote_asset_id SMALLINT     NOT NULL REFERENCES quote_assets(id),
    meta                   JSONB        NOT NULL DEFAULT '{}',
    created_at             TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ===========================================================================
-- Domain B — Token identity + market state (observed universe)
-- ===========================================================================

-- Static creation facts (write-once), natural key = mint_address (the ONE
-- token-data key). Amounts are base units: initial_supply_base (token),
-- initial_buy_quote (quote). is_own_launch flags tokens WE created.
CREATE TABLE IF NOT EXISTS tokens (
    mint_address          TEXT         PRIMARY KEY,
    launchpad_id          SMALLINT     NOT NULL REFERENCES launchpads(id),
    quote_asset_id        SMALLINT     NOT NULL REFERENCES quote_assets(id),
    creator_wallet        TEXT         NOT NULL,
    is_own_launch         BOOLEAN      NOT NULL DEFAULT FALSE,

    name                  TEXT         NOT NULL,
    symbol                TEXT         NOT NULL,
    decimals              SMALLINT     NOT NULL,           -- token base decimals (6 on pump)
    token_program_id      TEXT,

    initial_supply_base   BIGINT,                          -- token base units
    initial_buy_quote     BIGINT,                          -- quote base units
    creation_slot         BIGINT,
    creation_tx_signature TEXT         NOT NULL,
    ix_labels             JSONB        NOT NULL DEFAULT '[]',
    meta                  JSONB        NOT NULL DEFAULT '{}',
    created_at            TIMESTAMPTZ  NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tokens_creator_wallet ON tokens(creator_wallet);
CREATE INDEX IF NOT EXISTS idx_tokens_created_at      ON tokens(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tokens_launchpad       ON tokens(launchpad_id);
CREATE INDEX IF NOT EXISTS idx_tokens_quote_asset     ON tokens(quote_asset_id);
CREATE INDEX IF NOT EXISTS idx_tokens_is_own_launch   ON tokens(is_own_launch);
-- Default token-list order + stable tiebreak (newest-first, mint tiebreak).
CREATE INDEX IF NOT EXISTS idx_tokens_created_mint    ON tokens (created_at DESC, mint_address DESC);

-- A token's tradeable venue instance(s): curve first, then AMM after graduation.
-- 1..n per token — a token moving curve→amm is a new ROW, not a column flip.
CREATE TABLE IF NOT EXISTS markets (
    id             BIGINT       GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    mint_address   TEXT         NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    launchpad_id   SMALLINT     NOT NULL REFERENCES launchpads(id),
    market_kind    TEXT         NOT NULL CHECK (market_kind IN ('bonding_curve','amm','clmm','orderbook')),
    program_id     TEXT         NOT NULL,
    quote_asset_id SMALLINT     NOT NULL REFERENCES quote_assets(id),
    pool_address   TEXT,
    created_slot   BIGINT,
    UNIQUE (mint_address, launchpad_id, market_kind)
);

CREATE INDEX IF NOT EXISTS idx_markets_mint ON markets(mint_address);

-- Hot-updated live metrics (was tokens_info). PK = FK = mint_address (1:1).
-- Prices are RAW RATIOS (quote base units per base base unit) — see header.
CREATE TABLE IF NOT EXISTS token_market_state (
    mint_address        TEXT         PRIMARY KEY REFERENCES tokens(mint_address) ON DELETE CASCADE,
    current_price_quote DOUBLE PRECISION,                 -- raw spot ratio (quote_bu / base_bu)
    ath_price_quote     DOUBLE PRECISION,
    ath_at              TIMESTAMPTZ,
    volume_quote        BIGINT       NOT NULL DEFAULT 0,  -- quote base units
    trade_count         BIGINT       NOT NULL DEFAULT 0,
    last_trade_at       TIMESTAMPTZ,
    is_dead             BOOLEAN      NOT NULL DEFAULT FALSE,
    is_migrated         BOOLEAN      NOT NULL DEFAULT FALSE,
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tms_is_dead       ON token_market_state(is_dead);
CREATE INDEX IF NOT EXISTS idx_tms_volume_quote  ON token_market_state(volume_quote DESC);
CREATE INDEX IF NOT EXISTS idx_tms_last_trade_at ON token_market_state(last_trade_at DESC);
CREATE INDEX IF NOT EXISTS idx_tms_trade_count   ON token_market_state (trade_count DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tms_current_price ON token_market_state (current_price_quote DESC NULLS LAST);

-- Per-(mint, market) ingest watermark. A new market is a new ROW, not new cols.
CREATE TABLE IF NOT EXISTS token_sync_state (
    mint_address   TEXT         NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    market_id      BIGINT       NOT NULL REFERENCES markets(id) ON DELETE CASCADE,
    last_sig       TEXT,
    last_slot      BIGINT,
    last_synced_at TIMESTAMPTZ,
    PRIMARY KEY (mint_address, market_id)
);

CREATE INDEX IF NOT EXISTS idx_token_sync_state_synced ON token_sync_state(last_synced_at);

-- ===========================================================================
-- Domain C — The feed (high-volume hypertables)
-- ===========================================================================

-- Wallet interning dictionary: trades carry a 4-byte wallet_id instead of a
-- 44-byte base58 address. SOFT reference from trades (no FK on the hot insert —
-- performance budget). A missing dict row must never hide a trade (read paths
-- LEFT JOIN with a COALESCE fallback).
CREATE TABLE IF NOT EXISTS wallet_dict (
    id      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    address TEXT    NOT NULL UNIQUE
);

-- Source-of-truth unparsed feed (BYTEA payload). Shortest retention. `trades`
-- is a typed projection of this — replayable; the feed table IS the transport.
CREATE TABLE IF NOT EXISTS raw_txs (
    tx_signature  BYTEA        NOT NULL,
    slot          BIGINT       NOT NULL,
    block_time    TIMESTAMPTZ  NOT NULL,
    tx_index      INTEGER      NOT NULL,
    payload       BYTEA        NOT NULL,
    source        SMALLINT     NOT NULL DEFAULT 0,   -- 0=live 1=sync
    PRIMARY KEY (block_time, tx_signature)           -- partition col first; IS the dedup key
);

SELECT create_hypertable('raw_txs', by_range('block_time', INTERVAL '1 day'), if_not_exists => TRUE);

ALTER TABLE raw_txs SET (
    timescaledb.compress,
    timescaledb.compress_orderby = 'slot, tx_index'
);
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '2 days', if_not_exists => TRUE);
SELECT add_retention_policy('raw_txs', drop_after => INTERVAL '7 days', if_not_exists => TRUE);

-- Typed projection; quote/venue-generalized. Integer base units, BYTEA
-- signature, interned wallet_id, no surrogate key (the dedup key IS the PK).
-- launchpad_id / market_kind / quote_asset_id are DENORMALIZED onto the row so
-- the hot read never joins. Reserves are a single venue-neutral pair
-- (reserve_quote/reserve_base): curve virtual reserves on curve rows, pool real
-- reserves on amm rows. Ordering key (execution order): (slot, tx_index,
-- leg_index); block_time = wall-clock partition + candle-bucket axis.
CREATE TABLE IF NOT EXISTS trades (
    mint_address    TEXT         NOT NULL,
    wallet_id       INTEGER      NOT NULL,             -- soft ref → wallet_dict(id)
    launchpad_id    SMALLINT     NOT NULL,             -- denormalized (hot path, no join)
    market_kind     TEXT         NOT NULL CHECK (market_kind IN ('bonding_curve','amm','clmm','orderbook')),
    quote_asset_id  SMALLINT     NOT NULL,             -- denormalized
    trade_type      TEXT         NOT NULL CHECK (trade_type IN ('buy','sell')),

    amount_quote    BIGINT       NOT NULL,             -- quote base units (was amount_lamports)
    amount_base     BIGINT       NOT NULL,             -- token base units (was token_amount)
    reserve_quote   BIGINT,                            -- venue-neutral price pair
    reserve_base    BIGINT,

    slot            BIGINT       NOT NULL,
    tx_index        INTEGER      NOT NULL,
    leg_index       SMALLINT     NOT NULL DEFAULT 0,
    block_time      TIMESTAMPTZ  NOT NULL,
    tx_signature    BYTEA        NOT NULL,

    PRIMARY KEY (block_time, tx_signature, leg_index)  -- partition col first; IS the dedup key
);

SELECT create_hypertable('trades', by_range('block_time', INTERVAL '1 day'), if_not_exists => TRUE);

-- Per-mint chronological reads with exact intra-block order (recent chunks).
CREATE INDEX IF NOT EXISTS idx_trades_mint_order ON trades(mint_address, slot, tx_index, leg_index);

ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'mint_address',
    timescaledb.compress_orderby   = 'slot, tx_index, leg_index'
);
-- compress_after MUST exceed the sync/backfill lookback horizon.
SELECT add_compression_policy('trades', compress_after => INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('trades', drop_after => INTERVAL '30 days', if_not_exists => TRUE);

-- ===========================================================================
-- Derived-never-stored views (decimals + USD applied HERE, not in storage)
-- ===========================================================================

-- Priced trade read view. Joins the tiny quote_assets dimension for decimals +
-- usd_rate. exec_price/spot_price are raw ratios; amount_usd is the cross-quote
-- numeraire that makes a SOL trade and a USDC trade comparable.
DROP VIEW IF EXISTS trades_priced;
CREATE VIEW trades_priced AS
SELECT
    t.*,
    qa.symbol   AS quote_symbol,
    qa.decimals AS quote_decimals,
    qa.usd_rate AS quote_usd_rate,
    -- raw ratios (quote base units per base base unit)
    (t.amount_quote::double precision  / NULLIF(t.amount_base, 0))  AS exec_price_quote,
    (t.reserve_quote::double precision / NULLIF(t.reserve_base, 0)) AS spot_price_quote,
    -- display quote + USD (the generality payoff)
    (t.amount_quote::double precision / power(10, qa.decimals))                    AS amount_quote_display,
    (t.amount_quote::double precision / power(10, qa.decimals) * qa.usd_rate)      AS amount_usd
FROM trades t
JOIN quote_assets qa ON qa.id = t.quote_asset_id;

-- Full token picture; age / market_cap / USD derived (never stored). LEFT JOIN
-- token_market_state so freshly-created (pre-sync) tokens still appear. Joins
-- quote_assets for the decimal + USD projection.
--   price_quote_display = raw ratio × 10^(base_dec − quote_dec)  (display quote per display token)
--   market_cap_quote    = raw ratio × supply_base ÷ 10^quote_dec (display quote FDV)
DROP VIEW IF EXISTS token_overview;
CREATE VIEW token_overview AS
SELECT
    t.mint_address,
    t.launchpad_id,
    t.quote_asset_id,
    t.creator_wallet,
    t.is_own_launch,
    t.name,
    t.symbol,
    t.decimals,
    t.token_program_id,
    t.initial_supply_base,
    t.initial_buy_quote,
    t.creation_slot,
    t.creation_tx_signature,
    t.ix_labels,
    t.meta,
    t.created_at,
    s.current_price_quote,
    s.ath_price_quote,
    s.ath_at,
    s.volume_quote,
    s.trade_count,
    s.last_trade_at,
    s.is_dead,
    s.is_migrated,
    s.updated_at AS metrics_updated_at,
    qa.symbol   AS quote_symbol,
    qa.decimals AS quote_decimals,
    qa.usd_rate AS quote_usd_rate,
    EXTRACT(EPOCH FROM (now() - t.created_at))::bigint                                   AS age_secs,
    -- display quote per display token, and its USD
    (s.current_price_quote * power(10, t.decimals - qa.decimals))                        AS price_quote_display,
    (s.current_price_quote * power(10, t.decimals - qa.decimals) * qa.usd_rate)          AS price_usd,
    -- market cap (FDV) in display quote, and its USD
    (s.current_price_quote * t.initial_supply_base / power(10, qa.decimals))             AS market_cap_quote,
    (s.current_price_quote * t.initial_supply_base / power(10, qa.decimals) * qa.usd_rate) AS market_cap_usd
FROM tokens t
LEFT JOIN token_market_state s USING (mint_address)
JOIN quote_assets qa ON qa.id = t.quote_asset_id;

-- ===========================================================================
-- Seeds — quote_assets {SOL, USDC}, launchpads {pump_fun}.
--   Interned ids are stable references stamped on hot rows: SOL=1, USDC=2;
--   pump_fun=1. Dependency order: quote_assets (referenced) before launchpads.
-- ===========================================================================
INSERT INTO quote_assets (id, mint, symbol, decimals, is_native, usd_rate, usd_rate_at) VALUES
    (1, 'So11111111111111111111111111111111111111112', 'SOL',  9, TRUE,  NULL, NULL),
    (2, 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', 'USDC', 6, FALSE, 1.0,  now())
ON CONFLICT (id) DO NOTHING;

INSERT INTO launchpads (id, key, display_name, default_quote_asset_id, meta) VALUES
    (1, 'pump_fun', 'Pump.fun', 1, '{}')
ON CONFLICT (id) DO NOTHING;
