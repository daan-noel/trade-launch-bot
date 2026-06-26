-- ============================================================================
-- Initial schema (fully squashed — single init, migrations 0001–0006 included).
--
-- Every prior migration has been folded in directly:
--   * tpsl1_*/tpsl2_* table prefixes, p_token_/p_entry_/p_exit_ param prefixes,
--     the perf/composite indexes, the app_settings key-value store, the tpsl2
--     positions `target_*` columns (target_*/entry_*/exit_* order), and the
--     raw_transactions `source` = 'live'/'sync' vocabulary.
--   * `trades` is created **partitioned from the start** (daily RANGE on
--     block_time, 1-month retention) — the old copy-migration dance (rename aside,
--     recreate, copy rows, drop) is unnecessary on a fresh DB.
--   * The legacy *flat* tpsl2 sweep tables (tpsl2_sweep_runs/results) are omitted
--     entirely: they were created then dropped in favor of the strategy-agnostic
--     *grouped* sweep tables, which are the only sweep tables created here.
--   * Both strategies' *grouped* sweep triples (tpsl1_* and tpsl2_*) are created
--     here — the per-strategy tables formerly added by a later migration are
--     folded in, including the `best_score` column on both *_grouped_sweep_groups.
--   * tokens_info uses `is_dead` directly (the `is_rugged` -> `is_dead` rename is
--     folded in), and carries its id PRIMARY KEY / mint_address UNIQUE from the
--     start (the later constraint-restore migration is a no-op on a fresh DB).
--   * tokens_info has `lifetime_secs` from the start.
--   * grouped_sweep_runs tables have status/groups_done lifecycle columns and the
--     full history metadata (ix_labels_filter, field_filters, token_cap, max_combos,
--     label) from the start.
--   * grouped_sweep_groups tables have `mints TEXT[]` from the start.
--   * grouped_sweep_results tables use INTEGER counts and REAL floats; `params` is
--     in the separate *_grouped_sweep_combos dictionary table.
--   * All four position tables use `entry_tx_signatures`/`exit_tx_signatures` JSONB
--     arrays instead of single entry_tx/exit_tx TEXT columns; amount columns are
--     named *_token_amount; entry_price/entry_token_amount are nullable; status
--     includes 'PendingEntry'.
-- Data-only migrations (percent rescales, source relabels) leave no schema trace
-- and are intentionally omitted. Tables are ordered so foreign-key targets are
-- created before their referrers.
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
-- trades  (daily RANGE-partitioned on block_time, 1-month retention)
--   leg_index            — multiple pump trades in the same tx (multi-leg).
--   received_at          — ingest-time precision (Utc::now) for UI ordering;
--                          block_time stays chain seconds.
--   venue                — bonding-curve vs post-migration PumpSwap (AMM).
--
--   Partition-key constraints (Postgres): the partition column must be part of
--   any PRIMARY KEY / UNIQUE index on a partitioned table. So:
--     * PK is (block_time, id).
--     * The dedup unique key is (tx_signature, leg_index, block_time). block_time
--       is deterministic per tx (chain-assigned), so adding it changes nothing
--       about dedup exactness — re-delivery of the same (tx, leg) still collides.
--   All other indexes are non-unique and live on the parent (they cascade to
--   every partition, present and future). Retention is enforced by the app's
--   partition-maintenance task (KEEP_DAYS = 30, shared with raw_transactions)
--   via ensure_trades_partition / drop_trades_partition.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS trades (
    id                      UUID                NOT NULL DEFAULT uuid_generate_v4(),
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
                                CHECK (venue IN ('curve', 'amm')),
    PRIMARY KEY (block_time, id)
) PARTITION BY RANGE (block_time);

-- Dedup unique key (partition col included; deterministic per tx ⇒ still exact).
CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_tx_leg
    ON trades (tx_signature, leg_index, block_time);

