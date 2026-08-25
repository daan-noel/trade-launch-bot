-- ============================================================================
-- ix_pattern_sets — named `ix_labels` pattern sets owned by ANALYSIS, not by a
-- fingerprint.
--
-- `volume_ix_patterns` lives on a fingerprint's `metric_config` and is what the
-- ENGINE classifies flow with. A trader study has no fingerprint: the tokens a
-- wallet traded belong to no cohort, so the chart's vol/non-vol lines and the
-- trades table's Vol column have nothing to classify against.
--
-- This table is the second OWNER of the same fact — one set of ordered label
-- sequences — for read-only study surfaces (Trader Analysis' flow lens). The
-- classifier stays the ONE in `hunter_engine::metrics::flow_split` (mirrored
-- client-side in `lib/flow/classifyFlow.ts`); only where the set is stored
-- differs. A set is promoted into a fingerprint by copying its patterns, which
-- is the only path from study to a rule the engine reads.
--
-- `patterns` shape: `[{"group": "Axiom Trade" | null, "ix_labels": ["…", "…"]}]`.
-- `group` labels a subset so a lens can be narrowed to one launch client without
-- re-pasting; it carries no meaning to the classifier, which matches the
-- ordered `ix_labels` array exactly.
--
-- Written only by `lab` (workstation); `live`/EC2 never reads or writes it.
-- Plan: docs/plans/strategies/trader-flow-lens.md
-- ============================================================================

CREATE TABLE IF NOT EXISTS ix_pattern_sets (
    id             UUID        PRIMARY KEY,
    name           TEXT        NOT NULL,
    -- The wallet the set was derived FOR (a study tag, not a scope): a lens is
    -- reusable across traders, so this only sorts a picker and is nullable.
    wallet_address TEXT,
    patterns       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    notes          TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The picker addresses a set by name, so two sets may not share one.
CREATE UNIQUE INDEX IF NOT EXISTS idx_ix_pattern_sets_name
    ON ix_pattern_sets (lower(name));

CREATE INDEX IF NOT EXISTS idx_ix_pattern_sets_wallet
    ON ix_pattern_sets (wallet_address)
    WHERE wallet_address IS NOT NULL;
