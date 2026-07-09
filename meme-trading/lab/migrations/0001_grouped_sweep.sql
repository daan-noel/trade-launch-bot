-- ============================================================================
-- Consolidated lab-only grouped param-sweep storage (single-file init).
--
-- Squash of the entire lab migration chain into one end-state file — it creates
-- the schema exactly as running the full chain on a fresh database would leave
-- it. It absorbs the former on-disk lab files:
--   * 0001_grouped_sweep  — itself the squash of the legacy 0001..0003 lab chain:
--       - 0001 grouped_sweep     tpsl1/tpsl2 four-table sets
--       - 0002 swing1 + next_kill swing_1 set + shared n_exit_next_kill column
--       - 0003 drop cohort        n_exit_cohort removed from every _results table
--   * 0002_trades_wallet_index / 0003_trades_wallet_index_covering — the lab-only
--       supporting index for the Trader Analysis query. End state = the COVERING
--       variant (0003 superseded 0002); folded in as the single
--       `idx_trades_wallet_time` create at the bottom of this file.
--   * 0004_n_exit_dead — the analysis death-close counter; folded into each
--       `_results` table's CREATE as `n_exit_dead INTEGER NOT NULL DEFAULT 0`.
--
-- Written **only by `lab`** (the workstation analysis box); `live`/EC2 never
-- touches them, which is why they live in the lab-owned migration set (applied
-- via the lab-private `_lab_migrations` ledger, NOT the shared `_sqlx_migrations`).
--
-- Per strategy (`tpsl1`, `tpsl2`, `swing_1`) a four-table set, names per
-- `crate::sweep::registry`'s `GroupedSweepTables`:
--   <s>_grouped_sweep_runs     one row per sweep run (header + lifecycle status)
--   <s>_grouped_sweep_groups   one row per surviving fingerprint group
--   <s>_grouped_sweep_results  one row per (group, ranked combo)
--   <s>_grouped_sweep_combos   per-run combo->params dictionary
--
-- Storage types mirror the repo's read/write code (RunDbRow / GroupDbRow /
-- ResultDbRow + the append/insert binds): results PnL/score floats are REAL (f32)
-- and the count columns INTEGER (i32); group best_score/expectancy stay DOUBLE
-- PRECISION; run buy_amount_sol is REAL. The `_results` exit-reason counters are
-- the same set for every strategy (the sweep repo is strategy-blind); swing1's
-- `n_exit_next_kill` is present but only swing1 ever populates it.
-- ============================================================================

