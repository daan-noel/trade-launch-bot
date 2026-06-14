-- =========================================================================
-- TPSL2 positions — physical column reorder to target_* / entry_* / exit_*
--   Postgres can't reposition columns in place, so each positions table is
--   rebuilt (rename-old → create-new in the desired order → copy → drop old).
--   Same columns, constraints, and indexes as before — only the on-disk column
--   order changes: target_* (the scalp-entry trigger snapshot from 0004) now
--   precede entry_*/exit_*, matching the Rust structs, the JSON response, and
--   the frontend positions table. No rows are dropped or altered.
-- =========================================================================

-- ---- tpsl2_real_positions ------------------------------------------------
ALTER TABLE tpsl2_real_positions RENAME TO tpsl2_real_positions_old;

CREATE TABLE tpsl2_real_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    token_program_id    TEXT,
    target_price        DOUBLE PRECISION,
    target_amount       DOUBLE PRECISION,
    target_time         TIMESTAMPTZ,
    target_tx           TEXT,
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

INSERT INTO tpsl2_real_positions
    (id, mint, wallet, token_program_id,
     target_price, target_amount, target_time, target_tx,
     entry_price, entry_amount, entry_time, entry_tx,
     exit_price, exit_amount, exit_time, exit_tx,
     status, strategy, rule_id, exit_reason, created_at, updated_at)
SELECT
     id, mint, wallet, token_program_id,
     target_price, target_amount, target_time, target_tx,
     entry_price, entry_amount, entry_time, entry_tx,
     exit_price, exit_amount, exit_time, exit_tx,
     status, strategy, rule_id, exit_reason, created_at, updated_at
FROM tpsl2_real_positions_old;

DROP TABLE tpsl2_real_positions_old;

CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint              ON tpsl2_real_positions(mint);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_wallet            ON tpsl2_real_positions(wallet);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_status            ON tpsl2_real_positions(status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_strategy          ON tpsl2_real_positions(strategy);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_rule_id           ON tpsl2_real_positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_mint_status       ON tpsl2_real_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_real_positions_token_program_id  ON tpsl2_real_positions(token_program_id);

-- ---- tpsl2_paper_positions ----------------------------------------------
ALTER TABLE tpsl2_paper_positions RENAME TO tpsl2_paper_positions_old;

CREATE TABLE tpsl2_paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES tpsl2_paper_test_run(id) ON DELETE CASCADE,
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    token_program_id    TEXT,
    target_price        DOUBLE PRECISION,
    target_amount       DOUBLE PRECISION,
    target_time         TIMESTAMPTZ,
    target_tx           TEXT,
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

INSERT INTO tpsl2_paper_positions
    (id, run_id, mint, wallet, token_program_id,
     target_price, target_amount, target_time, target_tx,
     entry_price, entry_amount, entry_time, entry_tx,
     exit_price, exit_amount, exit_time, exit_tx,
     status, strategy, rule_id, exit_reason, created_at, updated_at)
SELECT
     id, run_id, mint, wallet, token_program_id,
     target_price, target_amount, target_time, target_tx,
     entry_price, entry_amount, entry_time, entry_tx,
     exit_price, exit_amount, exit_time, exit_tx,
     status, strategy, rule_id, exit_reason, created_at, updated_at
FROM tpsl2_paper_positions_old;

DROP TABLE tpsl2_paper_positions_old;

CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_run         ON tpsl2_paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_mint_status ON tpsl2_paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_tpsl2_paper_positions_rule        ON tpsl2_paper_positions(rule_id);
