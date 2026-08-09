-- ============================================================================
-- Consolidated core schema (single-file init). TimescaleDB, local-first.
--
-- This is the squash of the entire core migration chain into one end-state file —
-- it creates the schema exactly as running the full chain on a fresh database
-- would leave it (all renames/adds/drops already folded in). Two squash
-- generations are absorbed:
--
--   * the legacy 0001..0010 chain (already folded into the previous init):
--       - 0001 init                    base tables, hypertables, views, seeds
--       - 0002 token_account           strategy_positions.token_account
--       - 0003 venue-neutral reserves  reserve_sol/reserve_token
--       - 0004 creation_slot/first-slot tokens.creation_slot + first-slot lamports
--       - 0005 token-list pagination   tokens/tokens_info sort indexes
--       - 0007 drop cohort metric      strategy_run_metrics.n_exit_cohort removed
--       - 0009 SOL/lamports naming     *_sol/*_lamports unit-in-name renames
--       + 0002_strategy_positions_mint_address (mint -> mint_address)
--
--   * the 0002..0021 chain layered on top of that init:
--       0002 trades_ix_labels                trades.ix_labels
--       0003 part1_realmoney                 tokens.total_supply_token + market-cap basis
--       0004 strategy_redesign               fingerprints + generic strategy_rules
--       0005 retire_legacy_strategies        strategy_rules_legacy dropped
--       0006 fingerprint_metric_config       fingerprints.metric_config
--       0008 rule_enabled                    strategy_rules.is_enabled
--       0009 tokens_info_write_amplification 7 secondary indexes dropped + fillfactor
--       0011 amm_pool_facts                  durable PumpSwap pool layout
--       0012 exit_redrive_park               exit_redrive_count / exit_parked
--       0013 status_split_manual_origin      status domain + origin / manual_exit
--       0014 + 0020 + 0021 fingerprint bucket width: nullable + range CHECK
--       0017 position_last_entry_error       strategy_positions.last_entry_error
--       0018 position_fills_scale_out        position_fills + scale-out aggregates
--
--   * the 0002..0006 chain layered on top of THAT init:
--       0002 rule_tags                       strategy_rules.tags
--       0004 run_lifecycle                   strategy_run_metrics n_exit_dead /
--                                            _metrics / _manual / _migrated
--       0005 trade_fee                       trades.fee_lamports
--       0006 pnl_pct_is_capital_return       strategy_position_pnl.pnl_pct becomes
--                                            money-over-capital, not a price ratio
--
-- The pure data-backfill migrations only rewrote pre-existing rows — no-ops on a
-- fresh database — so they are intentionally NOT reproduced here:
--   legacy 0006/0008/0010 (JSONB key + paper-entry backfills), 0007 (bucket-width
--   f32 tidy), 0010 (metric-group renames), 0015 (empty ix_labels -> NULL),
--   0016 (slippage settings reset), 0019 (exit_price unit fix), and 0003
--   (position_fills backfill for pre-ledger positions — reconstructed one buy/sell
--   from the `strategy_positions` snapshot so the chart could mark positions whose
--   `…/fills` returned []; a fresh DB writes the ledger from the first fill).
--
-- Naming rule (locked): every column denoting a SOL amount names its unit —
-- `_lamports` = exact BIGINT, `_sol` = human f64. Ratios keep `_price`/`_pct`.
--
-- TimescaleDB note: hypertable creation + compression/retention policies are
-- transaction-safe and live here. Continuous aggregates (trades_ohlcv_*) CANNOT
-- be created inside a transaction, and sqlx 0.6 wraps every migration in one — so
-- the CAggs are created idempotently at boot by `storage::timescale::setup_caggs`.
--
-- NOTE (ledger): collapsing the chain changes this file's checksum and removes
-- versions 2..6 from `_sqlx_migrations`. An already-migrated database must be
-- reconciled once with `scripts/consolidate-migration-ledgers.ps1` before a bin
-- that embeds this file will boot against it.
--
-- ⚠ PRECONDITION, and it is the whole reason a squash is dangerous: reconciling
-- rewrites the LEDGER and never the SCHEMA. It stamps "version 1 applied" on a
-- database on the assumption that everything this file creates is already there.
-- Any folded-in migration that had NOT yet run on that database therefore never
-- runs at all — the column silently stays missing and the bin fails at query time,
-- not at boot. So before reconciling ANY database, bring its schema up to this
-- file's end state with `scripts/squash-catchup.sql` (idempotent, safe to re-run,
-- safe on a DB that is already current). Order across boxes is EC2 first, because
-- `hunter/scripts/db-incremental-sync.ps1` copies the server's `_sqlx_migrations`
-- rows into the local mirror and would re-insert versions you just cleaned.
--
-- ONE benign divergence from an incrementally-migrated database, verified by
-- diffing every column / constraint / index of both: `trades_priced` here expands
-- `t.*` AFTER `trades.ix_labels` exists, so a fresh DB's view carries that column
-- while an existing DB's does not (its view was frozen at creation, before 0002
-- added the column, and nothing re-created it). No reader selects it — the view has
-- no callers in Rust or TS — so the two are interchangeable; re-running the
-- DROP+CREATE below against an old database converges them if that ever changes.
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
    -- H1 (0003): market cap (FDV) is `current_price × TOTAL supply`, not
    -- `× initial_supply_token` (the dev's first-buy amount). pump.fun mints have a
    -- fixed total supply: 1B tokens (1e15 raw units @ 6 decimals), 2B for
    -- `create_v2` "mayhem" mints. Stored so no SQL site hardcodes the constant;
    -- populated at ingest from `config::constants::token_math::total_supply_for`.
    total_supply_token      BIGINT,
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
--
-- NO SECONDARY INDEXES, deliberately (0009). `tokens_info` is upserted once per
-- distinct mint on every ingest flush (~150 ms) — 563k UPDATEs in a 21 h window on
-- the live box. Each secondary index on a changing column forces the UPDATE off the
-- HOT path (a HOT tuple can only be created when NO indexed column changes), so
-- every one multiplied the write + WAL + autovacuum cost of the hottest write in the
-- system: HOT-update ratio 0.17%, 1216 autovacuums, ~8x write amplification — the
-- chronic I/O pressure that let a single `pool.acquire()` stall snowball into the
-- 2 h ingest freeze. The seven dropped indexes all had idx_scan = 0 over that same
-- window (only the PK was used, 1.47M scans): the token list pages/sorts the full
-- universe and the planner chose a seqscan+sort regardless.
--
-- REVERSAL. If a token-list sort ever regresses (a single-column ORDER BY the
-- planner *would* use), recreate the exact index and re-check
-- `pg_stat_user_indexes.idx_scan` after real traffic — do not restore blind:
--
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_is_dead       ON tokens_info(is_dead);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_volume_sol    ON tokens_info(volume_sol DESC);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_last_trade_at ON tokens_info(last_trade_at DESC);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_trade_count   ON tokens_info(trade_count   DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_current_price ON tokens_info(current_price DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_price     ON tokens_info(ath_price     DESC NULLS LAST);
--   CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_timestamp ON tokens_info(ath_timestamp DESC NULLS LAST);
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

-- fillfactor=80 (0009 C2) — the complement to having no secondary indexes. HOT
-- updates also need free space on the row's own page; reserving ~20% per page gives
-- the upsert room to stay heap-only instead of spilling to a new page. Applies to
-- pages written from here on; existing pages adopt it as they are rewritten. To
-- apply immediately on an existing DB, run once, MANUALLY (VACUUM FULL cannot run
-- inside this migration's transaction; trivial on this ~5 MB table):
--
--   VACUUM FULL tokens_info;
--
-- Infra note (cannot be set from a migration): `autovacuum_max_workers` defaults to
-- 10 on the 2-vCPU / 4 GB box. Cap it at 3 in postgresql.conf (needs a restart) so a
-- vacuum burst can't put 10 workers — up to ~2.5 GB of maintenance_work_mem —
-- against 2 cores and one disk.
ALTER TABLE tokens_info SET (fillfactor = 80);

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
    (i.current_price * t.total_supply_token)           AS market_cap
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
-- Shortest window in the system, and deliberately so: measured 2026-08-09 on the
-- live box, `raw_txs` was 21.5 GB against `trades`' 13 GB — 58% of the whole
-- database for a feed nothing routinely reads. Compression barely helps here
-- (4.5 GB -> 3.8 GB, ~18%) because `payload` is one opaque BYTEA that TOASTs and
-- is already row-compressed before columnar compression sees it; there is no
-- repeated value to segment on. Contrast `trades`, which compresses ~84%.
--
-- Both policies compare against a chunk's `range_end`, and chunks are 1 day wide,
-- so `compress_after => 1 day` still leaves ~2 uncompressed days on disk — the
-- effective lever is `drop_after`. 3 days holds the feed long enough to re-decode
-- a recent tx by hand while capping the table near ~12 GB instead of 21.5 GB.
--
-- Safe to keep this short because `raw_txs` is opt-in on the sync path
-- (`db-incremental-sync.ps1 -IncludeRawTxs`, OFF by default), so it is not the
-- workstation's source for anything, and `persist_raw` (an `app_settings` bool)
-- can switch the writes off entirely. Anything that must outlive 3 days has to be
-- denormalized onto `trades` at decode time — which is exactly why
-- `trades.ix_labels` and `trades.fee_lamports` are columns rather than derived.
SELECT add_compression_policy('raw_txs', compress_after => INTERVAL '1 day', if_not_exists => TRUE);
SELECT add_retention_policy('raw_txs', drop_after => INTERVAL '3 days', if_not_exists => TRUE);

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

    -- Per-leg instruction labels (0002). Already computed at ingest (it rides on
    -- the Trade event and populates `tokens.ix_labels` for the create tx), written
    -- straight from the in-hand value: deployments that DON'T persist `raw_txs` (to
    -- save disk) have no payload to re-derive it from. Cheap despite per-leg
    -- storage — label arrays are low-distinct-cardinality (`["buy"]`,
    -- `["create","buy",...]` repeat endlessly), so columnar compression crushes
    -- them to ~free. JSONB (not TEXT[]) so it matches `tokens.ix_labels`
    -- byte-for-byte and the ix_count / ix_labels filter helpers in tokens/sql.rs
    -- work verbatim on trades.
    ix_labels              JSONB,

    -- Per-TRANSACTION network fee (0005): base signature fee + priority fee, read
    -- from `TransactionStatusMeta.fee` on the LaserStream feed we already consume
    -- (no extra RPC, no Helius credits). NOT the Jito tip (a transfer instruction,
    -- absent from meta.fee) and NOT the venue's protocol/LP fee (already inside
    -- amount_lamports).
    --
    -- ATTRIBUTION — read before writing any aggregate. The fee is charged ONCE PER
    -- TX but this table is keyed per LEG, and one tx can emit many legs, so the
    -- value is denormalized onto every leg of its tx. Correct for per-row display;
    -- a straight `SUM(fee_lamports)` OVER-COUNTS by the leg multiplier. Collapse by
    -- signature first:
    --   SELECT SUM(fee_lamports)
    --   FROM (SELECT DISTINCT tx_signature, fee_lamports FROM trades WHERE …) s
    -- Denormalizing beats a per-signature side table, which would add a write per
    -- tx to the hot ingest path — the write-amplification shape that froze ingest
    -- before. This costs one more bind on an insert that already runs.
    --
    -- NULLABLE, and NULL is load-bearing: rows written before 0005 have no fee and
    -- can never get one (`raw_txs` is opt-in and 3-day retention regardless, so
    -- there is nothing to re-decode). NULL means "not captured" — never coalesce it
    -- to 0 for display, never sum it as 0 in an average. A landed transaction always
    -- pays at least the 5000-lamport base fee, so a genuine 0 does not exist; ingest
    -- folds any zero back to NULL at the source (`ingest_core::event::fee_lamports_opt`).
    fee_lamports           BIGINT,

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
-- fingerprints (0004) — a token-creation shape, shared by many rules.
--   Exact-match fields: cu_limit, cu_price, ix_labels (ordered). Bucket-matched
--   fields (via this row's own bucket_size_amount, SSOT grouping::same_bucket):
--   the five lamports axes. NULL = the field is not part of the fingerprint's
--   identity.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS fingerprints (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                     TEXT NOT NULL,
    cu_limit                 BIGINT,            -- exact match
    cu_price                 BIGINT,            -- exact match
    init_buy_lamports        BIGINT,            -- bucket-matched ┐
    max_cost_lamports        BIGINT,            --                │ all via this row's
    spendable_lamports_in    BIGINT,            --                │ bucket_size_amount
    first_slot_buy_lamports  BIGINT,            -- sum in slot    │
    first_slot_sell_lamports BIGINT,            -- sum in slot    ┘
    -- SOL bucket width, or NULL for exact-lamports matching (0020 + 0021). The
    -- DEFAULT is the storage-side mirror of `Fingerprint::from_json`'s rule that an
    -- ABSENT value means 0.1 (only an explicit `null` opts into exact mode), so a
    -- writer that omits the column can't silently land in exact mode.
    bucket_size_amount       DOUBLE PRECISION DEFAULT 0.1,
    ix_labels                TEXT[],            -- exact ordered sequence
    -- Per-fingerprint metric-group config, NOT identity (0006). Top-level keys are
    -- metric group names; `m_flow_split.volume_ix_patterns` is the first consumer.
    -- `find_or_create` must NOT match on this column — patterns are configuration.
    metric_config            JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 0014 + 0020: the width must be a real, positive SOL width — or NULL.
--
-- Why this is a correctness constraint, not tidying: the engine matcher
-- (`hunter_engine::fingerprint::matches_phase`) divides by this width RAW via
-- `grouping::bucket_index` = `floor(v / width + 1e-9)`. At width 0 every positive
-- amount divides to +inf and saturates to the same `i64::MAX` bucket index, so a
-- configured SOL axis stops discriminating entirely and the fingerprint arms on ANY
-- non-zero value — the match-everything hazard the `has_any_criterion` guard exists
-- to prevent, on the live arming path. `0` is also a second sentinel in a field that
-- already caused one live bug (two readers disagreeing about whether it meant
-- "default 0.1" or "literally zero"), so NULL is the ONE spelling of "not bucketed".
--
-- 1e-6 SOL = MIN_BUCKET_WIDTH_SOL (hunter/engine/src/grouping.rs): below it the
-- 1e-9 ratio-epsilon in `bucket_index` stops being negligible. The 1e6 ceiling
-- excludes NaN and Infinity, which DOUBLE PRECISION accepts and which Postgres
-- orders ABOVE every finite value (so a `>= 1e-6` bound alone lets both through).
--
-- Note the asymmetry with the SOL AXES on the same row: there, 0 is a perfectly
-- valid value (`spendable_lamports_in = 0` at width 1 means the bucket [0, 1)), and
-- NULL is the only way to say "axis not part of identity". Those columns stay
-- nullable with no CHECK.
ALTER TABLE fingerprints DROP CONSTRAINT IF EXISTS fingerprints_bucket_size_amount_positive;
ALTER TABLE fingerprints
    ADD CONSTRAINT fingerprints_bucket_size_amount_positive
    CHECK (
        bucket_size_amount IS NULL
        OR (bucket_size_amount >= 1e-6 AND bucket_size_amount <= 1e6)
    );

COMMENT ON COLUMN fingerprints.bucket_size_amount IS
    'SOL bucket width for every SOL match axis, or NULL for exact-lamports matching. '
    'Never 0 (division by zero in bucket_index, and a second sentinel). '
    'Read via hunter_engine::fingerprint::Fingerprint::precision().';

-- ===========================================================================
-- strategy_rules (0004) — the generic fingerprint+metrics engine's rule table.
--   Columns say HOW the rule trades; params JSONB says WHEN (strict
--   take_profit/stop_loss + entry/exit metric-condition groups with
--   {operator, value} lists). The named tpsl1/tpsl2/swing1 stack this replaced was
--   retired in Phase 7 (its `strategy_rules_legacy` table is gone, 0005).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_rules (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_name             TEXT NOT NULL,
    fingerprint_id        UUID NOT NULL REFERENCES fingerprints(id),
    trade_mode            TEXT NOT NULL CHECK (trade_mode IN ('paper','real')),
    is_active             BOOLEAN NOT NULL DEFAULT false,
    -- Soft-archive flag (0008), orthogonal to `is_active` (Active/Idle live
    -- arming): a disabled rule stays in the DB for reference but is hidden from
    -- default UI lists and cannot be activated until re-enabled.
    is_enabled            BOOLEAN NOT NULL DEFAULT true,
    buy_amount_lamports   BIGINT NOT NULL,
    max_concurrent_tokens BIGINT NOT NULL DEFAULT 1,
    max_total_tokens      BIGINT NOT NULL DEFAULT 0,      -- 0 = unlimited
    params                JSONB NOT NULL,
    -- Free-form presentational labels (0002) for slicing the Rules board (show only
    -- `fam:scalper`, hide `stage:experiment`). A typed column and deliberately NOT a
    -- `params` key: `params` is re-serialized from `RuleParams` on every write (an
    -- unknown key is dropped on the first save), it IS trading identity
    -- (`RuleRepo::find_identical` compares it, so a tag there would weaken the
    -- Duplicate gate), and it is frozen into `strategy_runs.params_snapshot` (so a
    -- tag rename would read as a strategy change in run history). Tags sit in the
    -- same bucket as `rule_name`: a label, never identity, never an input to the
    -- decision kernel — `list_active` is untouched.
    --
    -- Array rather than a `rule_tags` join table: the rules list is fetched whole and
    -- filtered client-side, so there is no server-side tag query to normalize for, and
    -- the catalog is derivable (`SELECT DISTINCT unnest(tags) FROM strategy_rules`).
    -- No GIN index for the same reason — add one only alongside a real `WHERE tags && $1`.
    -- Canonical shape (lowercase, `-` separated, deduped, sorted, no `,`) is enforced
    -- in ONE place, `trading_core::strategies::rules::normalize_tags`; the DB only
    -- guarantees non-NULL. Rationale: docs/plans/strategies/rule-tags.md
    tags                  TEXT[] NOT NULL DEFAULT '{}'::text[],
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_rules_active      ON strategy_rules (is_active, trade_mode);
CREATE INDEX IF NOT EXISTS idx_strategy_rules_fingerprint ON strategy_rules (fingerprint_id);
CREATE INDEX IF NOT EXISTS idx_strategy_rules_enabled     ON strategy_rules (is_enabled);

-- ===========================================================================
-- strategy_runs — one activation session of a rule (paper or real).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS strategy_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    -- NO FK, deliberately: the original FK pointed at the pre-redesign rule table
    -- and was dropped with it (0004 rename + 0005 `DROP TABLE ... CASCADE`).
    -- Deleting a rule must not rewrite or block its run history.
    rule_id          UUID,
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

    -- Exit buckets the generic engine actually produces (0004). The set above was
    -- shaped for the retired tpsl/swing ladder, so `ExitReason::Metrics` — where
    -- EVERY generic-engine exit lands — the analysis-only death-close, and the
    -- operator/migration closes had no column: the persisted histogram read
    -- all-zero beside a non-zero n_closed. These are only reachable now that a run
    -- ends when its rule stops being active (`set_run_status` had no caller before
    -- 0004, so `Sink::ensure_run` reused run #1 forever and metrics were never
    -- rolled up). Runs left 'Running' by that old behavior are NOT healed at
    -- migration time — the engine finalizes them at boot via
    -- `StrategyRepo::orphan_running_runs`, which also keeps healing later drift
    -- (a db-sync from another box, a hand-edited `is_active`).
    n_exit_dead         INTEGER     NOT NULL DEFAULT 0,
    n_exit_metrics      INTEGER     NOT NULL DEFAULT 0,
    n_exit_manual       INTEGER     NOT NULL DEFAULT 0,
    n_exit_migrated     INTEGER     NOT NULL DEFAULT 0,

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
    -- Constant sentinel 'generic' under the redesigned engine; kept because it is
    -- woven through the hot StrategyPosition/StrategyRun models and several repo
    -- queries (dropping it is a model refactor, not a schema tidy — see 0005).
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

    -- exit fill. On `End` these still stamp a SOL-weighted average across every
    -- sell leg, so CLOSED_PRED / realized PnL keep working without a JOIN.
    exit_price              DOUBLE PRECISION,
    exit_token_amount       BIGINT,                 -- raw token units
    exit_lamports           BIGINT,                 -- lamports
    exit_time               TIMESTAMPTZ,
    exit_tx_signatures      JSONB       NOT NULL DEFAULT '[]',

    -- Scale-out running aggregates (0018) — caches of the position_fills ledger, so
    -- list/PnL queries stay JOIN-free.
    sold_token_amount       BIGINT      NOT NULL DEFAULT 0,
    exit_sol_lamports_total BIGINT      NOT NULL DEFAULT 0,
    scale_stage             SMALLINT    NOT NULL DEFAULT 0,

    submitted_buy_signatures TEXT[]     NOT NULL DEFAULT '{}',
    -- Status domain per 0013: `ExitFailed` was split into EntryFailed (the buy never
    -- filled — terminal, no SOL deployed, excluded from realized PnL) and ExitStuck
    -- (the sell gave up, the bag is still held — an OPEN problem, reaper redrive +
    -- park still apply). `Arming` was vestigial (the sink always overwrote it before
    -- the first insert) and is gone.
    status                  TEXT        NOT NULL,
    exit_reason             TEXT,
    -- Manual-position support (0013): `origin` marks manual buys as first-class
    -- positions, `manual_exit` holds their optional TP/SL config
    -- ({"tp_pct": .., "sl_pct": ..}; NULL = tracked-only, no auto-exit).
    origin                  TEXT        NOT NULL DEFAULT 'bot',
    manual_exit             JSONB,
    -- Reaper redrive bound (0012). The reaper re-attempts every real ExitStuck
    -- position that still has a bag; on a dead/rugged pool every retry reverts and
    -- burns tip+fees, so it retries a bounded number of times then PARKS (stops
    -- auto-retrying, surfaces for a manual decision) — never auto-writes-off.
    exit_redrive_count      INTEGER     NOT NULL DEFAULT 0,
    exit_parked             BOOLEAN     NOT NULL DEFAULT false,
    -- Cause of the most recent buy attempt that did not fill (0017).
    last_entry_error        TEXT,
    extra                   JSONB       NOT NULL DEFAULT '{}',

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_status_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_status_check
    CHECK (status IN ('BuySubmitted','Holding','ExitPending',
                      'ExitUnconfirmed','ExitStuck','End','EntryFailed'));

ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_origin_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_origin_check CHECK (origin IN ('bot','manual'));

-- 0012/0017 note: exit_redrive_count / exit_parked / last_entry_error are
-- deliberately NOT in `StrategyRepo::update_position`'s fixed column list. The
-- executor writes them at the moment of failure and the engine sink's full-row
-- write of the terminal status lands afterwards, so a shared write path would
-- clobber them (a jsonb `extra` key would be overwritten the same way).
COMMENT ON COLUMN strategy_positions.last_entry_error IS
    'Cause of the most recent buy attempt that did not fill (send error or Anchor '
    'custom code). Written only by note_last_entry_error; never cleared on success.';
COMMENT ON COLUMN strategy_positions.sold_token_amount IS
    'Running sum of confirmed sell-leg raw token units (cache of position_fills).';
COMMENT ON COLUMN strategy_positions.exit_sol_lamports_total IS
    'Running sum of confirmed sell-leg SOL in lamports (cache of position_fills).';
COMMENT ON COLUMN strategy_positions.scale_stage IS
    'Next scale-out stage index (0 = pre-first partial / legacy full-bag).';

CREATE INDEX IF NOT EXISTS idx_strategy_positions_run            ON strategy_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_strategy_created
    ON strategy_positions(strategy_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_rule_created   ON strategy_positions(rule_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_mint_address_status ON strategy_positions(mint_address, status);
CREATE INDEX IF NOT EXISTS idx_strategy_positions_token_program  ON strategy_positions(token_program_id);

-- In-flight recovery sweep (real mode).
CREATE INDEX IF NOT EXISTS idx_strategy_positions_buy_submitted
    ON strategy_positions(updated_at) WHERE status = 'BuySubmitted';

-- Double-buy safety: unique on the first entry tx leg, REAL MODE ONLY. There is no
-- exit-side twin — with N sell legs a later leg's sig could collide with another
-- position's first, so exit uniqueness moved to `position_fills.tx_signature` (0018).
CREATE UNIQUE INDEX IF NOT EXISTS uq_strategy_positions_entry_sig0
    ON strategy_positions ((entry_tx_signatures->>0))
    WHERE mode = 'real' AND jsonb_array_length(entry_tx_signatures) > 0;

-- ===========================================================================
-- position_fills (0018) — append-only per-leg ledger; the durable truth behind
--   tranched scale-out (N sell legs inside one episode).
-- ===========================================================================
CREATE TABLE IF NOT EXISTS position_fills (
    position_id   UUID        NOT NULL REFERENCES strategy_positions(id) ON DELETE CASCADE,
    seq           INTEGER     NOT NULL,
    side          TEXT        NOT NULL CHECK (side IN ('buy', 'sell')),
    price         DOUBLE PRECISION NOT NULL,
    sol_lamports  BIGINT      NOT NULL,
    token_amount  BIGINT      NOT NULL,
    at            TIMESTAMPTZ NOT NULL,
    reason        TEXT,
    stage         SMALLINT,
    tx_signature  TEXT,
    PRIMARY KEY (position_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_position_fills_position
    ON position_fills (position_id, seq);

-- Real (and any non-empty) sell sigs must be unique across positions.
-- Empty/NULL sigs (paper) are excluded.
CREATE UNIQUE INDEX IF NOT EXISTS uq_position_fills_sell_tx
    ON position_fills (tx_signature)
    WHERE side = 'sell' AND tx_signature IS NOT NULL AND tx_signature <> '';

-- Derived per-position PnL (never stored).
--
-- `pnl_pct` is a MONEY return, not a PRICE return (0006):
--   realized_pnl_lamports / entry_lamports * 100
-- and NOT the old (exit_price - entry_price) / entry_price * 100. Two independent
-- defects made that price ratio disagree with the ◎ on the same row:
--
--   1. NO EXECUTION COST. A price ratio charges nothing, but a round trip pays the
--      venue fee on BOTH legs (125 bps/leg, measured 2026-07-28) plus a fixed tip
--      per leg plus our own price impact — roughly a +4% move just to break even.
--      Every trade between 0% and break-even rendered a GREEN percent beside a RED
--      SOL figure. `realized_pnl_sol` is measured from the lamports that actually
--      moved, so it already carries all of those costs.
--   2. SCALE-OUT BLINDNESS. `exit_price` stamps the LAST sell leg only, while the
--      SOL figure sums every leg via `exit_sol_lamports_total`. On a laddered exit
--      the two headline numbers described different trades. Both now read the same
--      CASE.
--
-- Because the denominator is capital (always > 0), the sign of `pnl_pct` can no
-- longer disagree with the sign of `realized_pnl_sol` — the guarantee
-- `hunter_core::strategies::kernel::weighted_return_pct` gives the aggregate
-- surfaces, now at per-position grain.
--
-- SSOT: this view, `StrategyPosition::pnl_pct` (models/strategy.rs), and
-- `PNL_PCT_SQL` (the positions table's sort/filter expression in strategy_repo.rs)
-- are three spellings of ONE formula. Change one, change all three;
-- `pnl_sql_columns_share_one_numerator` guards the two SQL copies.
--
-- Derived, so there is no data to rewrite: every historical position's percent is
-- recomputed on read, and already-closed rows show a SMALLER (for sub-break-even
-- winners, NEGATIVE) percent than they did before — that is the correction, not a
-- regression. `strategy_run_metrics` is untouched: those columns were stamped at
-- rollup time and are not recomputable from a view, so a run finalized before 0006
-- keeps its price-based mean and later runs get the money-based one.
DROP VIEW IF EXISTS strategy_position_pnl;
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    -- exit_lamports/entry_lamports are lamports (BIGINT); divide back to human SOL
    -- so realized_pnl_sol matches StrategyPosition::realized_pnl_sol() (f64 SOL).
    -- Prefer the running aggregate once any sell leg has landed (scale-out or
    -- single-leg); legacy End rows keep sold_token_amount = 0 and use exit_lamports.
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8 / 1e9) AS realized_pnl_sol,
    -- Same numerator as realized_pnl_sol, over the capital deployed. NULL whenever
    -- the chosen exit column or entry_lamports is absent, so an open position still
    -- has no percent.
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8
     / NULLIF(p.entry_lamports, 0) * 100.0)  AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END     AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                     AS is_closed
FROM strategy_positions p;

-- ===========================================================================
-- amm_pool_facts (0011) — durable PumpSwap pool-layout facts per migrated token.
--
-- The executor caches the AMM pool layout (`needs_pool_v2`, the fee-share marker,
-- the creator vault pair, cashback flag) in an in-memory map with no TTL, warmed for
-- FREE by the live feed harvest (`observe_amm_swap_accounts`, zero RPC). A restart
-- wipes that map, and a token whose pool has since gone dead never re-harvests — its
-- sell then falls to the cold RPC path, which could not reconstruct the swap tail
-- once the pool's recent signatures were all failed exit attempts, stranding the
-- position. This table persists the harvested facts across restarts: `live` upserts
-- them from a background loop as the trader learns them, and re-seeds the trader
-- cache for held migrated mints on boot — both with NO RPC.
--
-- Written only when a NEW pool is first observed (not on the ~150 ms ingest flush),
-- so it carries none of the tokens_info write-amplification concerns; the PK is the
-- only access path (point read + `= ANY($1)` batch seed), so no secondary indexes.
-- All pubkeys are base58 TEXT — mirroring the executor's transport DTO
-- (`pump_trader::AmmPoolFacts`), its own decoupled vocab; the mint PK follows the
-- hunter SSOT key name `mint_address`.
-- ===========================================================================
CREATE TABLE IF NOT EXISTS amm_pool_facts (
    mint_address                 TEXT PRIMARY KEY,
    pool                         TEXT        NOT NULL,
    base_mint                    TEXT        NOT NULL,
    quote_mint                   TEXT        NOT NULL,
    base_token_program           TEXT        NOT NULL,
    pool_base_token_account      TEXT        NOT NULL,
    pool_quote_token_account     TEXT        NOT NULL,
    coin_creator                 TEXT        NOT NULL,
    coin_creator_vault_ata       TEXT        NOT NULL,
    coin_creator_vault_authority TEXT        NOT NULL,
    is_cashback_coin             BOOLEAN     NOT NULL,
    fee_share_marker             TEXT,          -- NULL for cashback / pool_v2 pools
    needs_pool_v2                BOOLEAN     NOT NULL,
    updated_at                   TIMESTAMPTZ NOT NULL DEFAULT now()
);

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
-- NOTE (0016): the slippage keys are deliberately NOT seeded. `trade.slippage_bps`
-- (the legacy combined key) is retired outright, and absent
-- `trade.buy_slippage_bps` / `trade.sell_slippage_bps` restore the documented
-- per-side defaults (buy = DEFAULT_SLIPPAGE_BPS, sell = no floor / sell all).
CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
