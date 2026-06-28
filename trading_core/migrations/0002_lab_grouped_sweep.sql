-- ============================================================================
-- Lab-local grouped param-sweep storage (live/lab remake — Phase 4 gap fix).
--
-- The Phase-1 clean rebuild (0001_init.sql) recreated the shared market-data +
-- unified strategy tables but dropped the per-strategy grouped-sweep result
-- tables that the OLD schema built up across many incremental migrations
-- (0003_tpsl2_sweep, 0004_tpsl1_grouped_sweep, best_score, group_mints, and the
-- 0007 dedup/narrow). The Phase-4 carve-out keeps the sweep's own grouped engine
-- + tables (it did NOT move onto strategy_runs/strategy_run_metrics), so `lab`
-- still reads/writes them via `grouped_sweep_repo` + the `registry` table map.
-- This migration restores them in their final, post-0007 shape.
--
-- These tables are written **only by `lab`** (the workstation analysis box); `live`
-- never touches them. They live in the shared `trading_core` migration set because
-- that is the single migration runner both bins use — exactly as the pre-split
-- single backend created them everywhere. On EC2 they are created empty + unused.
--
-- Per strategy (`tpsl1`, `tpsl2`) a four-table set, names per
-- `crate::sweep::registry`'s `GroupedSweepTables`:
--   <s>_grouped_sweep_runs     one row per sweep run (header + lifecycle status)
--   <s>_grouped_sweep_groups   one row per surviving fingerprint group
--   <s>_grouped_sweep_results  one row per (group, ranked combo) — narrowed (0007)
--   <s>_grouped_sweep_combos   per-run combo->params dictionary (0007 dedup)
--
-- Storage types mirror the repo's read/write code (RunDbRow / GroupDbRow /
-- ResultDbRow + the append/insert binds): results PnL/score floats are REAL (f32)
-- and the count columns INTEGER (i32) post-0007; group best_score/expectancy stay
-- DOUBLE PRECISION; run buy_amount_sol is REAL.
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
    n_exit_cohort       INTEGER NOT NULL,
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
    n_exit_cohort       INTEGER NOT NULL,
    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tpsl1_gsweep_results_group
    ON tpsl1_grouped_sweep_results(run_id, group_id);
