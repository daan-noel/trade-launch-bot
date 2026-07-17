-- Generic (redesigned-engine) grouped sweep tables + the `n_exit_metrics` column.
--
-- Strategy redesign phase 5.4 collapses the three per-strategy sweep table sets
-- (`tpsl1_*`/`tpsl2_*`/`swing_1_*`) into ONE generic set. The generic engine has a
-- single metric-condition exit (`ExitReason::Metrics`) in place of the legacy
-- ladder's granular metric exits, so its results carry a new `n_exit_metrics`
-- counter. The shared `GroupedSweepRepo` writes one row shape across every table
-- set, so the legacy result tables gain the same column (always 0 there) to keep
-- the repo uniform through the transition (the legacy tables are dropped in
-- phase 7).
--
-- `IF NOT EXISTS` / `ADD COLUMN IF NOT EXISTS` keep this idempotent beside the
-- `_lab_migrations` ledger that already gates re-application.

-- 1. New exit counter on the legacy result tables (always 0 — the legacy ladder
--    never produces `ExitReason::Metrics`).
ALTER TABLE tpsl1_grouped_sweep_results   ADD COLUMN IF NOT EXISTS n_exit_metrics INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tpsl2_grouped_sweep_results   ADD COLUMN IF NOT EXISTS n_exit_metrics INTEGER NOT NULL DEFAULT 0;
ALTER TABLE swing_1_grouped_sweep_results ADD COLUMN IF NOT EXISTS n_exit_metrics INTEGER NOT NULL DEFAULT 0;

-- 2. The one generic table set (unprefixed) the redesigned sweep writes.
CREATE TABLE IF NOT EXISTS grouped_sweep_runs (
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
    buy_amount_sol   REAL,
    bucket_width_sol REAL
);
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_runs_created
    ON grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS grouped_sweep_groups (
    id                  UUID  PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID  NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
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
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_groups_run
    ON grouped_sweep_groups(run_id, best_score DESC NULLS LAST, group_index ASC);

CREATE TABLE IF NOT EXISTS grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

CREATE TABLE IF NOT EXISTS grouped_sweep_results (
    run_id              UUID    NOT NULL REFERENCES grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID    NOT NULL REFERENCES grouped_sweep_groups(id) ON DELETE CASCADE,
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
    n_exit_metrics      INTEGER NOT NULL DEFAULT 0,
    n_exit_open         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grouped_sweep_results_group
    ON grouped_sweep_results(run_id, group_id);
