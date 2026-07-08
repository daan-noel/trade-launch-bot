-- Post-launch token management, Phase 2: the audit log of executed management
-- actions (token-management-plan.md). Every sell/buy/consolidate the operator
-- runs lands one row — the previewed plan, what was selected, and the per-leg
-- outcome — so a "sell all" is never a black box.
--
-- NO FK on `mint_address` (same reason as token_positions: an action can target a
-- launch before its create tx is ingested into `tokens`).
--
-- Keep every CHECK IN-list byte-equal to the matching Rust enum's `as_str()` in
-- `platform_core::models::status` (ManageKind / ManageSizing / ManageStatus), each
-- guarded by its roundtrip test — this is the SQL half of that SSOT.

CREATE TABLE manage_actions (
    id             UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address   TEXT        NOT NULL,
    kind           TEXT        NOT NULL CHECK (kind IN ('sell','buy','consolidate')),
    sizing         TEXT        NOT NULL CHECK (sizing IN
                       ('pct_of_holdings','to_sol_target','fixed_base','fixed_sol','sweep')),
    -- The wallet selection that produced the plan: {role?, wallet_ids?}.
    selection      JSONB       NOT NULL DEFAULT '{}'::jsonb,
    -- The per-wallet legs actually executed (the ActionPlan): full audit of who
    -- sold/bought how much and the confirmed signature per leg.
    plan           JSONB       NOT NULL DEFAULT '[]'::jsonb,
    status         TEXT        NOT NULL DEFAULT 'planned' CHECK (status IN
                       ('planned','executing','completed','partial','failed')),
    legs_total     INTEGER     NOT NULL DEFAULT 0,
    legs_confirmed INTEGER     NOT NULL DEFAULT 0,
    error          TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at   TIMESTAMPTZ
);

-- The one read path is a mint's action history, newest first.
CREATE INDEX idx_manage_actions_mint ON manage_actions(mint_address, created_at DESC);
