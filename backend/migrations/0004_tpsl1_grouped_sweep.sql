-- =========================================================================
-- TPSL1 grouped param-sweep tables
--
-- Same shape as TPSL2's triple (the grouped-sweep repo is table-name-driven and
-- the registry maps `strategy_id` → this triple), but separate per the
-- per-strategy-tables design. TPSL1 sweeps the exit ladder only (TP/SL plus the
-- optional trailing/time/stall/liquidity exits); it has no scalp entry gates and
-- no cohort-dump exit, so its `params` JSON carries fewer keys and the
-- `n_exit_cohort` count is always 0 — the column is kept for schema parity so the
-- generic repo can write every strategy's rows the same way.
--
-- TPSL1's triple:
--   tpsl1_grouped_sweep_runs    — one row per sweep invocation
--   tpsl1_grouped_sweep_groups  — one row per surviving fingerprint group
--   tpsl1_grouped_sweep_results — one row per (group, param-combo)
-- =========================================================================

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_runs (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    strategy_id     TEXT        NOT NULL,          -- 'tpsl1' (traceability)
    rule_id         UUID,                          -- base rule the params overlay
    source          TEXT        NOT NULL,          -- 'db' (DB-range corpus)
    method          TEXT        NOT NULL,          -- 'grid' | 'random' | 'lhs'
    created_after   TIMESTAMPTZ,                   -- selection lower bound (incl.)
    created_before  TIMESTAMPTZ,                   -- selection upper bound (excl.)
    curve_only      BOOLEAN     NOT NULL DEFAULT FALSE,
    grouping_spec   JSONB       NOT NULL,          -- ["cu_price", …]
    axes_spec       JSONB       NOT NULL,          -- resolved param axes
    min_tokens      INTEGER     NOT NULL,          -- groups below this are dropped
    token_count     INTEGER     NOT NULL,          -- corpus size after selection
    group_count     INTEGER     NOT NULL,          -- surviving groups swept
    combo_count     INTEGER     NOT NULL,          -- combos per group
    corpus_hash     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_grouped_sweep_runs_created
    ON tpsl1_grouped_sweep_runs(created_at DESC);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_groups (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_index         INTEGER     NOT NULL,      -- deterministic order (largest first)
    group_key           JSONB       NOT NULL,      -- {"cu_price":"…", …}; {} = ALL
    token_count         INTEGER     NOT NULL,
    fired_count         BIGINT      NOT NULL,       -- best combo's n_fired (sample size)
    best_combo_id       INTEGER     NOT NULL,
    best_expectancy_sol DOUBLE PRECISION NOT NULL,
    best_params         JSONB       NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl1_grouped_sweep_groups_run
    ON tpsl1_grouped_sweep_groups(run_id, group_index);

CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_results (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    group_id            UUID        NOT NULL REFERENCES tpsl1_grouped_sweep_groups(id) ON DELETE CASCADE,
    combo_id            INTEGER     NOT NULL,
    params              JSONB       NOT NULL,       -- the swept knob values

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
    std_pnl_pct         DOUBLE PRECISION NOT NULL DEFAULT 0,  -- stddev of realized pnl%
    profit_factor       DOUBLE PRECISION,           -- NULL = no losing trades (∞)
    score               DOUBLE PRECISION,           -- robust rank μ−z·σ/√n; NULL = n_closed<2
    expectancy_sol      DOUBLE PRECISION NOT NULL,
    avg_holding_secs    DOUBLE PRECISION NOT NULL,
    median_holding_secs DOUBLE PRECISION NOT NULL,

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
