-- -------------------------------------------------------------------------
-- strategy_TPSL_rules
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS strategy_TPSL_rules (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    rule_name           TEXT        NOT NULL,
    p_initial_buy_sol   DOUBLE PRECISION NOT NULL,
    p_cu_limit          BIGINT,
    p_cu_price          BIGINT,
    p_ix_labels         JSONB       NOT NULL DEFAULT '[]',
    buy_amount          DOUBLE PRECISION NOT NULL,
    take_profit         DOUBLE PRECISION NOT NULL,
    stop_loss           DOUBLE PRECISION NOT NULL,
    is_active           BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_strategy_TPSL_rules_active ON strategy_TPSL_rules(is_active);

-- -------------------------------------------------------------------------
-- positions
-- -------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS positions (
    id                  UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint                TEXT        NOT NULL,
    wallet              TEXT        NOT NULL,
    entry_price         DOUBLE PRECISION NOT NULL,
    exit_price          DOUBLE PRECISION,
    entry_tx            TEXT        NOT NULL UNIQUE,
    exit_tx             TEXT        UNIQUE,
    status              TEXT        NOT NULL CHECK (status IN ('Holding', 'End')),
    strategy            TEXT        NOT NULL,
    rule_id             UUID        NOT NULL,
    entry_amount        DOUBLE PRECISION NOT NULL,
    exit_amount         DOUBLE PRECISION,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    
    FOREIGN KEY (rule_id) REFERENCES strategy_TPSL_rules(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_positions_mint ON positions(mint);
CREATE INDEX IF NOT EXISTS idx_positions_wallet ON positions(wallet);
CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status);
CREATE INDEX IF NOT EXISTS idx_positions_strategy ON positions(strategy);
CREATE INDEX IF NOT EXISTS idx_positions_rule_id ON positions(rule_id);
CREATE INDEX IF NOT EXISTS idx_positions_mint_status ON positions(mint, status);
