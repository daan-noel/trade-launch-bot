-- ===========================================================================
-- 0003  grouped sweep runs: an explicit PARTITION replaces the bucket width
-- ===========================================================================
-- A run stored one `bucket_width_sol` for every continuous SOL grouping field —
-- an infinite implicit lattice (`floor(v/width)`) that the sweep, the promoted
-- rule's matcher, and the creation-stats SQL each had to re-derive identically,
-- down to a boundary epsilon, with a `0` in it meaning a division by zero.
--
-- It becomes `partition`: `[[field, {"kind":"distinct"}], …]`, a finite list of
-- explicit edges that travels with the run. A group's key then carries the
-- `[min, max]` window it selected, and that window IS the predicate a promoted
-- fingerprint matches on, so "what you swept = what you run" holds by
-- construction rather than by three implementations agreeing.
--
-- Stored runs are NOT converted. A width names a lattice, not the windows a
-- particular run actually produced, and their group keys are rendered `"lo–hi"`
-- LABELS that no longer parse — so a conversion would have to invent the windows
-- it claims to preserve. They keep an empty partition and read as one-group-per-
-- value, which is what an unreadable key degrades to honestly. Re-run to promote.
-- ===========================================================================

ALTER TABLE grouped_sweep_runs ADD COLUMN IF NOT EXISTS partition JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE grouped_sweep_runs DROP COLUMN IF EXISTS bucket_width_sol;
