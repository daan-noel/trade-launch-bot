-- tpsl_sniper_2 — a full, independent clone of the tpsl_sniper_1 storage:
-- its own rules table plus its own real/paper position tables, so the second
-- strategy can run (and later diverge with the N1–N10 features) without sharing
-- rows or foreign keys with the live tpsl_sniper_1 tables.
--
-- Mirrors the current shape of the originals: strategy_TPSL_rules (0001) and
-- positions / paper_positions including the 'ExitFailed' status (0002) and the
-- exit_reason column (0003).

-- -------------------------------------------------------------------------
-- strategy_TPSL2_rules  (clone of strategy_TPSL_rules)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS strategy_TPSL2_rules (
    id                      UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name               TEXT        NOT NULL,
    p_initial_buy_sol       DOUBLE PRECISION,
    p_cu_limit              BIGINT,
    p_cu_price              BIGINT,
    p_max_sol_cost          DOUBLE PRECISION,
    p_spendable_sol_in      DOUBLE PRECISION,
    p_max_concurrent_tokens BIGINT,
    p_max_total_tokens      BIGINT,
    p_ix_labels             JSONB       NOT NULL DEFAULT '[]',
    p_trailing_stop_pct     DOUBLE PRECISION,
    p_time_stop_secs        BIGINT,
    p_stall_secs            BIGINT,
    p_liquidity_drop_pct    DOUBLE PRECISION,
    buy_amount              DOUBLE PRECISION NOT NULL,
    take_profit             DOUBLE PRECISION NOT NULL,
    stop_loss               DOUBLE PRECISION NOT NULL,
    tolerance_pct           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    trade_mode              TEXT        NOT NULL DEFAULT 'paper',
    is_active               BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_TPSL2_rules_active ON strategy_TPSL2_rules(is_active);

-- -------------------------------------------------------------------------
-- tpsl2_positions  (clone of positions, with exit_reason + ExitFailed)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_positions (
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

    FOREIGN KEY (rule_id) REFERENCES strategy_TPSL2_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_mint              ON tpsl2_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_wallet            ON tpsl2_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_status            ON tpsl2_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_strategy          ON tpsl2_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_rule_id           ON tpsl2_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_mint_status       ON tpsl2_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_positions_token_program_id  ON tpsl2_positions(token_program_id);

-- -------------------------------------------------------------------------
-- tpsl2_paper_test_runs  (clone of paper_test_runs)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_paper_test_runs (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_id            UUID        NOT NULL REFERENCES strategy_TPSL2_rules(id) ON DELETE CASCADE,
    run_seq            BIGINT      NOT NULL,
    status             TEXT        NOT NULL CHECK (status IN ('Running', 'Finished', 'Stopped')),
    max_total_tokens   BIGINT,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    UNIQUE (rule_id, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_test_runs_rule ON tpsl2_paper_test_runs(rule_id);

-- -------------------------------------------------------------------------
-- tpsl2_paper_positions  (clone of paper_positions, with exit_reason + ExitFailed)
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tpsl2_paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl2_paper_test_runs(id) ON DELETE CASCADE,
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
