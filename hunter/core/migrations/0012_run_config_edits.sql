-- ===========================================================================
-- 0012  a run records the config it is running under, and every edit to it
-- ===========================================================================
-- `params_snapshot` freezes the rule params at run launch, and nothing updated it
-- afterwards. A rule edited while active keeps its run: the engine reloads and the
-- new config decides from that moment, so one run's numbers can span two configs
-- while the snapshot still describes only the first.
--
-- Two things the snapshot cannot say on its own:
--   * WHEN the config changed, so a run's numbers can be read either side of it;
--   * that the FINGERPRINT changed. `m_flow_ix.ix_patterns` and the identity axes
--     live on `fingerprints`, never in `params_snapshot`, and one fingerprint is
--     shared by every rule that points at it - so an ix-structure edit silently
--     re-defines several runs at once and leaves no trace in any of them.
--
-- `config_hash` is the digest of the config the run is currently running under
-- (rule params + buy size + caps + the fingerprint's criteria + its metric_config);
-- `config_edits` is the append-only log of the changes that landed mid-run, each
-- `{"at": <rfc3339>, "changed": ["ix structure", ...]}`. The run is NOT rotated:
-- rotating mid-flight would split a rule's open positions across two runs.
--
-- NULL `config_hash` = a run that started before this migration (or a run whose
-- config was never observed). The first reload after deploy adopts the live config
-- as that run's baseline WITHOUT recording an edit - there is nothing to diff it
-- against, and inventing one would date a change that may never have happened.
-- ===========================================================================

ALTER TABLE strategy_runs
    ADD COLUMN IF NOT EXISTS config_hash  TEXT,
    ADD COLUMN IF NOT EXISTS config_edits JSONB NOT NULL DEFAULT '[]'::jsonb;

COMMENT ON COLUMN strategy_runs.config_hash IS
    'Digest of the config this run is running under now (params + sizing + caps + fingerprint criteria + metric_config). NULL = never observed.';
COMMENT ON COLUMN strategy_runs.config_edits IS
    'Append-only [{at, changed[]}] of config changes that landed while this run was Running. Non-empty = the run''s numbers span more than one config.';
