-- Post-launch token management, Phase 5: volume-making bots (token-management-plan.md).
-- A volume bot autonomously cycles a jittered buy (+ optional sell-back) across a
-- rotating wallet selection on an interval, generating trade volume on one of our
-- launched tokens. Built on the Phase 2/3 buy + sell pipelines (audit + confirm) —
-- NOT a second trade engine. Gated by the same MANAGE_ENABLED kill switch: the
-- scheduler no-ops while it's off, so a `running` bot never silently trades.
--
-- NO FK on mint_address (same as token_positions / manage_actions / sell_ladders).
-- `selection` is a WalletSelection {role?, wallet_ids?} — the pool the cycle
-- rotates across. `config` is a VolumeConfig JSONB (buy size range, interval range,
-- sell-back pct, SOL budget, optional max cycles) — the bot's SSOT is the row.
-- Money guardrails live in exact bigint lamports (`_quote` = quote base units).
--
-- Keep the status CHECK byte-equal to `platform_core::models::status::VolumeBotStatus`
-- (roundtrip-tested) — the SQL half of that SSOT.

CREATE TABLE volume_bots (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address  TEXT        NOT NULL,
    status        TEXT        NOT NULL DEFAULT 'running' CHECK (status IN ('running','paused','stopped')),
    selection     JSONB       NOT NULL DEFAULT '{}'::jsonb,
    config        JSONB       NOT NULL,
    cycles_done   INT         NOT NULL DEFAULT 0,
    -- Cumulative SOL (lamports) spent on buys — checked against the config budget.
    spent_quote   BIGINT      NOT NULL DEFAULT 0,
    -- Cumulative notional (lamports) traded (buys + sell-backs) — the volume stat.
    volume_quote  BIGINT      NOT NULL DEFAULT 0,
    -- When the scheduler should run the next cycle (jittered forward each cycle).
    next_run_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The scheduler scans only RUNNING bots that are due — a partial index keeps that
-- scan cheap regardless of how many stopped bots accumulate.
CREATE INDEX idx_volume_bots_due ON volume_bots(next_run_at) WHERE status = 'running';
