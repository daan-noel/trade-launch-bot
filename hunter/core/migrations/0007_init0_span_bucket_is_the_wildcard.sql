-- `init=0 · bkt=1000` IS the wildcard -- spell it as one.
--
-- A 1000 SOL bucket on `init_buy_lamports` puts every token in bucket 0 (the
-- largest dev-buy on record is 85.005 SOL), so the axis discriminates on exactly
-- one thing: whether the token HAS a parsed dev-buy at all. `sol_axis` is
-- `tf_lamports.is_some_and(..)`, so a NULL token value fails a configured axis and
-- passes a wildcard -- 18.4% of tokens (170,550 / 927,708 at the time, running
-- 13-17% per day). That gap is our own parser coverage, not a creation shape, so
-- these rows mean "every token" and now say so.
--
-- Widens the 11 rows below by that 18.4%, two rules on them live at the time
-- (`isl-ab-confirmed`, `isl-b-quiet-pause`). Deliberate.
--
-- Scope is exactly the SPANNING bucket. The other `init=0` rows (`bkt=1.6`, `5`,
-- `6.4`) bound a real band well inside the 85 SOL range and keep their axis. So
-- does `fs_buy=500 · bkt=1000`: a first-slot axis is judged at `MatchPhase::Full`,
-- so making it a wildcard would also move it from arming after the creation slot
-- closes to arming at `TokenCreated` -- a change in WHEN, not just what.

-- One statement: `fingerprints_wildcard_excludes_axes` (0005) requires a wildcard
-- to carry no axis, and `fingerprints_bucket_width_needs_a_sol_axis` (0006)
-- requires no width once the axis is gone. Both are satisfied per row, after.
UPDATE fingerprints SET
    wildcard = TRUE,
    init_buy_lamports = NULL,
    bucket_size_amount = NULL,
    -- `init=0 · bkt=1000` is `auto_name` output, so it re-derives to `ALL`. The
    -- `8dtx · <router>` nicknames are not in that grammar and stay -- each is the
    -- only record of which router its `metric_config` classifies as volume.
    name = CASE WHEN name = 'init=0 ' || U&'\00b7' || ' bkt=1000' THEN 'ALL' ELSE name END,
    updated_at = now()
WHERE NOT wildcard
  AND init_buy_lamports = 0
  AND bucket_size_amount = 1000
  AND cu_limit IS NULL
  AND cu_price IS NULL
  AND ix_labels IS NULL
  AND max_cost_lamports IS NULL
  AND spendable_lamports_in IS NULL
  AND first_slot_buy_lamports IS NULL
  AND first_slot_sell_lamports IS NULL;

-- No row is deleted and no rule moves: every pair that just became match-identical
-- carries a DIFFERENT `metric_config`, which is not identity but IS live (it
-- compiles into `m_flow_split` at reload). The ten routers are the whole point of
-- that column, and the `8dtx-derived` classifier row carries its own
-- `organic_ix_markers`. Collapsing them would retune the rules underneath.
