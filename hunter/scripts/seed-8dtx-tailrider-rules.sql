-- Right-tail rider rules calibrated from wallet `8dtx2tr4`
-- (8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF), analyzed 2026-08-04 on the
-- local PG window 2026-07-22 18:47 .. 08-04 12:25 (12.7d, 17.2M trades,
-- 196,119 mints). His slice: 5,673 legs / 2,399 mints / 2,847 episodes.
--
-- A FOURTH family, distinct from every wallet seeded so far:
--   * fs2/fs4 (omego, 64hP)  = dip-reversion scalpers, tight ~3% trail, re-entry
--   * tru-*  (trunoest)      = momentum-ignition pump-rider, buy-wave gate
--   * w8d-*  (this file)     = one-shot right-tail rider: a deliberately DUMB
--                              entry filter and a WIDE trail that does all the work
--
-- Measured profile: 34.3% win, +122.8 SOL gross on 2,166 SOL cycled
-- (+5.66%/round trip vs a ~2.5% round-trip fee), every one of 13 full days
-- positive. Net after 125bps/leg fees (56.8 SOL) + tips ~= +4.2..4.7 SOL/day.
--
-- Knob -> measured value map:
--   liquidity 32..47    entry vsol band. He is measured POST-trade at 33..48
--                       (97.1% of entries; sharp floor at 31, cliff at 48); the
--                       engine gates on PRE-trade state, so the band is shifted
--                       down by his own ~0.657 SOL buy. THIS IS THE ONLY REAL
--                       TOKEN FILTER HE HAS.
--   time >= 15          age p10 17.4s / med 89.8s / p90 620s. Floor only - he
--                       has no upper age bound and 25% of his money comes from
--                       entries older than 4 min.
--   gross_flow(60)>=10  prior-60s gross p25 10.19 / med 26.8 SOL ("is it alive").
--   NO price-geometry gate  <-- DELIBERATE, and the load-bearing finding. Split
--                       by entry dip vs the 30s high, all four buckets are
--                       profitable with near-identical avg return (4.6-7.1%) and
--                       win rate (30-37%); same across every 5s net-flow regime.
--                       He times neither dips nor breakouts. Adding a `trail`
--                       entry gate here (as every other seed rule in this repo
--                       has) would NOT be copying him - it would be inventing a
--                       filter the data says is noise. At his entry moments only
--                       ~3 other tokens qualify, so selection is capacity-limited,
--                       not clever.
--   retrace >= 20       his winners exit med -21.9% off the since-entry peak
--                       (p25 -28.8 / p75 -15.5); losers -15.2%. Left UNARMED on
--                       purpose: the peak seeds at the entry fill, so this
--                       doubles as a ~-20% hard floor from entry, which matches
--                       his loss containment (p10 -14.4%; only 62 of 2,773 closed
--                       episodes finish worse than -20%).
--   held >= 600         safety cap. His hold p99 is 626s, and he strands 74 bags
--                       (55.5 SOL) that he simply never sells - we do NOT copy
--                       that defect.
--   buy 0.657 SOL       near-fixed size (p10 = p25 = 0.657, med 0.691; 35% of his
--                       buys land in 0.65-0.66). NOT pct-of-vsol - his pct spread
--                       is 1.47-2.72%, the opposite of 64hP/omego.
--   max_concurrent 1    his concurrency is median 1 / p90 1 / max 4. One at a
--                       time IS the strategy's ration, not a safety knob - the
--                       whole result comes off ~1-3 SOL of working capital.
--   no reentry block    omitted = one-shot per token (1,808/2,399 of his mints
--                       are exactly 1 buy -> 1 sell; max 3 buys ever).
--
-- READ THIS BEFORE ARMING: the edge is ENTIRELY tail. His top 100 episodes
-- (3.6%) made +136.1 SOL while the other 2,673 LOST 13.3 SOL in aggregate.
-- Episodes that peaked only +10..25% still lose money after cost. That means
-- (a) a paper run of a few hundred episodes can easily show a loss and still be
-- faithful, and (b) fees eat ~half the gross, so a 40% shortfall in the tail is
-- break-even. Validate via simulate with CostModelKind::pumpfun_impact before
-- ever considering trade_mode='real'.
--
-- Analysis source: hunter/docs/plans/strategies/wallet-analysis.md
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-8dtx-tailrider-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'w8d-%';
DELETE FROM fingerprints   WHERE name      LIKE 'w8d-%';

-- Broad fingerprint: a fingerprint with zero configured axes never matches
-- (Fingerprint::has_any_criterion), so "match everything" is expressed as one
-- bucket axis with a 1000 SOL bucket width. Creation shape is irrelevant here -
-- he selects purely on live curve state, and all 2,399 of his mints sit in the
-- 105,990-token pool that ever reaches vsol >= 33 (2.26% mint-level precision).
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('w8d-ALL broad', NULL, 0, 1000.0);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT
  v.rule_name, f.id, 'paper', false, true,
  v.buy, 1, 0,
  ARRAY['wallet-copy', 'tail-rider', 'w8dtx']::text[],
  jsonb_build_object(
    'entry', jsonb_build_object(
      'm_snapshot', jsonb_build_object(
        'time',      jsonb_build_array(
                       jsonb_build_object('operator','>=','value',15)),
        'liquidity', jsonb_build_array(
                       jsonb_build_object('operator','>=','value',32),
                       jsonb_build_object('operator','<=','value',47))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 60,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','>=','value',10))))),
    'exit', jsonb_build_object(
      'm_position', jsonb_build_object(
        'retrace', jsonb_build_array(jsonb_build_object('operator','>=','value',v.trail)),
        'held',    jsonb_build_array(jsonb_build_object('operator','>=','value',600)))))
FROM (VALUES
  --  rule_name                                buy (lamports)  trail %
  ('w8d-00 8dtx base (his 0.657 / trail 20)',      657000000,   20),
  ('w8d-01 size 0.28 (impact-optimal)',            280000000,   20),
  ('w8d-02 tight trail 12 (giveback probe)',       657000000,   12)
) AS v(rule_name, buy, trail)
CROSS JOIN (SELECT id FROM fingerprints WHERE name = 'w8d-ALL broad') f;

COMMIT;

-- ---------------------------------------------------------------------------
-- Verify
-- ---------------------------------------------------------------------------
SELECT r.rule_name,
       r.trade_mode,
       r.is_active,
       r.buy_amount_lamports/1e9                                     AS buy_sol,
       r.max_concurrent_tokens                                       AS conc,
       r.params->'entry'->'m_snapshot'->'liquidity'                  AS vsol_band,
       r.params->'entry'->'m_snapshot'->'time'->0->>'value'          AS min_age_s,
       r.params->'entry'->'m_flow_window'->0->'gross_flow'->0->>'value' AS gross60,
       r.params->'exit'->'m_position'->'retrace'->0->>'value'        AS trail_pct,
       r.params->'exit'->'m_position'->'held'->0->>'value'           AS held_cap_s,
       r.tags
FROM strategy_rules r
WHERE r.rule_name LIKE 'w8d-%'
ORDER BY r.rule_name;

-- Arm in paper mode:
--   UPDATE strategy_rules SET is_active = true WHERE rule_name LIKE 'w8d-%';
-- Stop:
--   UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'w8d-%';
