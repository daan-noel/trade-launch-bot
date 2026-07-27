-- 0015: `fingerprints.ix_labels = '{}'` is the same state as NULL — store only one.
--
-- An empty label list is the collection analogue of a `0` sentinel: a second
-- spelling of "axis not part of this fingerprint's identity". The engine matcher
-- has always treated it that way (`configured_labels` / the legacy
-- `check_instruction_labels`), but `models::Fingerprint::has_any_criterion` used
-- a bare `ix_labels.is_some()`, so the two readers disagreed about whether such a
-- row has ANY criteria — and they gate opposite failure modes:
--
--   * the engine matcher: no criteria => matches NOTHING (rules bound to the
--     fingerprint go silently dead);
--   * `fingerprint_scope_clauses`: passing the guard with no axis set emits ZERO
--     predicates => the scoped dashboard matches EVERY token in the window.
--
-- `Fingerprint::from_json` now folds `[]` to NULL at the wire boundary so the
-- ambiguous state can't be created again; this normalizes rows already stored.
-- No CHECK: NULL is the canonical spelling and nothing can write '{}' anymore.

UPDATE fingerprints
SET ix_labels = NULL
WHERE ix_labels IS NOT NULL AND cardinality(ix_labels) = 0;
