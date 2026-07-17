-- 0004_strategy_redesign.sql — fingerprint + metrics generic engine (Phase 0).
--
-- The named strategies (tpsl_sniper_1 / tpsl_sniper_2 / swing_1) are replaced by
-- ONE generic engine: a rule = fingerprint reference + metric conditions. See
-- hunter/docs/plans/strategy-redesign/fingerprint-metrics-engine-plan.md.
--
-- Unit convention (0009_sol_lamports_naming): SOL amounts at rest are exact
-- lamports BIGINT with a `_lamports` suffix; models expose human-SOL f64.

-- 1. fingerprints — a token-creation shape, shared by many rules. Exact-match
--    fields: cu_limit, cu_price, ix_labels (ordered). Bucket-matched fields (via
--    this row's own bucket_size_amount, SSOT grouping::same_bucket): the five
--    lamports axes. NULL = the field is not part of the fingerprint's identity.
CREATE TABLE fingerprints (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                     TEXT NOT NULL,
    cu_limit                 BIGINT,            -- exact match
    cu_price                 BIGINT,            -- exact match
    init_buy_lamports        BIGINT,            -- bucket-matched ┐
    max_cost_lamports        BIGINT,            --                │ all via this row's
    spendable_lamports_in    BIGINT,            --                │ bucket_size_amount
    first_slot_buy_lamports  BIGINT,            -- sum in slot    │
    first_slot_sell_lamports BIGINT,            -- sum in slot    ┘
    bucket_size_amount       DOUBLE PRECISION NOT NULL DEFAULT 0.1,  -- SOL width
    ix_labels                TEXT[],            -- exact ordered sequence
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. Legacy rules kept read-only for reference — the params vocabulary is
--    incompatible with the new engine, so old rules are NOT migrated.
ALTER TABLE strategy_rules RENAME TO strategy_rules_legacy;

-- 3. New rules table. Columns say HOW the rule trades; params JSONB says WHEN
--    (strict take_profit/stop_loss + entry/exit metric-condition groups with
--    {operator, value} lists — shape in the redesign plan §5).
CREATE TABLE strategy_rules (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_name             TEXT NOT NULL,
    fingerprint_id        UUID NOT NULL REFERENCES fingerprints(id),
    trade_mode            TEXT NOT NULL CHECK (trade_mode IN ('paper','real')),
    is_active             BOOLEAN NOT NULL DEFAULT false,
    buy_amount_lamports   BIGINT NOT NULL,
    max_concurrent_tokens BIGINT NOT NULL DEFAULT 1,
    max_total_tokens      BIGINT NOT NULL DEFAULT 0,      -- 0 = unlimited
    params                JSONB NOT NULL,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_strategy_rules_active ON strategy_rules (is_active, trade_mode);
CREATE INDEX idx_strategy_rules_fingerprint ON strategy_rules (fingerprint_id);

-- strategy_positions keeps its lifecycle unchanged. Its strategy_id TEXT loses
-- meaning under the generic engine: existing rows keep their historical values
-- (they reference strategy_rules_legacy rules); new engine writes use the
-- constant 'generic'. The column is dropped in a later cleanup migration once
-- nothing reads it. exit_reason vocabulary becomes:
-- TakeProfit | StopLoss | Metrics | Dead | Manual | Migrated.