-- Non-unique indexes on the parent cascade to all (incl. future) partitions.
-- Only indexes with confirmed pg_stat_user_indexes scans are kept (0004 dropped
-- the 5 zero-scan indexes: wallet, mint_venue_slot, wallet_mint, mint_slot_leg,
-- wallet_mint_sig — pure write amplification on the high-volume ingest path).
CREATE INDEX IF NOT EXISTS idx_trades_mint          ON trades(mint_address);
CREATE INDEX IF NOT EXISTS idx_trades_mint_time     ON trades(mint_address, block_time DESC);
-- BRIN on append-ordered block_time: equivalent range-scan support in a few KB
-- vs the btree's per-insert write amplification (0006).
CREATE INDEX IF NOT EXISTS idx_trades_block_time_brin ON trades USING BRIN (block_time);

-- Daily partition helpers (one calendar day each), analogous to the
-- raw_transactions fns.
CREATE OR REPLACE FUNCTION ensure_trades_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_end   date := v_start + 1;
    v_name  text := format('trades_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF trades FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamp AT TIME ZONE 'UTC', v_end::timestamp AT TIME ZONE 'UTC'
    );
END;
$$;

CREATE OR REPLACE FUNCTION drop_trades_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_name  text := format('trades_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- Pre-create the retained window (now-1mo .. now+2d) so the first inserts always
-- have a target partition before the maintenance task first runs.
DO $$
DECLARE d date := (now() - interval '1 month')::date;
BEGIN
    WHILE d <= (now() + interval '2 days')::date LOOP
        PERFORM ensure_trades_partition(d);
        d := d + 1;
    END LOOP;
END $$;

-- -------------------------------------------------------------------------
-- raw_transactions  (unified raw tx result — never mutated, replay/analysis source)
--   Single write-only table for both ingest provenances, tagged by `source`:
--     'live' — real-time LaserStream (Yellowstone) ingest pipeline
--     'sync' — token_sync backfill (both "Fetch All" via gTFA AND "Fetch New"
--              via LaserStream replay)
--   DAILY range partitions on received_at with 1-month retention (managed by
--   the app's partition-maintenance task; the retained window must match
--   ingest::maintenance::KEEP_DAYS = 30). No UNIQUE(signature): a unique
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
    source      TEXT        NOT NULL DEFAULT 'live',
    PRIMARY KEY (received_at, id)
) PARTITION BY RANGE (received_at);

-- Non-unique indexes on the parent cascade to all (incl. future) partitions.
CREATE INDEX IF NOT EXISTS idx_raw_tx_signature ON raw_transactions (signature);
CREATE INDEX IF NOT EXISTS idx_raw_tx_slot      ON raw_transactions (slot DESC);

-- Idempotently create the daily partition (one calendar day) containing p_anchor.
CREATE OR REPLACE FUNCTION ensure_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_end   date := v_start + 1;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF raw_transactions FOR VALUES FROM (%L) TO (%L)',
        v_name, v_start::timestamp AT TIME ZONE 'UTC', v_end::timestamp AT TIME ZONE 'UTC'
    );
END;
$$;

-- Drop the daily partition containing p_anchor, if it exists (retention).
CREATE OR REPLACE FUNCTION drop_raw_partition(p_anchor date)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_start date := p_anchor;
    v_name  text := format('raw_transactions_%s', to_char(v_start, 'YYYY_MM_DD'));
BEGIN
    EXECUTE format('DROP TABLE IF EXISTS %I', v_name);
END;
$$;

