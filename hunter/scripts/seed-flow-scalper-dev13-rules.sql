-- Flow-scalper rules on the DEV-BUY fingerprint (2026-07-28).
--
-- What is new here vs seed-flow-scalper-64hp-rules.sql: that file runs a metric-knob
-- ladder on ONE BROAD fingerprint, on the standing assumption that "creation shape
-- carries no signal once hotness is known". That assumption was tested for token
-- SELECTION (which token a scalper picks) and holds. It was never tested for
-- OUTCOME (how an episode on that token ends), and for outcome it is false:
--
--   On 64hP's 6,515 closed episodes, priced net of the measured 125 bps/leg fee,
--   a creator dev buy of >= 12.8 SOL (net; ~13 SOL gross) scores
--       59.2% win / +7.11 %/ep   (284 eps, 80 mints)
--   against
--       49.1% win / +2.44 %/ep   (6,218 eps, 2,400 mints)
--   below it. The lift survives (a) a mint-level block permutation, p=0.006 on win
--   rate, (b) an untouched 07-26..27 holdout, (c) conditioning on the entry-vsol
--   band - so it is NOT a liquidity proxy - and (d) it beats the small-dev-buy
--   cohort on every single day of the window. Full derivation:
--   docs/plans/strategies/wallet-analysis.md, section "Dev-buy size".
--
-- Why it also fixes the tractability problem: the broad `fs2-ALL` fingerprint arms
-- ~18,000 tokens/day, which is why its matched set is too large to simulate or to
-- trade. `fs3-dev big` arms ~110/day (933 of 107,954 tokens over the 6-day window).
--
-- The dev-buy axis is an INSTANT fingerprint axis (Fingerprint::has_instant_criterion),
-- so it matches synchronously on TokenCreated - no PendingFirstSlot deferral.
--
-- Bucket encoding: the matcher is bucket-membership (grouping::same_bucket), not a
-- threshold, so ">= 12.8 SOL" is spelled as consecutive buckets at width 12.8. Bucket
-- 1 = [12.8, 25.6) holds 586 of the 933 tokens over the window and ALL 284 of 64hP's
-- big-dev-buy episodes; bucket 2 = [25.6, 38.4) is seeded but UNVALIDATED (he has only
-- 13 episodes there).
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-flow-scalper-dev13-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'fs3-%';
DELETE FROM fingerprints   WHERE name      LIKE 'fs3-%';

