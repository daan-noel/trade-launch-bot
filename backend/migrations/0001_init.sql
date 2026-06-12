-- ============================================================================
-- Initial schema (squashed).
--
-- This single migration is the full schema after the original 0001–0013
-- migrations were merged. It reflects the FINAL state: table/column renames
-- (tpsl1_*/tpsl2_* prefixes, p_token_/p_entry_/p_exit_ param prefixes),
-- partitioned raw_transactions / raw_transactions_grpc, and all added columns
-- are baked in directly. Tables are ordered so foreign-key targets are created
-- before their referrers.
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
-- raw_transactions  (unified raw tx result — never mutated, replay/analysis source)
--   Single write-only table for both ingest provenances, tagged by `source`:
--     'grpc' — live LaserStream (Yellowstone) pipeline
--     'rpc'  — token_sync archival/replay backfill
--   WEEKLY range partitions on received_at with ~2-month retention (managed by
--   the app's partition-maintenance task; KEEP_WEEKS must match
--   ingest::maintenance::KEEP_WEEKS = 9). No UNIQUE(signature): a unique
--   constraint on a partitioned table must include the partition key, and
--   received_at isn't stable across reconnect re-delivery — duplicates are
--   tolerated in this write-only analysis table.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS raw_transactions (
    id          UUID        NOT NULL DEFAULT uuid_generate_v4(),
    signature   TEXT        NOT NULL,
    slot        BIGINT      NOT NULL,
    block_time  TIMESTAMPTZ NOT NULL,
    raw_data    JSONB       NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    source      TEXT        NOT NULL DEFAULT 'grpc',
    PRIMARY KEY (received_at, id)
) PARTITION BY RANGE (received_at);

-- Non-unique indexes on the parent cascade to all (incl. future) partitions.
CREATE INDEX IF NOT EXISTS idx_raw_tx_signature ON raw_transactions (signature);
CREATE INDEX IF NOT EXISTS idx_raw_tx_slot      ON raw_transactions (slot DESC);

-- Idempotently create the weekly partition (Mon–Mon) containing p_anchor.
CREATE OR REPLACE FUNCTION ensure_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_end   date := v_start + 7;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF raw_transactions FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamptz, v_end::timestamptz
    );
END;
$$;

-- Drop the weekly partition containing p_anchor, if it exists (retention).
CREATE OR REPLACE FUNCTION drop_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := date_trunc('week', p_anchor::timestamp)::date;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- Pre-create the retained window (now-9w .. now+2w) so the first inserts always
-- have a target partition before the maintenance task first runs.
DO $$
DECLARE w date := date_trunc('week', (now() - interval '9 weeks'))::date;
BEGIN
    WHILE w <= date_trunc('week', (now() + interval '2 weeks'))::date LOOP
        PERFORM ensure_raw_partition(w);
        w := w + 7;
    END LOOP;
END $$;

