-- ============================================================================
-- 0001_init — consolidated platform schema (single-file init). TimescaleDB.
--
-- Squash of the entire 0001..0013 migration chain into ONE end-state file — it
-- creates the schema exactly as running the full chain on a fresh database would
-- leave it (all renames/adds/drops/CHECK-tightenings already folded in). Absorbs:
--   * 0001 init                     dimensions + token identity + feed + views + seeds
--   * 0002 own_launch               managed_wallets, launch_templates, launches, bundles
--   * 0003 bundle_confirm           bundles jito_bundle_id/leg_signatures/*_at + index
--   * 0004 wallet_pool              managed_wallets status lifecycle (replaces is_active)
--   * 0005 metadata_templates       metadata_templates table
--   * 0006 status_check             launches/bundles named status CHECKs (+ default heal)
--   * 0007 launch_metadata_fk       launch_templates.metadata_template_id FK; image_uri NULL
--   * 0008 wallet_funding           managed_wallets 'funding' status + pollable index
--   * 0009 collapse_devbuy_variant  DATA-ONLY (variant string rewrite) — omitted
--   * 0010 token_positions          token_positions table
--   * 0011 manage_actions           manage_actions table
--   * 0012 sell_ladders             sell_ladders table
--   * 0013 volume_bots              volume_bots table
-- Data-only migrations (0009's variant rewrite, and the 0004/0006/0007 backfills
-- that only rewrote pre-existing rows) are no-ops on a fresh DB and intentionally
-- omitted here. See the per-domain plan docs for column rationale.
--
-- Naming rule (locked): a column denoting an amount names its unit as a SUFFIX —
-- `amount_quote`/`amount_base` (exact BIGINT base units, display ÷10^decimals);
-- reserves `reserve_quote`/`reserve_base`. Ratios keep `_price`. Wallet gas
-- balances are native lamports (`balance_lamports`) — no quote asset to reference.
--
-- Price convention (locked): every STORED / AGGREGATED price is a RAW RATIO in
-- "quote base units per base base unit" (p = amount_quote / amount_base) —
-- decimals-agnostic; decimals + USD are applied ONLY in derived views.
--
-- TimescaleDB note: hypertable creation + compression/retention policies are
-- transaction-safe and live here. The OHLCV continuous aggregates were dropped
-- (audit 2026-07-13: no reader ever consumed them); boot now runs an idempotent
-- teardown, `platform_core::storage::timescale::teardown_dead_caggs`, so already
-- deployed DBs stop paying the per-minute refresh. Re-add CAgg creation there
-- (outside a txn — CAggs can't be created in one) when a candle reader exists.
--
-- CHECK IN-lists are the SQL half of the SSOT owned by
-- `platform_core::models::status` (LaunchStatus / BundleStatus / WalletStatus /
-- WalletRole / PositionStatus / ManageKind / ManageSizing / ManageStatus /
-- LadderStatus / VolumeBotStatus). Keep them byte-equal to each `as_str()`.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ===========================================================================
-- Domain A — Dimensions (small, slow-changing, interned onto hot rows)
-- ===========================================================================

-- Quote assets: SOL, USDC, …  Native SOL is the row whose base unit is the
-- lamport (is_native, decimals 9). usd_rate is the cross-quote numeraire.
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
-- token-data key). is_own_launch flags tokens WE created.
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
CREATE INDEX IF NOT EXISTS idx_tokens_created_mint    ON tokens (created_at DESC, mint_address DESC);

-- A token's tradeable venue instance(s): curve first, then AMM after graduation.
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

-- Hot-updated live metrics. PK = FK = mint_address (1:1). Prices are RAW RATIOS.
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

-- Wallet interning dictionary. SOFT reference from trades (no FK on hot insert).
CREATE TABLE IF NOT EXISTS wallet_dict (
    id      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    address TEXT    NOT NULL UNIQUE
);

-- Source-of-truth unparsed feed (BYTEA payload). Shortest retention.
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

-- Typed projection; quote/venue-generalized. launchpad_id / market_kind /
-- quote_asset_id are DENORMALIZED onto the row so the hot read never joins.
CREATE TABLE IF NOT EXISTS trades (
    mint_address    TEXT         NOT NULL,
    wallet_id       INTEGER      NOT NULL,             -- soft ref → wallet_dict(id)
    launchpad_id    SMALLINT     NOT NULL,             -- denormalized (hot path, no join)
    market_kind     TEXT         NOT NULL CHECK (market_kind IN ('bonding_curve','amm','clmm','orderbook')),
    quote_asset_id  SMALLINT     NOT NULL,             -- denormalized
    trade_type      TEXT         NOT NULL CHECK (trade_type IN ('buy','sell')),

    amount_quote    BIGINT       NOT NULL,             -- quote base units
    amount_base     BIGINT       NOT NULL,             -- token base units
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

CREATE INDEX IF NOT EXISTS idx_trades_mint_order ON trades(mint_address, slot, tx_index, leg_index);

ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'mint_address',
    timescaledb.compress_orderby   = 'slot, tx_index, leg_index'
);
SELECT add_compression_policy('trades', compress_after => INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('trades', drop_after => INTERVAL '30 days', if_not_exists => TRUE);

-- ===========================================================================
-- Derived-never-stored views (decimals + USD applied HERE, not in storage)
-- ===========================================================================

DROP VIEW IF EXISTS trades_priced;
CREATE VIEW trades_priced AS
SELECT
    t.*,
    qa.symbol   AS quote_symbol,
    qa.decimals AS quote_decimals,
    qa.usd_rate AS quote_usd_rate,
    (t.amount_quote::double precision  / NULLIF(t.amount_base, 0))  AS exec_price_quote,
    (t.reserve_quote::double precision / NULLIF(t.reserve_base, 0)) AS spot_price_quote,
    (t.amount_quote::double precision / power(10, qa.decimals))                    AS amount_quote_display,
    (t.amount_quote::double precision / power(10, qa.decimals) * qa.usd_rate)      AS amount_usd
FROM trades t
JOIN quote_assets qa ON qa.id = t.quote_asset_id;

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
    (s.current_price_quote * power(10, t.decimals - qa.decimals))                        AS price_quote_display,
    (s.current_price_quote * power(10, t.decimals - qa.decimals) * qa.usd_rate)          AS price_usd,
    (s.current_price_quote * t.initial_supply_base / power(10, qa.decimals))             AS market_cap_quote,
    (s.current_price_quote * t.initial_supply_base / power(10, qa.decimals) * qa.usd_rate) AS market_cap_usd
FROM tokens t
LEFT JOIN token_market_state s USING (mint_address)
JOIN quote_assets qa ON qa.id = t.quote_asset_id;

-- ===========================================================================
-- Seeds — quote_assets {SOL, USDC}, launchpads {pump_fun}.
-- ===========================================================================
INSERT INTO quote_assets (id, mint, symbol, decimals, is_native, usd_rate, usd_rate_at) VALUES
    (1, 'So11111111111111111111111111111111111111112', 'SOL',  9, TRUE,  NULL, NULL),
    (2, 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', 'USDC', 6, FALSE, 1.0,  now())
ON CONFLICT (id) DO NOTHING;

INSERT INTO launchpads (id, key, display_name, default_quote_asset_id, meta) VALUES
    (1, 'pump_fun', 'Pump.fun', 1, '{}')
ON CONFLICT (id) DO NOTHING;

-- ===========================================================================
-- Domain D — Own-launch (creation) + wallet pool + management
-- ===========================================================================

-- OUR wallets (dev / bundler / treasury / trading). key_ref only — no key bytes.
-- End state of the 0004 + 0008 lifecycle: `is_active` is GONE, replaced by the
-- `status` lifecycle  generated → funding → funded → reserved → used → retired.
-- `reserved_by_launch_id`'s FK → launches is added after `launches` exists
-- (circular: launches.dev_wallet_id → managed_wallets.id).
CREATE TABLE IF NOT EXISTS managed_wallets (
    id                    UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    address               TEXT         NOT NULL UNIQUE,
    label                 TEXT,
    role                  TEXT         NOT NULL CHECK (role IN ('dev','bundler','treasury','trading')),
    key_ref               TEXT         NOT NULL,        -- external keystore/KMS ref, NEVER a raw key
    derivation_index      INTEGER,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT now(),

    -- wallet-pool lifecycle (0004) + funding orchestration (0008).
    status                TEXT         NOT NULL DEFAULT 'generated'
                              CHECK (status IN ('generated','funding','funded','reserved','used','retired')),
    funding_source        TEXT,                          -- free-text audit note
    reserved_by_launch_id UUID,                          -- FK → launches(id) added below
    reserved_at           TIMESTAMPTZ,
    balance_lamports      BIGINT,                        -- native SOL balance (pool bookkeeping)
    balance_checked_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_managed_wallets_role ON managed_wallets(role);
-- Bounds the reservation TTL sweep to the (small) in-flight reserved set.
CREATE INDEX IF NOT EXISTS idx_managed_wallets_reserved
    ON managed_wallets(reserved_at) WHERE status = 'reserved';
-- Bounds the atomic claim query (`role = $1 AND status = 'funded'`).
CREATE INDEX IF NOT EXISTS idx_managed_wallets_role_funded
    ON managed_wallets(role) WHERE status = 'funded';
-- Bounds the balance poller's scan to the pollable (generated|funding) set.
CREATE INDEX IF NOT EXISTS idx_managed_wallets_pollable
    ON managed_wallets(created_at) WHERE status IN ('generated','funding');

-- Authored token-metadata content, pinned to a hosted JSON `uri`. image_uri is
-- NULLable (0007): a preset backfilled from a legacy template has no separately-
-- pinned image (it's embedded in the metadata JSON at `uri`).
CREATE TABLE IF NOT EXISTS metadata_templates (
    id            UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_name TEXT         NOT NULL,
    name          TEXT         NOT NULL,
    symbol        TEXT         NOT NULL,
    description   TEXT,
    twitter       TEXT,
    telegram      TEXT,
    website       TEXT,
    image_uri     TEXT,                    -- ipfs://<cid> — the pinned image (nullable, 0007)
    uri           TEXT         NOT NULL,   -- ipfs://<cid> — the pinned metadata JSON
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_metadata_templates_created ON metadata_templates(created_at DESC);

-- Authored launch specs. `metadata_template_id` (0007) is the SSOT for
-- name/symbol/uri (resolved at launch); the old inlined params.{name,symbol,uri}
-- were removed by the 0007 data backfill.
CREATE TABLE IF NOT EXISTS launch_templates (
    id                   UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_name        TEXT         NOT NULL,
    launchpad_id         SMALLINT     NOT NULL REFERENCES launchpads(id),
    variant              TEXT         NOT NULL,          -- 'pumpfun.create_v1' | '…create_v2'
    quote_asset_id       SMALLINT     NOT NULL REFERENCES quote_assets(id),
    params               JSONB        NOT NULL DEFAULT '{}',
    metadata_template_id UUID         REFERENCES metadata_templates(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_launch_templates_launchpad ON launch_templates(launchpad_id);
CREATE INDEX IF NOT EXISTS idx_launch_templates_metadata  ON launch_templates(metadata_template_id);

-- Executed launch record. `status` CHECK pinned by 0006. `bundle_id` is a soft
-- back-ref to bundles (bare UUID, no FK).
CREATE TABLE IF NOT EXISTS launches (
    id               UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_id      UUID         REFERENCES launch_templates(id) ON DELETE SET NULL,
    mint_address     TEXT         NOT NULL,
    launchpad_id     SMALLINT     NOT NULL REFERENCES launchpads(id),
    variant          TEXT         NOT NULL,
    quote_asset_id   SMALLINT     NOT NULL REFERENCES quote_assets(id),
    dev_wallet_id    UUID         REFERENCES managed_wallets(id),
    create_signature TEXT,
    dev_buy_quote    BIGINT,                        -- quote base units
    bundle_id        UUID,                          -- soft ref → bundles.id
    status           TEXT         NOT NULL DEFAULT 'pending'
                         CHECK (status IN ('pending', 'created', 'failed')),
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_launches_mint     ON launches(mint_address);
CREATE INDEX IF NOT EXISTS idx_launches_template ON launches(template_id);
CREATE INDEX IF NOT EXISTS idx_launches_status   ON launches(status, created_at DESC);

-- Atomic Jito bundle of a launch's buy legs. `status` default 'planned' + CHECK
-- (0006). Confirm columns (jito_bundle_id/leg_signatures/*_at) from 0003.
CREATE TABLE IF NOT EXISTS bundles (
    id             UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    launch_id      UUID         NOT NULL REFERENCES launches(id) ON DELETE CASCADE,
    status         TEXT         NOT NULL DEFAULT 'planned'
                       CHECK (status IN ('planned', 'submitting', 'submitted',
                                         'landed', 'dropped', 'partial', 'failed')),
    tip_quote      BIGINT,                             -- total Jito tip, quote base units
    legs           JSONB        NOT NULL DEFAULT '[]',
    jito_bundle_id TEXT,
    leg_signatures TEXT[]       NOT NULL DEFAULT '{}',
    submitted_at   TIMESTAMPTZ,
    confirmed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_bundles_launch ON bundles(launch_id);
-- Bounds the confirm watcher's poll to the tiny in-flight set.
CREATE INDEX IF NOT EXISTS idx_bundles_status_submitted
    ON bundles(status, submitted_at)
    WHERE status = 'submitted';

-- Close the circular FK now that `launches` exists (managed_wallets predates it).
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'managed_wallets_reserved_by_launch_id_fkey'
    ) THEN
        ALTER TABLE managed_wallets
            ADD CONSTRAINT managed_wallets_reserved_by_launch_id_fkey
            FOREIGN KEY (reserved_by_launch_id) REFERENCES launches(id) ON DELETE SET NULL;
    END IF;
END $$;

-- ===========================================================================
-- Post-launch token management (0010..0013). NO FK on mint_address on any of
-- these — a position/action can target a launch before its create tx is ingested
-- into `tokens`. wallet_id DOES FK — the wallet always pre-exists.
-- ===========================================================================

-- Per-wallet holdings read model (0010).
CREATE TABLE IF NOT EXISTS token_positions (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address       TEXT        NOT NULL,
    wallet_id          UUID        NOT NULL REFERENCES managed_wallets(id),
    role               TEXT        NOT NULL CHECK (role IN ('dev','bundler','treasury','trading')),
    token_account      TEXT,                    -- canonical ATA buys/sells route through
    balance_base       BIGINT      NOT NULL DEFAULT 0,   -- tokens held (token base units)
    cost_quote         BIGINT      NOT NULL DEFAULT 0,   -- cost basis (quote base units)
    realized_quote     BIGINT      NOT NULL DEFAULT 0,   -- quote recovered from sells
    balance_checked_at TIMESTAMPTZ,
    status             TEXT        NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (mint_address, wallet_id)
);

CREATE INDEX IF NOT EXISTS idx_token_positions_mint ON token_positions(mint_address);

-- Audit log of executed management actions (0011).
CREATE TABLE IF NOT EXISTS manage_actions (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address   TEXT        NOT NULL,
    kind           TEXT        NOT NULL CHECK (kind IN ('sell','buy','consolidate')),
    sizing         TEXT        NOT NULL CHECK (sizing IN
                       ('pct_of_holdings','to_sol_target','fixed_base','fixed_sol','sweep')),
    selection      JSONB       NOT NULL DEFAULT '{}'::jsonb,
    plan           JSONB       NOT NULL DEFAULT '[]'::jsonb,
    status         TEXT        NOT NULL DEFAULT 'planned' CHECK (status IN
                       ('planned','executing','completed','partial','failed')),
    legs_total     INTEGER     NOT NULL DEFAULT 0,
    legs_confirmed INTEGER     NOT NULL DEFAULT 0,
    error          TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_manage_actions_mint ON manage_actions(mint_address, created_at DESC);

-- Simple-threshold sell ladders (0012).
CREATE TABLE IF NOT EXISTS sell_ladders (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address TEXT        NOT NULL,
    selection    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    rungs        JSONB       NOT NULL DEFAULT '[]'::jsonb,
    status       TEXT        NOT NULL DEFAULT 'armed' CHECK (status IN ('armed','done','cancelled')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sell_ladders_armed ON sell_ladders(mint_address) WHERE status = 'armed';

-- Volume-making bots (0013).
CREATE TABLE IF NOT EXISTS volume_bots (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address  TEXT        NOT NULL,
    status        TEXT        NOT NULL DEFAULT 'running' CHECK (status IN ('running','paused','stopped')),
    selection     JSONB       NOT NULL DEFAULT '{}'::jsonb,
    config        JSONB       NOT NULL,
    cycles_done   INT         NOT NULL DEFAULT 0,
    spent_quote   BIGINT      NOT NULL DEFAULT 0,   -- cumulative buy spend (lamports)
    volume_quote  BIGINT      NOT NULL DEFAULT 0,   -- cumulative notional traded (lamports)
    next_run_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_volume_bots_due ON volume_bots(next_run_at) WHERE status = 'running';
