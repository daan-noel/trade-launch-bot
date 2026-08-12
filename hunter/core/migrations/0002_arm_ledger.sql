-- ============================================================================
-- 0002 arm_ledger — `strategy_arms`, the durable record of every (rule, mint)
--   arming episode.
--
-- An arm is a decision the engine makes and then discards: `ArmedRegistry` holds
-- it in RAM while it lives and the `strategy_armed_changed` SSE announces its
-- end. Nothing wrote it down, so a rule's selectivity — how many tokens it
-- looked at per token it bought — was unmeasurable, and a disarmed token was
-- gone the moment the tab reloaded.
--
-- One row per episode: written at the arm (`ended_at` NULL), updated at whatever
-- ended it. Insert-at-arm rather than a single write at the end so a restart
-- cannot swallow every in-flight episode.
--
-- Volume is UNBOUNDED by design — an arm costs nothing on chain, so a loose
-- fingerprint arms on most launches. Hypertable + compression + retention on the
-- same footing as `trades`, so an episode and the trades that explain it expire
-- together. Treat it as a rolling buffer, never as permanent history.
--
-- Plan: docs/plans/strategies/arm-ledger.md
-- ============================================================================

CREATE TABLE IF NOT EXISTS strategy_arms (
    rule_id       UUID        NOT NULL,
    mint_address  TEXT        NOT NULL,
    -- Frozen at arm time: a rule's trade_mode can be flipped mid-life, and the
    -- episode belongs to the mode it was armed under.
    mode          TEXT        NOT NULL CHECK (mode IN ('real', 'paper')),

    armed_at      TIMESTAMPTZ NOT NULL,
    -- NULL = the episode is still live (the engine is evaluating entry).
    ended_at      TIMESTAMPTZ,
    end_reason    TEXT,
    -- Set only when end_reason = 'entered'. No FK: `strategy_positions` has no
    -- retention policy and this table does, so the ledger must be droppable
    -- without touching a position row (and vice versa).
    position_id   UUID,

    -- Natural key. `armed_at` leads because it is the partition column — a
    -- hypertable rejects a unique index that omits it.
    PRIMARY KEY (armed_at, rule_id, mint_address)
);

-- The engine's DisarmReason vocabulary plus the sink's own `entered`: the Enter
-- path emits no ArmedChanged, so the position sink closes the episode itself.
ALTER TABLE strategy_arms DROP CONSTRAINT IF EXISTS strategy_arms_end_reason_check;
ALTER TABLE strategy_arms
    ADD CONSTRAINT strategy_arms_end_reason_check
    CHECK (end_reason IS NULL OR end_reason IN
        ('entered','dead','migrated','unsatisfiable','paused','duplicate_identity'));

-- A live episode must have no reason, and an ended one must have both halves —
-- otherwise `waited_sec` and the funnel counts read a half-written row.
ALTER TABLE strategy_arms DROP CONSTRAINT IF EXISTS strategy_arms_ended_pair_check;
ALTER TABLE strategy_arms
    ADD CONSTRAINT strategy_arms_ended_pair_check
    CHECK ((ended_at IS NULL) = (end_reason IS NULL));

COMMENT ON TABLE strategy_arms IS
    'One row per (rule, mint) arming episode: armed_at -> ended_at/end_reason. '
    'The durable twin of the in-RAM ArmedRegistry; the Console Waiting lane reads '
    'the registry, the Console Arms section reads this.';
COMMENT ON COLUMN strategy_arms.end_reason IS
    'entered | dead | migrated | unsatisfiable | paused | duplicate_identity. '
    'Written from disarm_reason_str (engine SSOT); `entered` is the sink member.';

SELECT create_hypertable(
    'strategy_arms',
    by_range('armed_at', INTERVAL '1 day'),
    if_not_exists => TRUE
);

-- Segment by rule: every read is either rule-scoped or reason-scoped, and an
-- episode row is narrow enough that ordering by armed_at within a rule leaves
-- long runs of identical rule_id/mode for the compressor.
ALTER TABLE strategy_arms SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'rule_id',
    timescaledb.compress_orderby   = 'armed_at DESC'
);

-- Compression only ever sees SETTLED chunks: an episode ends within a token's
-- life (minutes), so a day-old chunk no longer takes UPDATEs.
SELECT add_compression_policy('strategy_arms', compress_after => INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('strategy_arms', drop_after => INTERVAL '30 days', if_not_exists => TRUE);

-- The Arms section's default read: one rule (or all), newest first, over a window.
CREATE INDEX IF NOT EXISTS idx_strategy_arms_rule_armed
    ON strategy_arms(rule_id, armed_at DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_arms_mint
    ON strategy_arms(mint_address, armed_at DESC);
-- The funnel counts and the `end_reason` column filter.
CREATE INDEX IF NOT EXISTS idx_strategy_arms_reason_armed
    ON strategy_arms(end_reason, armed_at DESC);
-- Boot reconciliation: episodes left live by a crash.
CREATE INDEX IF NOT EXISTS idx_strategy_arms_live
    ON strategy_arms(armed_at DESC) WHERE ended_at IS NULL;