-- ---------------------------------------------------------------------------
-- tpsl2
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    source           TEXT        NOT NULL,
    method           TEXT        NOT NULL,
    created_after    TIMESTAMPTZ,
    created_before   TIMESTAMPTZ,
    curve_only       BOOLEAN     NOT NULL,
    grouping_spec    JSONB       NOT NULL,
    axes_spec        JSONB       NOT NULL,
    min_tokens       INTEGER     NOT NULL,
    token_count      INTEGER     NOT NULL,
    group_count      INTEGER     NOT NULL,
    combo_count      INTEGER     NOT NULL,
    corpus_hash      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    status           TEXT        NOT NULL DEFAULT 'running',
    groups_done      INTEGER     NOT NULL DEFAULT 0,
    ix_labels_filter JSONB,
    field_filters    JSONB,
    token_cap        INTEGER,
    max_combos       INTEGER,
    label            TEXT,
    buy_amount_sol   REAL
);
CREATE INDEX IF NOT EXISTS idx_tpsl2_gsweep_runs_created
    ON tpsl2_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_groups (
    id                  UUID  PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID  NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER          NOT NULL,
    group_key           JSONB            NOT NULL,
    token_count         INTEGER          NOT NULL,
    fired_count         BIGINT           NOT NULL,
    best_combo_id       INTEGER          NOT NULL,
    best_score          DOUBLE PRECISION,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    best_params         JSONB            NOT NULL,
    mints               TEXT[]
);
CREATE INDEX IF NOT EXISTS idx_tpsl2_gsweep_groups_run
    ON tpsl2_grouped_sweep_groups(run_id, best_score DESC NULLS LAST, group_index ASC);

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_results (
    run_id              UUID    NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID    NOT NULL REFERENCES tpsl2_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER NOT NULL,
    n_fired             INTEGER NOT NULL,
    n_open              INTEGER NOT NULL,
    n_closed            INTEGER NOT NULL,
    win_rate            REAL    NOT NULL,
    total_pnl_sol       REAL    NOT NULL,
    mean_pnl_pct        REAL    NOT NULL,
    median_pnl_pct      REAL    NOT NULL,
    p90_pnl_pct         REAL    NOT NULL,
    best_pnl_pct        REAL    NOT NULL,
    worst_pnl_pct       REAL    NOT NULL,
    std_pnl_pct         REAL    NOT NULL,
    profit_factor       REAL,
    score               REAL,
    expectancy_sol      REAL    NOT NULL,
    avg_holding_secs    REAL    NOT NULL,
    median_holding_secs REAL    NOT NULL,
    n_exit_take_profit  INTEGER NOT NULL,
    n_exit_stop_loss    INTEGER NOT NULL,
    n_exit_trailing     INTEGER NOT NULL,
    n_exit_stall        INTEGER NOT NULL,
    n_exit_time         INTEGER NOT NULL,
    n_exit_liquidity    INTEGER NOT NULL,
    n_exit_next_kill    INTEGER NOT NULL DEFAULT 0,
    n_exit_dead         INTEGER NOT NULL DEFAULT 0,
    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tpsl2_gsweep_results_group
    ON tpsl2_grouped_sweep_results(run_id, group_id);

-- ---------------------------------------------------------------------------
-- tpsl1 (identical shape; separate tables per the registry map)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    source           TEXT        NOT NULL,
    method           TEXT        NOT NULL,
    created_after    TIMESTAMPTZ,
    created_before   TIMESTAMPTZ,
    curve_only       BOOLEAN     NOT NULL,
    grouping_spec    JSONB       NOT NULL,
    axes_spec        JSONB       NOT NULL,
    min_tokens       INTEGER     NOT NULL,
    token_count      INTEGER     NOT NULL,
    group_count      INTEGER     NOT NULL,
    combo_count      INTEGER     NOT NULL,
    corpus_hash      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    status           TEXT        NOT NULL DEFAULT 'running',
    groups_done      INTEGER     NOT NULL DEFAULT 0,
    ix_labels_filter JSONB,
    field_filters    JSONB,
    token_cap        INTEGER,
    max_combos       INTEGER,
    label            TEXT,
    buy_amount_sol   REAL
);
CREATE INDEX IF NOT EXISTS idx_tpsl1_gsweep_runs_created
    ON tpsl1_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_groups (
    id                  UUID  PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID  NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER          NOT NULL,
    group_key           JSONB            NOT NULL,
    token_count         INTEGER          NOT NULL,
    fired_count         BIGINT           NOT NULL,
    best_combo_id       INTEGER          NOT NULL,
    best_score          DOUBLE PRECISION,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    best_params         JSONB            NOT NULL,
    mints               TEXT[]
);
CREATE INDEX IF NOT EXISTS idx_tpsl1_gsweep_groups_run
    ON tpsl1_grouped_sweep_groups(run_id, best_score DESC NULLS LAST, group_index ASC);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_results (
    run_id              UUID    NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID    NOT NULL REFERENCES tpsl1_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER NOT NULL,
    n_fired             INTEGER NOT NULL,
    n_open              INTEGER NOT NULL,
    n_closed            INTEGER NOT NULL,
    win_rate            REAL    NOT NULL,
    total_pnl_sol       REAL    NOT NULL,
    mean_pnl_pct        REAL    NOT NULL,
    median_pnl_pct      REAL    NOT NULL,
    p90_pnl_pct         REAL    NOT NULL,
    best_pnl_pct        REAL    NOT NULL,
    worst_pnl_pct       REAL    NOT NULL,
    std_pnl_pct         REAL    NOT NULL,
    profit_factor       REAL,
    score               REAL,
    expectancy_sol      REAL    NOT NULL,
    avg_holding_secs    REAL    NOT NULL,
    median_holding_secs REAL    NOT NULL,
    n_exit_take_profit  INTEGER NOT NULL,
    n_exit_stop_loss    INTEGER NOT NULL,
    n_exit_trailing     INTEGER NOT NULL,
    n_exit_stall        INTEGER NOT NULL,
    n_exit_time         INTEGER NOT NULL,
    n_exit_liquidity    INTEGER NOT NULL,
    n_exit_next_kill    INTEGER NOT NULL DEFAULT 0,
    n_exit_dead         INTEGER NOT NULL DEFAULT 0,
    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tpsl1_gsweep_results_group
    ON tpsl1_grouped_sweep_results(run_id, group_id);

-- ---------------------------------------------------------------------------
-- swing_1 (kill→volume swing-phase strategy; identical shape)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS swing_1_grouped_sweep_runs (
    id               UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id      TEXT        NOT NULL,
    source           TEXT        NOT NULL,
    method           TEXT        NOT NULL,
    created_after    TIMESTAMPTZ,
    created_before   TIMESTAMPTZ,
    curve_only       BOOLEAN     NOT NULL,
    grouping_spec    JSONB       NOT NULL,
    axes_spec        JSONB       NOT NULL,
    min_tokens       INTEGER     NOT NULL,
    token_count      INTEGER     NOT NULL,
    group_count      INTEGER     NOT NULL,
    combo_count      INTEGER     NOT NULL,
    corpus_hash      TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    status           TEXT        NOT NULL DEFAULT 'running',
    groups_done      INTEGER     NOT NULL DEFAULT 0,
    ix_labels_filter JSONB,
    field_filters    JSONB,
    token_cap        INTEGER,
    max_combos       INTEGER,
    label            TEXT,
    buy_amount_sol   REAL
);
CREATE INDEX IF NOT EXISTS idx_swing_1_gsweep_runs_created
    ON swing_1_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS swing_1_grouped_sweep_groups (
    id                  UUID  PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID  NOT NULL REFERENCES swing_1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER          NOT NULL,
    group_key           JSONB            NOT NULL,
    token_count         INTEGER          NOT NULL,
    fired_count         BIGINT           NOT NULL,
    best_combo_id       INTEGER          NOT NULL,
    best_score          DOUBLE PRECISION,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    best_params         JSONB            NOT NULL,
    mints               TEXT[]
);
CREATE INDEX IF NOT EXISTS idx_swing_1_gsweep_groups_run
    ON swing_1_grouped_sweep_groups(run_id, best_score DESC NULLS LAST, group_index ASC);

CREATE TABLE IF NOT EXISTS swing_1_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES swing_1_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS swing_1_grouped_sweep_results (
    run_id              UUID    NOT NULL REFERENCES swing_1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID    NOT NULL REFERENCES swing_1_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER NOT NULL,
    n_fired             INTEGER NOT NULL,
    n_open              INTEGER NOT NULL,
    n_closed            INTEGER NOT NULL,
    win_rate            REAL    NOT NULL,
    total_pnl_sol       REAL    NOT NULL,
    mean_pnl_pct        REAL    NOT NULL,
    median_pnl_pct      REAL    NOT NULL,
    p90_pnl_pct         REAL    NOT NULL,
    best_pnl_pct        REAL    NOT NULL,
    worst_pnl_pct       REAL    NOT NULL,
    std_pnl_pct         REAL    NOT NULL,
    profit_factor       REAL,
    score               REAL,
    expectancy_sol      REAL    NOT NULL,
    avg_holding_secs    REAL    NOT NULL,
    median_holding_secs REAL    NOT NULL,
    n_exit_take_profit  INTEGER NOT NULL,
    n_exit_stop_loss    INTEGER NOT NULL,
    n_exit_trailing     INTEGER NOT NULL,
    n_exit_stall        INTEGER NOT NULL,
    n_exit_time         INTEGER NOT NULL,
    n_exit_liquidity    INTEGER NOT NULL,
    n_exit_next_kill    INTEGER NOT NULL DEFAULT 0,
    n_exit_dead         INTEGER NOT NULL DEFAULT 0,
    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_swing_1_gsweep_results_group
    ON swing_1_grouped_sweep_results(run_id, group_id);

-- ---------------------------------------------------------------------------
-- Lab-only supporting index for the Trader Analysis query
-- (`TradeRepo::wallet_traded_mints` -> GET /api/wallets/:wallet/tokens).
--
-- Folded from the former 0002_trades_wallet_index / 0003_trades_wallet_index_covering:
-- end state is the COVERING variant. Seek by `wallet_id`, range-scan
-- `block_time DESC` (recent-first), with `mint_address` + `trade_type` trailing so
-- the GROUP BY + `COUNT(*) FILTER (WHERE trade_type=…)` are satisfied index-only
-- (no heap fetch). Lives in the lab-only migration set on purpose: only `lab`
-- runs this analysis query, so the EC2 `live` box's ingest hot path pays NO extra
-- per-insert index-maintenance cost. Idempotent.
CREATE INDEX IF NOT EXISTS idx_trades_wallet_time
    ON trades (wallet_id, block_time DESC, mint_address, trade_type);
