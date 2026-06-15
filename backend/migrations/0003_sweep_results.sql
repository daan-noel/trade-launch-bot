-- ============================================================================
-- Strategy param-sweep results.
--
-- A `sweep` CLI run (`backend -- sweep --strategy …`) backtests a grid of param
-- pairs over the token/trade corpus and folds the per-(combo, token) outcomes
-- into one ranked row per param pair. `sweep_runs` is one row per run;
-- `sweep_results` is one row per param-pair combo in that run (bounded —
-- hundreds to low thousands), read whole by the dashboard's per-strategy sweep
-- page for client-side sort/filter.
-- ============================================================================

CREATE TABLE IF NOT EXISTS sweep_runs (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy        TEXT        NOT NULL,          -- 'tpsl1' | 'tpsl2'
    rule_id         UUID,                          -- base rule the params overlay
    source          TEXT        NOT NULL,          -- 'cache' | 'db'
    method          TEXT        NOT NULL,          -- 'grid' | 'random' | 'lhs'
    token_count     INTEGER     NOT NULL,
    combo_count     INTEGER     NOT NULL,
    corpus_hash     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sweep_runs_strategy_created
    ON sweep_runs(strategy, created_at DESC);

CREATE TABLE IF NOT EXISTS sweep_results (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES sweep_runs(id) ON DELETE CASCADE,
    combo_id            INTEGER     NOT NULL,
    params              JSONB       NOT NULL,      -- the swept knob values

    -- counts
    n_fired             BIGINT      NOT NULL,
    n_open              BIGINT      NOT NULL,
    n_closed            BIGINT      NOT NULL,

    -- profitability / success
    win_rate            DOUBLE PRECISION NOT NULL,
    total_pnl_sol       DOUBLE PRECISION NOT NULL,
    mean_pnl_pct        DOUBLE PRECISION NOT NULL,
    median_pnl_pct      DOUBLE PRECISION NOT NULL,
    p90_pnl_pct         DOUBLE PRECISION NOT NULL,
    best_pnl_pct        DOUBLE PRECISION NOT NULL,
    worst_pnl_pct       DOUBLE PRECISION NOT NULL,
    profit_factor       DOUBLE PRECISION,          -- NULL = no losing trades (∞)
    expectancy_sol      DOUBLE PRECISION NOT NULL,
    avg_holding_secs    DOUBLE PRECISION NOT NULL,
    median_holding_secs DOUBLE PRECISION NOT NULL,

    -- exit-reason mix
    exit_take_profit    INTEGER     NOT NULL,
    exit_stop_loss      INTEGER     NOT NULL,
    exit_trailing       INTEGER     NOT NULL,
    exit_stall          INTEGER     NOT NULL,
    exit_time           INTEGER     NOT NULL,
    exit_liquidity      INTEGER     NOT NULL,
    exit_cohort         INTEGER     NOT NULL,
    exit_open           INTEGER     NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sweep_results_run ON sweep_results(run_id);
