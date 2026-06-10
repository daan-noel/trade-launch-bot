-- Paper-trading runs + positions, isolated from the real `positions` table
-- (which is now reserved for real trade history).
--
-- A paper "run" begins when a paper rule is activated and ends when its
-- total-token cap is reached and every position has exited (the rule
-- auto-deactivates and a notification fires), or when the rule is manually
-- stopped. Only the latest run per rule is retained: starting a new run deletes
-- the prior run for that rule, and its positions go with it via ON DELETE
-- CASCADE.

CREATE TABLE IF NOT EXISTS paper_test_runs (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_id            UUID        NOT NULL REFERENCES strategy_TPSL_rules(id) ON DELETE CASCADE,
    -- Monotonic per-rule run counter (1, 2, 3 …) for display ("run #N").
    run_seq            BIGINT      NOT NULL,
    status             TEXT        NOT NULL CHECK (status IN ('Running', 'Finished', 'Stopped')),
    -- Snapshot of the rule's total-token cap at run start (NULL = unlimited).
    max_total_tokens   BIGINT,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    UNIQUE (rule_id, run_seq)
);

CREATE INDEX IF NOT EXISTS idx_paper_test_runs_rule ON paper_test_runs(rule_id);

-- Mirrors `positions` (minus the UNIQUE constraints on entry_tx / exit_tx — a
-- paper position is seeded with the token's creation tx and the same token may
-- be traded again in a later run, so those columns are not unique here) plus a
-- run_id binding each position to its run.
CREATE TABLE IF NOT EXISTS paper_positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id              UUID        NOT NULL REFERENCES paper_test_runs(id) ON DELETE CASCADE,
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
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'ExitPending', 'End')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_paper_positions_run         ON paper_positions(run_id);
CREATE INDEX IF NOT EXISTS idx_paper_positions_mint_status ON paper_positions(mint, status);
CREATE INDEX IF NOT EXISTS idx_paper_positions_rule        ON paper_positions(rule_id);