-- Pre-create the retained window (now-1mo .. now+2d) so the first inserts always
-- have a target partition before the maintenance task first runs.
DO $$
DECLARE d date := (now() - interval '1 month')::date;
BEGIN
    WHILE d <= (now() + interval '2 days')::date LOOP
        PERFORM ensure_raw_partition(d);
        d := d + 1;
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
--   lifetime_secs          — seconds from token creation to the last meaningful
--                            trade (sol_amount >= DEAD_MEANINGFUL_TRADE_SOL).
--                            Populated only when is_dead = true; NULL means alive
--                            or no meaningful trade yet.
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
    is_dead                BOOLEAN     NOT NULL DEFAULT FALSE,
    is_migrated            BOOLEAN     NOT NULL DEFAULT FALSE,
    last_synced_at         TIMESTAMPTZ,
    last_synced_curve_sig  TEXT,
    last_synced_amm_sig    TEXT,
    last_synced_curve_slot BIGINT,
    last_synced_amm_slot   BIGINT,
    lifetime_secs          BIGINT,
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
-- Analysis-freshness scans (newest-first by computed_at).
CREATE INDEX IF NOT EXISTS idx_analysis_computed_at ON tokens_analysis(computed_at DESC);

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
-- Creator suspiciousness ranking / threshold filtering.
CREATE INDEX IF NOT EXISTS idx_creator_profiles_suspiciousness
    ON creator_profiles(suspiciousness_score DESC);

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
--   status              — PendingEntry / Holding / ExitPending / End / ExitFailed.
--   exit_reason         — TakeProfit / StopLoss / TrailingStop / Stall / TimeStop /
--                         LiquidityExit (NULL until the position exits).
--   entry_price         — NULL until the actual buy fill lands (PendingEntry state).
--   entry_token_amount  — NULL until the actual buy fill lands.
--   entry_tx_signatures — JSONB array of entry fill tx signatures (single-leg = 1
--                         element; empty array = unentered / recovery fallback).
--   exit_tx_signatures  — JSONB array of exit tx signatures.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_real_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                    TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,
    entry_price             DOUBLE PRECISION,
    entry_token_amount      DOUBLE PRECISION,
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',
    exit_price              DOUBLE PRECISION,
    exit_token_amount       DOUBLE PRECISION,
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures       JSONB       NOT NULL DEFAULT '[]',
    submitted_buy_signatures TEXT[]      NOT NULL DEFAULT '{}',
    status                   TEXT        NOT NULL CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy                 TEXT        NOT NULL,
    rule_id                  UUID        NOT NULL,
    exit_reason              TEXT,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (rule_id) REFERENCES tpsl1_strategy_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_mint              ON tpsl1_real_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_wallet            ON tpsl1_real_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_status            ON tpsl1_real_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_strategy          ON tpsl1_real_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_rule_id           ON tpsl1_real_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_mint_status       ON tpsl1_real_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_token_program_id  ON tpsl1_real_positions(token_program_id);