-- -------------------------------------------------------------------------
-- tokens_info
--   last_synced_at         — wall-clock time of the last successful sync (display).
--   last_synced_curve_sig  — newest bonding-curve signature seen, used as the
--                            `until` boundary for the next incremental sync.
--   last_synced_amm_sig    — same, for post-migration PumpSwap (AMM) signatures.
--   last_synced_*_slot     — slot of the newest signature per venue, so a
--                            LaserStream-replay Fetch New can resume by slot.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tokens_info (
    id                     UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address           TEXT        UNIQUE NOT NULL,
    ath_price              DOUBLE PRECISION,
    ath_timestamp          TIMESTAMPTZ,
    age                    BIGINT,
    volume                 DOUBLE PRECISION DEFAULT 0.0,
    market_cap             DOUBLE PRECISION,
    trade_count            BIGINT      NOT NULL DEFAULT 0,
    last_trade_at          TIMESTAMPTZ,
    current_price          DOUBLE PRECISION,
    is_rugged              BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated            BOOLEAN     NOT NULL DEFAULT FALSE,
    last_synced_at         TIMESTAMPTZ,
    last_synced_curve_sig  TEXT,
    last_synced_amm_sig    TEXT,
    last_synced_curve_slot BIGINT,
    last_synced_amm_slot   BIGINT,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
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

-- =========================================================================
-- tpsl_sniper_1 storage (live strategy)
-- =========================================================================

-- -------------------------------------------------------------------------
-- tpsl1_strategy_rules
--   Param prefixes:
--     p_token_*  — token fingerprint (which token the rule matches at creation)
--     p_exit_*   — exit gates (thresholds that decide when to sell)
--   p_exit_trailing_stop_pct  — exit when price falls X% below peak-since-entry.
--   p_exit_time_stop_secs     — exit at first trade >= N secs after entry.
--   p_exit_stall_secs         — exit when no new higher-high for N secs.
--   p_exit_liquidity_drop_pct — exit once virtual SOL reserves crash X% below peak.
--   All exit gates are inert by default (0 / NULL = disabled, per the
--   ignore_zero_* convention in the tpsl strategy).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_strategy_rules (
    id                          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name                   TEXT        NOT NULL,
    p_token_initial_buy_sol     DOUBLE PRECISION,
    p_token_cu_limit            BIGINT,
    p_token_cu_price            BIGINT,
    p_token_max_sol_cost        DOUBLE PRECISION,
    p_token_spendable_sol_in    DOUBLE PRECISION,
    p_max_concurrent_tokens     BIGINT,
    p_max_total_tokens          BIGINT,
    p_token_ix_labels           JSONB       NOT NULL DEFAULT '[]',
    p_exit_trailing_stop_pct    DOUBLE PRECISION,
    p_exit_time_stop_secs       BIGINT,
    p_exit_stall_secs           BIGINT,
    p_exit_liquidity_drop_pct   DOUBLE PRECISION,
    buy_amount                  DOUBLE PRECISION NOT NULL,
    p_exit_take_profit          DOUBLE PRECISION NOT NULL,
    p_exit_stop_loss            DOUBLE PRECISION NOT NULL,
    tolerance_pct               DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    trade_mode                  TEXT        NOT NULL DEFAULT 'paper',
    is_active                   BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_strategy_rules_active ON tpsl1_strategy_rules(is_active);

-- -------------------------------------------------------------------------
-- tpsl1_real_positions
--   status      — Holding / ExitPending / End / ExitFailed.
--   exit_reason — TakeProfit / StopLoss / TrailingStop / Stall / TimeStop /
--                 LiquidityExit (NULL until the position exits).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_real_positions (
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
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    exit_reason         TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (rule_id) REFERENCES tpsl1_strategy_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_mint              ON tpsl1_real_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_wallet            ON tpsl1_real_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_status            ON tpsl1_real_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_strategy          ON tpsl1_real_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_rule_id           ON tpsl1_real_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_mint_status       ON tpsl1_real_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_token_program_id  ON tpsl1_real_positions(token_program_id);

-- -------------------------------------------------------------------------
-- tpsl1_paper_test_run
-- Paper-trading runs, isolated from the real positions table. A paper "run"
-- begins when a paper rule is activated and ends when its total-token cap is
-- reached (rule auto-deactivates) or it is manually stopped. Only the latest
-- run per rule is retained (starting a new run deletes the prior run, and its
-- positions go with it via ON DELETE CASCADE).
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_paper_test_run (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_id            UUID        NOT NULL REFERENCES tpsl1_strategy_rules(id) ON DELETE CASCADE,
    -- Monotonic per-rule run counter (1, 2, 3 …) for display ("run #N").
    run_seq            BIGINT      NOT NULL,
    status             TEXT        NOT NULL CHECK (status IN ('Running', 'Finished', 'Stopped')),
    -- Snapshot of the rule's total-token cap at run start (NULL = unlimited).
    max_total_tokens   BIGINT,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    UNIQUE (rule_id, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_paper_test_run_rule ON tpsl1_paper_test_run(rule_id);

-- -------------------------------------------------------------------------
-- tpsl1_paper_positions
-- Mirrors tpsl1_real_positions (minus the UNIQUE constraints on entry_tx /
-- exit_tx — a paper position is seeded with the token's creation tx and the same
-- token may be traded again in a later run) plus a run_id binding each position
-- to its run.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl1_paper_test_run(id) ON DELETE CASCADE,
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
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    exit_reason         TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_paper_positions_run         ON tpsl1_paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_tpsl1_paper_positions_mint_status ON tpsl1_paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl1_paper_positions_rule        ON tpsl1_paper_positions(rule_id);

-- =========================================================================
-- tpsl_sniper_2 storage (independent clone; diverges with the scalp experiment)
-- =========================================================================

-- -------------------------------------------------------------------------
-- tpsl2_strategy_rules
--   Adds the scalp-continuation entry gates (p_entry_*) and the cohort-dump
--   exit gate (p_exit_cohort_ratio) on top of the tpsl1 shape. All gates are
--   nullable and inert at NULL/0 (ignore_zero convention).
--     p_entry_min_age_secs       — skip the launch spike (entry age >= N secs)
--     p_entry_min_alive_sol      — liveness: total SOL traded in trailing window
--     p_entry_min_organic_sol    — net SOL bought by outside (non-cohort) wallets
--     p_entry_pullback_pct       — higher-low: min dip off the local high to count
--     p_entry_higher_low_secs    — higher-low: min span the pattern must form over
--     p_entry_max_cohort_held    — launch cohort must have sold down to <= this ratio
--     p_entry_min_liquidity_sol  — min real SOL reserves
--     p_entry_min_organic_liq    — min real reserves sourced from outside buyers
--     p_exit_cohort_ratio        — cohort net holdings collapsed to <= this -> exit
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_strategy_rules (
    id                          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name                   TEXT        NOT NULL,
    p_token_initial_buy_sol     DOUBLE PRECISION,
    p_token_cu_limit            BIGINT,
    p_token_cu_price            BIGINT,
    p_token_max_sol_cost        DOUBLE PRECISION,
    p_token_spendable_sol_in    DOUBLE PRECISION,
    p_max_concurrent_tokens     BIGINT,
    p_max_total_tokens          BIGINT,
    p_token_ix_labels           JSONB       NOT NULL DEFAULT '[]',
    p_exit_trailing_stop_pct    DOUBLE PRECISION,
    p_exit_time_stop_secs       BIGINT,
    p_exit_stall_secs           BIGINT,
    p_exit_liquidity_drop_pct   DOUBLE PRECISION,
    buy_amount                  DOUBLE PRECISION NOT NULL,
    p_exit_take_profit          DOUBLE PRECISION NOT NULL,
    p_exit_stop_loss            DOUBLE PRECISION NOT NULL,
    tolerance_pct               DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    trade_mode                  TEXT        NOT NULL DEFAULT 'paper',
    is_active                   BOOLEAN     NOT NULL DEFAULT TRUE,
    -- Scalp-continuation entry gates (tpsl2 only).
    p_entry_min_age_secs        BIGINT,
    p_entry_min_alive_sol       DOUBLE PRECISION,
    p_entry_min_organic_sol     DOUBLE PRECISION,
    p_entry_pullback_pct        DOUBLE PRECISION,
    p_entry_higher_low_secs     BIGINT,
    p_entry_max_cohort_held     DOUBLE PRECISION,
    p_entry_min_liquidity_sol   DOUBLE PRECISION,
    p_entry_min_organic_liq     DOUBLE PRECISION,
    -- Scalp-continuation cohort-dump exit gate (tpsl2 only).
    p_exit_cohort_ratio         DOUBLE PRECISION,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_strategy_rules_active ON tpsl2_strategy_rules(is_active);

-- -------------------------------------------------------------------------
-- tpsl2_real_positions  (clone of tpsl1_real_positions)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_real_positions (
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
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    exit_reason         TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (rule_id) REFERENCES tpsl2_strategy_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint              ON tpsl2_real_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_wallet            ON tpsl2_real_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_status            ON tpsl2_real_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_strategy          ON tpsl2_real_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_rule_id           ON tpsl2_real_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint_status       ON tpsl2_real_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_token_program_id  ON tpsl2_real_positions(token_program_id);

-- -------------------------------------------------------------------------
-- tpsl2_paper_test_run  (clone of tpsl1_paper_test_run)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_paper_test_run (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_id            UUID        NOT NULL REFERENCES tpsl2_strategy_rules(id) ON DELETE CASCADE,
    run_seq            BIGINT      NOT NULL,
    status             TEXT        NOT NULL CHECK (status IN ('Running', 'Finished', 'Stopped')),
    max_total_tokens   BIGINT,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    UNIQUE (rule_id, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_test_run_rule ON tpsl2_paper_test_run(rule_id);

-- -------------------------------------------------------------------------
-- tpsl2_paper_positions  (clone of tpsl1_paper_positions)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl2_paper_test_run(id) ON DELETE CASCADE,
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
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    exit_reason         TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_run         ON tpsl2_paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_mint_status ON tpsl2_paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_rule        ON tpsl2_paper_positions(rule_id);

-- =========================================================================
-- Wallet directory + global settings
-- =========================================================================

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
