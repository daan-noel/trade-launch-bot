-- Post-launch token management, Phase 4: simple-threshold sell ladders
-- (token-management-plan.md). An armed ladder auto-fires a sell of X% across a
-- wallet selection when the token crosses a price / market-cap milestone —
-- authored in SLP, evaluated against the ingested market state (NOT meme-trading's
-- tpsl engine). This is the subsystem's FIRST automation; every fired rung still
-- runs through the Phase 2 sell pipeline (audit + feed-confirm) and is gated by
-- the same MANAGE_ENABLED kill switch.
--
-- NO FK on mint_address (same as token_positions / manage_actions).
-- `rungs` is a JSONB array of { metric, threshold, pct, fired } — the ladder's
-- SSOT is the row; the evaluator flips each rung's `fired` in place.
--
-- Keep the status CHECK byte-equal to `platform_core::models::status::LadderStatus`
-- (roundtrip-tested) — the SQL half of that SSOT.

CREATE TABLE sell_ladders (
    id           UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address TEXT        NOT NULL,
    selection    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    rungs        JSONB       NOT NULL DEFAULT '[]'::jsonb,
    status       TEXT        NOT NULL DEFAULT 'armed' CHECK (status IN ('armed','done','cancelled')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The evaluator scans only ARMED ladders — a partial index keeps that cheap.
CREATE INDEX idx_sell_ladders_armed ON sell_ladders(mint_address) WHERE status = 'armed';