-- Composite (filter, created_at DESC) for the list views (filter+order in one index).
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_strategy_created     ON tpsl1_real_positions(strategy, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_rule_created         ON tpsl1_real_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_mint_status_created  ON tpsl1_real_positions(mint, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl1_positions_wallet_status_created ON tpsl1_real_positions(wallet, status, created_at DESC);
-- Unique backstops on the first tx signature leg (skips empty-array / unentered rows).
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl1_real_positions_entry_sig0
    ON tpsl1_real_positions ((entry_tx_signatures->>0))
    WHERE jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl1_real_positions_exit_sig0
    ON tpsl1_real_positions ((exit_tx_signatures->>0))
    WHERE jsonb_array_length(exit_tx_signatures) > 0;
CREATE INDEX IF NOT EXISTS idx_tpsl1_real_positions_buy_submitted
    ON tpsl1_real_positions(updated_at) WHERE status = 'BuySubmitted';

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
-- Mirrors tpsl1_real_positions (minus uniqueness on signature arrays — a paper
-- position is seeded with the token's creation tx and the same token may be
-- traded again in a later run) plus a run_id binding each position to its run.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_paper_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id                  UUID        NOT NULL REFERENCES tpsl1_paper_test_run(id) ON DELETE CASCADE,
    mint                    TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,
    entry_price             DOUBLE PRECISION,
    entry_token_amount      DOUBLE PRECISION,
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',
    exit_price              DOUBLE PRECISION,
    exit_token_amount       DOUBLE PRECISION,
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures      JSONB       NOT NULL DEFAULT '[]',
    status                  TEXT        NOT NULL CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy                TEXT        NOT NULL,
    rule_id                 UUID        NOT NULL,
    exit_reason             TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
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
    p_entry_max_age_secs        BIGINT,
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
-- tpsl2_real_positions  (clone of tpsl1_real_positions + scalp-entry target_*)
--   target_* records the scalp-entry *trigger* trade (someone else's trade that
--   armed the entry), distinct from the actual entry fill in entry_*. All
--   nullable; target_tx is intentionally NOT unique (the trigger can repeat
--   across positions). Physical column order is target_*/entry_*/exit_* to match
--   the Rust struct, JSON response, and frontend table.
--   target_token_amount / entry_token_amount / exit_token_amount hold TOKEN counts
--   (not SOL); SOL cost is derived as price × token_amount at display time.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_real_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                    TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,
    target_price            DOUBLE PRECISION,
    target_token_amount     DOUBLE PRECISION,
    target_time             TIMESTAMPTZ,
    target_tx               TEXT,
    entry_price             DOUBLE PRECISION,
    entry_token_amount      DOUBLE PRECISION,
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',
    exit_price              DOUBLE PRECISION,
    exit_token_amount       DOUBLE PRECISION,
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures       JSONB       NOT NULL DEFAULT '[]',
    submitted_buy_signatures TEXT[]      NOT NULL DEFAULT '{}',
    status                   TEXT        NOT NULL CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy                 TEXT        NOT NULL,
    rule_id                  UUID        NOT NULL,
    exit_reason              TEXT,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),

    FOREIGN KEY (rule_id) REFERENCES tpsl2_strategy_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint              ON tpsl2_real_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_wallet            ON tpsl2_real_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_status            ON tpsl2_real_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_strategy          ON tpsl2_real_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_rule_id           ON tpsl2_real_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint_status       ON tpsl2_real_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_token_program_id  ON tpsl2_real_positions(token_program_id);
-- Composite (filter, created_at DESC) for the list views (filter+order in one index).
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_strategy_created     ON tpsl2_real_positions(strategy, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_rule_created         ON tpsl2_real_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_mint_status_created  ON tpsl2_real_positions(mint, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_wallet_status_created ON tpsl2_real_positions(wallet, status, created_at DESC);
-- Unique backstops on the first tx signature leg (skips empty-array / unentered rows).
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl2_real_positions_entry_sig0
    ON tpsl2_real_positions ((entry_tx_signatures->>0))
    WHERE jsonb_array_length(entry_tx_signatures) > 0;
CREATE UNIQUE INDEX IF NOT EXISTS uq_tpsl2_real_positions_exit_sig0
    ON tpsl2_real_positions ((exit_tx_signatures->>0))
    WHERE jsonb_array_length(exit_tx_signatures) > 0;
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_buy_submitted
    ON tpsl2_real_positions(updated_at) WHERE status = 'BuySubmitted';

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
-- tpsl2_paper_positions  (clone of tpsl1_paper_positions + scalp-entry target_*)
--   See tpsl2_real_positions for the target_* semantics and column ordering.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_paper_positions (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id                  UUID        NOT NULL REFERENCES tpsl2_paper_test_run(id) ON DELETE CASCADE,
    mint                    TEXT        NOT NULL,
    wallet                  TEXT        NOT NULL,
    token_program_id        TEXT,
    target_price            DOUBLE PRECISION,
    target_token_amount     DOUBLE PRECISION,
    target_time             TIMESTAMPTZ,
    target_tx               TEXT,
    entry_price             DOUBLE PRECISION,
    entry_token_amount      DOUBLE PRECISION,
    entry_time              TIMESTAMPTZ,
    entry_tx_signatures     JSONB       NOT NULL DEFAULT '[]',
    exit_price              DOUBLE PRECISION,
    exit_token_amount       DOUBLE PRECISION,
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures      JSONB       NOT NULL DEFAULT '[]',
    status                  TEXT        NOT NULL CHECK (status IN ('Arming', 'BuySubmitted', 'Holding', 'ExitPending', 'End', 'ExitFailed')),
    strategy                TEXT        NOT NULL,
    rule_id                 UUID        NOT NULL,
    exit_reason             TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_run         ON tpsl2_paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_mint_status ON tpsl2_paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_rule        ON tpsl2_paper_positions(rule_id);

-- =========================================================================
-- Grouped strategy param-sweeps (per-strategy tables).
--
-- The grouped sweep (`POST /api/strategies/sweeps`) selects tokens by a
-- `created_at` range, partitions them by an exact-value fingerprint key
-- (creator, CU settings, max_sol_cost, …), and runs the full combo sweep PER
-- group so the UI can answer "for tokens with THIS fingerprint, which param
-- combo is best?". One run → many groups → each group's ranked combo rows.
--
-- Tables are **separate per strategy**: the generic repo is table-name-driven
-- and the registry maps `strategy_id` → this triple. A new strategy (e.g. swing)
-- adds its own `swing_grouped_sweep_*` tables — no shared/generic table reused.
--
-- Per strategy there are four tables:
--   *_grouped_sweep_runs    — one row per sweep invocation
--   *_grouped_sweep_groups  — one row per surviving fingerprint group
--   *_grouped_sweep_combos  — one row per (run, combo_id): the deduped params dict
--   *_grouped_sweep_results — one row per (group, combo): metrics only (no params)
--
-- The params JSON that was formerly stored on every results row is now stored
-- once per (run, combo_id) in the combos table, saving `group_count`× redundancy.
-- Results use INTEGER counts and REAL floats (display-only precision, 4 B each).
-- =========================================================================

-- ---------------------------------------------------------------------------
-- TPSL2 grouped sweep tables
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,          -- 'tpsl2' (traceability)
    source           TEXT        NOT NULL,          -- 'db' (DB-range corpus)
    method           TEXT        NOT NULL,          -- 'grid' | 'random' | 'lhs'
    created_after    TIMESTAMPTZ,                   -- selection lower bound (incl.)
    created_before   TIMESTAMPTZ,                   -- selection upper bound (excl.)
    curve_only       BOOLEAN     NOT NULL DEFAULT FALSE,
    grouping_spec    JSONB       NOT NULL,          -- ["creator_wallet", …]
    axes_spec        JSONB       NOT NULL,          -- resolved param axes
    min_tokens       INTEGER     NOT NULL,          -- groups below this are dropped
    token_count      INTEGER     NOT NULL,          -- corpus size after selection
    group_count      INTEGER     NOT NULL,          -- surviving groups swept
    combo_count      INTEGER     NOT NULL,          -- combos per group
    buy_amount_sol   REAL,                         -- per-run notional (NULL = legacy default)
    corpus_hash      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Incremental persistence lifecycle.
    status           TEXT        NOT NULL DEFAULT 'completed'
                         CHECK (status IN ('running', 'completed', 'cancelled')),
    groups_done      INTEGER     NOT NULL DEFAULT 0,  -- bumped per append_group
    -- Full launch config for history / re-run.
    ix_labels_filter JSONB,                        -- exact ix-label set (NULL = none)
    field_filters    JSONB,                        -- per-field value filters (NULL = none)
    token_cap        INTEGER,                      -- per-run token cap (NULL = backend default)
    max_combos       INTEGER,                      -- per-group combo cap (NULL = backend default)
    label            TEXT                          -- user-given name (NULL = unnamed)
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_grouped_sweep_runs_created
    ON tpsl2_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_groups (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER     NOT NULL,      -- deterministic order (largest first)
    group_key           JSONB       NOT NULL,      -- {"creator_wallet":"…", …}; {} = ALL
    token_count         INTEGER     NOT NULL,
    fired_count         BIGINT      NOT NULL,      -- best combo's n_fired (sample size)
    best_combo_id       INTEGER     NOT NULL,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    -- Robust realized rank (μ−Z·σ/√n over closed trades, coverage-gated): the
    -- same metric the drill-in combo table sorts by. NULL when the winning combo
    -- has < 2 closed trades (no score).
    best_score          DOUBLE PRECISION,
    best_params         JSONB       NOT NULL,
    mints               TEXT[]                    -- token mints in this group (NULL = legacy)
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_grouped_sweep_groups_run
    ON tpsl2_grouped_sweep_groups(run_id, group_index);

-- Deduped params dictionary: one row per (run, combo_id) instead of repeating
-- params on every results row.
CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_results (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID        NOT NULL REFERENCES tpsl2_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER     NOT NULL,   -- join to tpsl2_grouped_sweep_combos for params

    -- counts (INTEGER: bounded by corpus size, well under 2.1 B)
    n_fired             INTEGER     NOT NULL,
    n_open              INTEGER     NOT NULL,
    n_closed            INTEGER     NOT NULL,

    -- profitability / success (REAL: display-only, ~7 sig digits is sufficient)
    win_rate            REAL        NOT NULL,
    total_pnl_sol       REAL        NOT NULL,
    mean_pnl_pct        REAL        NOT NULL,
    median_pnl_pct      REAL        NOT NULL,
    p90_pnl_pct         REAL        NOT NULL,
    best_pnl_pct        REAL        NOT NULL,
    worst_pnl_pct       REAL        NOT NULL,
    std_pnl_pct         REAL        NOT NULL DEFAULT 0,
    profit_factor       REAL,                  -- NULL = no losing trades (∞)
    score               REAL,                  -- robust rank μ−z·σ/√n; NULL = n_closed<2
    expectancy_sol      REAL        NOT NULL,
    avg_holding_secs    REAL        NOT NULL,
    median_holding_secs REAL        NOT NULL,

    -- exit-reason mix: per-reason trade counts (how closed trades terminated),
    -- NOT param values. `n_` marks them as counts, distinct from the
    -- exit_take_profit / exit_stop_loss *threshold* knobs carried in `params`.
    n_exit_take_profit  INTEGER     NOT NULL,
    n_exit_stop_loss    INTEGER     NOT NULL,
    n_exit_trailing     INTEGER     NOT NULL,
    n_exit_stall        INTEGER     NOT NULL,
    n_exit_time         INTEGER     NOT NULL,
    n_exit_liquidity    INTEGER     NOT NULL,
    n_exit_cohort       INTEGER     NOT NULL,
    n_exit_open         INTEGER     NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_grouped_sweep_results_group
    ON tpsl2_grouped_sweep_results(group_id, combo_id);

-- ---------------------------------------------------------------------------
-- TPSL1 grouped sweep tables
--
-- Same shape as TPSL2's quartet (the grouped-sweep repo is table-name-driven and
-- the registry maps `strategy_id` → this quartet), but separate per the
-- per-strategy-tables design. TPSL1 sweeps the exit ladder only (TP/SL plus the
-- optional trailing/time/stall/liquidity exits); it has no scalp entry gates and
-- no cohort-dump exit, so its `params` JSON carries fewer keys and the
-- `n_exit_cohort` count is always 0 — the column is kept for schema parity so the
-- generic repo can write every strategy's rows the same way.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,          -- 'tpsl1' (traceability)
    source           TEXT        NOT NULL,          -- 'db' (DB-range corpus)
    method           TEXT        NOT NULL,          -- 'grid' | 'random' | 'lhs'
    created_after    TIMESTAMPTZ,                   -- selection lower bound (incl.)
    created_before   TIMESTAMPTZ,                   -- selection upper bound (excl.)
    curve_only       BOOLEAN     NOT NULL DEFAULT FALSE,
    grouping_spec    JSONB       NOT NULL,          -- ["cu_price", …]
    axes_spec        JSONB       NOT NULL,          -- resolved param axes
    min_tokens       INTEGER     NOT NULL,          -- groups below this are dropped
    token_count      INTEGER     NOT NULL,          -- corpus size after selection
    group_count      INTEGER     NOT NULL,          -- surviving groups swept
    combo_count      INTEGER     NOT NULL,          -- combos per group
    buy_amount_sol   REAL,                         -- per-run notional (NULL = legacy default)
    corpus_hash      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Incremental persistence lifecycle.
    status           TEXT        NOT NULL DEFAULT 'completed'
                         CHECK (status IN ('running', 'completed', 'cancelled')),
    groups_done      INTEGER     NOT NULL DEFAULT 0,
    -- Full launch config for history / re-run.
    ix_labels_filter JSONB,
    field_filters    JSONB,
    token_cap        INTEGER,
    max_combos       INTEGER,
    label            TEXT
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_grouped_sweep_runs_created
    ON tpsl1_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_groups (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER     NOT NULL,      -- deterministic order (largest first)
    group_key           JSONB       NOT NULL,      -- {"cu_price":"…", …}; {} = ALL
    token_count         INTEGER     NOT NULL,
    fired_count         BIGINT      NOT NULL,      -- best combo's n_fired (sample size)
    best_combo_id       INTEGER     NOT NULL,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    -- Robust realized rank (μ−Z·σ/√n over closed trades, coverage-gated): the
    -- same metric the drill-in combo table sorts by. NULL when the winning combo
    -- has < 2 closed trades (no score).
    best_score          DOUBLE PRECISION,
    best_params         JSONB       NOT NULL,
    mints               TEXT[]                    -- token mints in this group (NULL = legacy)
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_grouped_sweep_groups_run
    ON tpsl1_grouped_sweep_groups(run_id, group_index);

-- Deduped params dictionary: one row per (run, combo_id).
CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_results (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER     NOT NULL,   -- join to tpsl1_grouped_sweep_combos for params

    -- counts
    n_fired             INTEGER     NOT NULL,
    n_open              INTEGER     NOT NULL,
    n_closed            INTEGER     NOT NULL,

    -- profitability / success
    win_rate            REAL        NOT NULL,
    total_pnl_sol       REAL        NOT NULL,
    mean_pnl_pct        REAL        NOT NULL,
    median_pnl_pct      REAL        NOT NULL,
    p90_pnl_pct         REAL        NOT NULL,
    best_pnl_pct        REAL        NOT NULL,
    worst_pnl_pct       REAL        NOT NULL,
    std_pnl_pct         REAL        NOT NULL DEFAULT 0,
    profit_factor       REAL,
    score               REAL,
    expectancy_sol      REAL        NOT NULL,
    avg_holding_secs    REAL        NOT NULL,
    median_holding_secs REAL        NOT NULL,

    -- exit-reason mix: per-reason trade counts (how closed trades terminated).
    -- `n_exit_cohort` stays 0 for TPSL1 (no cohort-dump exit) — kept for schema
    -- parity with the generic repo's INSERT.
    n_exit_take_profit  INTEGER     NOT NULL,
    n_exit_stop_loss    INTEGER     NOT NULL,
    n_exit_trailing     INTEGER     NOT NULL,
    n_exit_stall        INTEGER     NOT NULL,
    n_exit_time         INTEGER     NOT NULL,
    n_exit_liquidity    INTEGER     NOT NULL,
    n_exit_cohort       INTEGER     NOT NULL,
    n_exit_open         INTEGER     NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_grouped_sweep_results_group
    ON tpsl1_grouped_sweep_results(group_id, combo_id);

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
-- app_settings  (typed key-value store)
-- Global, server-wide settings: one row per setting (dotted, namespaced key →
-- JSONB value), giving per-key atomic upserts (no whole-document read-modify-
-- write) and per-key audit via updated_at. The key set + defaults are owned by
-- the application's `AppSettings` struct (settings_repo.rs `keys`); absent keys
-- fall back to per-setting defaults at load time, so adding a setting is a struct
-- field, never a migration.
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
