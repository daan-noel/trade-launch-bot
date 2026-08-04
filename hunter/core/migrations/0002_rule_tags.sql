-- ============================================================================
-- 0002 — rule tags: free-form labels on `strategy_rules`.
--
-- Presentational metadata only (slice the Rules board: show only `fam:scalper`,
-- hide `stage:experiment`). Deliberately a typed column and NOT a `params` key:
--
--   * `params` is re-serialized from `RuleParams` on every write, so an unknown
--     key is dropped on the first save;
--   * `params` IS trading identity (`RuleRepo::find_identical` compares it), so a
--     tag in there would silently weaken the Duplicate gate;
--   * `params` is frozen into `strategy_runs.params_snapshot`, so a tag rename
--     would read as a strategy change in run history.
--
-- Tags sit in the same bucket as `rule_name`: a label, never identity, never an
-- input to the decision kernel (`list_active` is untouched).
--
-- Array column rather than a `rule_tags` join table: the rules list is fetched
-- whole and filtered client-side, so there is no server-side tag query to
-- normalize for, and the catalog is derivable
-- (`SELECT DISTINCT unnest(tags) FROM strategy_rules`). No GIN index for the
-- same reason — add one only alongside a real `WHERE tags && $1` read.
--
-- Canonical shape (lowercase, `-` separated, deduped, sorted, no `,`) is enforced
-- in one place, `trading_core::strategies::rules::normalize_tags`; the DB only
-- guarantees non-NULL. Rationale + filter semantics:
-- docs/plans/strategies/rule-tags.md
-- ============================================================================

ALTER TABLE strategy_rules
    ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}'::text[];