-- Ids are PINNED, not generated: scripts/flow-scalper-ladder.ps1's `fp13` / `fp13ctl`
-- plans name them in comments so the A/B can be re-run against the same two rows.
INSERT INTO fingerprints (id, name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES
  -- The validated band. 586 tokens / 6 days.
  ('49471cf5-41d5-4929-973a-4bc0afc63754', 'fs3-dev big [12.8-25.6)',  NULL, 12800000000, 12.8),
  -- The exact COMPLEMENT. Kept for reference but DO NOT SIMULATE IT: at ~85k tokens a
  -- 6-day fold dies with `lake trade fetch failed: Out of Memory Error` on a 16 GB
  -- box. Use the two size-matched control bands below instead - they are also a
  -- cleaner A/B, because they hold token count roughly constant.
  ('c9a76f18-9a14-4a5a-83fe-99bd97dea05e', 'fs3-dev small [0-12.8)',   NULL,           0, 12.8),
  -- CONTROL A, the sharp one: the band immediately BELOW the cut (1,269 tokens).
  -- On 64hP's own episodes it scores 48.7% win / +0.89 %/ep - i.e. at baseline - so
  -- if the engine reproduces that while [12.8,25.6) scores ~59%, the lift is
  -- attributable to the fingerprint and not to the dip-25 / liq-40-75 gate changes.
  ('7b2e4c10-5d3a-4f81-9c6e-2a1b0d4e8f37', 'fs3-dev adj [9.6-12.8)',   NULL,  9600000000,  3.2),
  -- CONTROL B, one band further down (1,020 tokens): 55.6% win / +3.57 %/ep for him.
  ('3f9a1d62-8c47-4e05-b1d2-6e7c93a4b508', 'fs3-dev mid [6.4-9.6)',    NULL,  6400000000,  3.2),
  -- Next bucket up. Seeded for completeness; 64hP has only 13 episodes here.
  ('2008cea4-0195-461e-81d1-a372664cbcd8', 'fs3-dev huge [25.6-38.4)', NULL, 25600000000, 12.8);

-- ---------------------------------------------------------------------------
-- Rules. `fs3-00` is the configuration validated by the `fp13`/`fp13b` simulate
-- ladders (scripts/flow-scalper-ladder.ps1); the rest move ONE knob off it.
--
--   dip      m_price_window(30).trail >=      entry dip off the 30 s rolling high
--   retrace  m_position.retrace >=            trailing stop off the since-entry peak
--   arm      m_position.arm_above_pct         profit % before the trail arms (NULL = unarmed)
--   held     m_position.held >=               hard time exit, seconds
--   dead     m_flow_window(30).gross_flow <=  cold-token bailout
--   sl       stop_loss                        floor while the trail is unarmed
--   eps      reentry.max_episodes_per_token   NULL = one-shot
-- ---------------------------------------------------------------------------
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params)
SELECT
  v.rule_name, f.id, 'paper', false, true,
  300000000, 4, 0,
  jsonb_strip_nulls(jsonb_build_object(
    'stop_loss', 12.0,
    'entry', jsonb_build_object(
      'm_state', jsonb_build_object(
        'time',      jsonb_build_array(jsonb_build_object('operator','>=','value',45)),
        'liquidity', jsonb_build_array(
                       jsonb_build_object('operator','>=','value',40),
                       jsonb_build_object('operator','<=','value',75))),
      'm_price_window', jsonb_build_object(
        'window_size_sec', 30,
        'trail', jsonb_build_array(jsonb_build_object('operator','>=','value',v.dip))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 60,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','>=','value',45))),
        jsonb_build_object('window_size_sec', 2,
          'net_flow',   jsonb_build_array(jsonb_build_object('operator','>=','value',0))))),
    'exit', jsonb_build_object(
      'm_position', jsonb_strip_nulls(jsonb_build_object(
        'retrace',       jsonb_build_array(jsonb_build_object('operator','>=','value',7.0)),
        'arm_above_pct', v.arm,
        'held',          jsonb_build_array(jsonb_build_object('operator','>=','value',90)))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 30,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','<=','value',3.0))))),
    'reentry', CASE WHEN v.eps IS NULL THEN NULL
                    ELSE jsonb_build_object('cooldown_sec', 5, 'max_episodes_per_token', v.eps) END))
FROM (VALUES
  --  rule_name                        dip   arm   eps
  -- The validated base: simulate `first` fill / pumpfun_impact / 0.30 SOL / conc 4
  -- over 07-22..07-28 => n=74 eps on 39 mints, 59.5% win, +5.42 %/ep, PF 1.67.
  -- Under the adversarial `worst` fill it stays positive (51.4% win, +3.48 %/ep) -
  -- the first configuration in this project's history that does.
  ('fs3-00 dev13 base',                25.0,  2.0,   40),
  -- Highest measured win rate (66.7%, +11.04 %/ep) but only 39 episodes, and the
  -- mean is right-tail driven. Re-entry was PnL-negative on the broad universe
  -- because it compounds bad picks; on a fingerprint that selects well that should
  -- reverse (64hP's own ep9+ is his best cohort), so this pair is the live test.
  ('fs3-01 one-shot',                  25.0,  2.0, NULL),
  -- Dip bracket. Measured win rate is single-peaked at 25: 18 -> 51.0%,
  -- 20 -> 56.2%, 25 -> 59.5%, 30 -> 58.0%.
  ('fs3-02 dip 20 (loose)',            20.0,  2.0,   40),
  ('fs3-03 dip 30 (tight)',            30.0,  2.0,   40),
  -- 64hP's LITERAL exit (unarmed trail). Included as the negative control: it
  -- collapses to 26.7% win / -0.98 %/ep with a 5 s median hold, because an
  -- unarmed `retrace >= 7` is a hard -7% stop from entry until the price rises
  -- (PositionCtx::at_fill seeds peak = entry). Do NOT arm this one in real mode.
  ('fs3-04 unarmed (negative control)',25.0, NULL,   40)
) AS v(rule_name, dip, arm, eps)
CROSS JOIN (SELECT id FROM fingerprints WHERE name = 'fs3-dev big [12.8-25.6)') f;

COMMIT;

-- ---------------------------------------------------------------------------
-- Verify
-- ---------------------------------------------------------------------------
SELECT r.rule_name,
       fp.name                                                     AS fingerprint,
       r.buy_amount_lamports/1e9                                   AS buy_sol,
       r.params->'entry'->'m_price_window'->'trail'->0->>'value'   AS dip,
       r.params->'exit'->'m_position'->'retrace'->0->>'value'      AS trail,
       r.params->'exit'->'m_position'->>'arm_above_pct'            AS arm,
       r.params->'exit'->'m_position'->'held'->0->>'value'         AS held_cap,
       r.params->'reentry'->>'max_episodes_per_token'              AS max_eps,
       r.params->>'stop_loss'                                      AS stop_loss
FROM strategy_rules r
JOIN fingerprints fp ON fp.id = r.fingerprint_id
WHERE r.rule_name LIKE 'fs3-%'
ORDER BY r.rule_name;

-- Arm the base in paper mode:
--   UPDATE strategy_rules SET is_active = true WHERE rule_name = 'fs3-00 dev13 base';
-- Stop:
--   UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'fs3-%';
