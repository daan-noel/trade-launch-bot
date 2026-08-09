-- Flow-scalper rules calibrated from wallet 64hP97Bwr5Pubotc... (2026-07-28).
--
-- Supersedes the old seed-flow-scalper-rules.sql (deleted). That file was an
-- omego-calibrated FINGERPRINT A/B; omego's premise is refuted (his gross edge is
-- below the fee - see docs/plans/strategies/flow-scalper-findings.md). This file is
-- a METRIC-KNOB ladder on ONE broad fingerprint, calibrated from the 64hP wallet
-- whose closed-episode edge is +2.54% per SOL cycled NET of the measured 125 bps/leg
-- pump.fun fee.
--
-- Analysis: hunter/docs/plans/strategies/wallet-analysis.md, section "`64hP` - second
-- scalper wallet, same family, better economics (2026-07-28)"; the rule-ladder spec is
-- the "`fs2-*` ladder" section of that same file. Run status + the `fs3-*` ladder that
-- superseded this one: hunter/docs/roadmap/pending-measurement-runs.md.
--
-- Design: ONE fingerprint (broad; creation shape carries no signal once hotness is
-- known) x 9 rules that each move ONE knob off the base, so every comparison is
-- clean. Base = 64hP's own measured behaviour, with two deliberate improvements he
-- does not have:
--   * an ARMED trailing stop (`arm_above_pct`) -- 23.6% of his big winners dipped
--     >7% before peaking, i.e. his unarmed trail cut them short;
--   * a dead-flow bailout -- his single defect is 227 episodes (3.3%) that never
--     sold because the token went cold, costing -225 SOL against a +169 SOL book.
--
-- NOTE on the dead-flow exit: `m_price_lifetime.stall` is NOT "seconds since the
-- last trade" -- it is seconds since the last NEW ALL-TIME HIGH, so on a dip-entry
-- rule it is true by construction and caps position lifetime at the threshold. Do
-- not use it here. The cold-token bailout is expressed as a LOW `gross_flow` exit.
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-flow-scalper-64hp-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'fs2-%';
DELETE FROM fingerprints   WHERE name      LIKE 'fs2-%';

-- Broad fingerprint: a fingerprint with zero configured axes never matches
-- (Fingerprint::has_any_criterion), so "match everything" is expressed as one
-- bucket axis with a 1000 SOL bucket width.
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('fs2-ALL broad', NULL, 0, 1000.0);

-- ---------------------------------------------------------------------------
-- Rules. Each row changes ONE knob vs `fs2-00 base` (marked in the comment).
--
--   dip      m_price_window(30).trail >=   entry dip off the 30 s rolling high
--   liq_lo/hi m_snapshot.liquidity band    vsol at entry
--   gross60  m_flow_window(60).gross_flow  hotness floor
--   retrace  m_position.retrace >=         trailing stop off since-entry peak
--   arm      m_position.arm_above_pct      profit % before the trail arms (NULL = unarmed)
--   held     m_position.held >=            hard time exit, seconds (NULL = none)
--   dead     m_flow_window(30).gross_flow <=  cold-token bailout (NULL = none)
--   sl       stop_loss                     floor while the trail is unarmed
--   buy      buy_amount_lamports
-- ---------------------------------------------------------------------------
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params)
SELECT
  v.rule_name, f.id, 'paper', false, true,
  v.buy, 4, 0,
  jsonb_strip_nulls(jsonb_build_object(
    'stop_loss', v.sl,
    'entry', jsonb_build_object(
      'm_snapshot', jsonb_build_object(
        'time',      jsonb_build_array(jsonb_build_object('operator','>=','value',30)),
        'liquidity', jsonb_build_array(
                       jsonb_build_object('operator','>=','value',v.liq_lo),
                       jsonb_build_object('operator','<=','value',v.liq_hi))),
      'm_price_window', jsonb_build_object(
        'window_size_sec', 30,
        'trail', jsonb_build_array(jsonb_build_object('operator','>=','value',v.dip))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 60,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','>=','value',v.gross60))),
        jsonb_build_object('window_size_sec', 2,
          'net_flow',   jsonb_build_array(jsonb_build_object('operator','>=','value',0))))),
    'exit', jsonb_build_object(
      'm_position', jsonb_strip_nulls(jsonb_build_object(
        'retrace',        jsonb_build_array(jsonb_build_object('operator','>=','value',v.retrace)),
        'arm_above_pct',  v.arm,
        'held',           CASE WHEN v.held IS NULL THEN NULL
                               ELSE jsonb_build_array(jsonb_build_object('operator','>=','value',v.held)) END)),
      'm_flow_window', CASE WHEN v.dead IS NULL THEN NULL ELSE jsonb_build_array(
        jsonb_build_object('window_size_sec', 30,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','<=','value',v.dead)))) END),
    'reentry', jsonb_build_object('cooldown_sec', 5, 'max_episodes_per_token', 20)))
FROM (VALUES
  --  rule_name                  dip  liq_lo liq_hi gross60 retrace  arm  held  dead   sl    buy
  ('fs2-00 base',                18.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  300000000),
  -- entry dip: his >-8% bucket is his ONLY losing cohort; -25..-40 is his best
  ('fs2-01 dip 12 (loose)',      12.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  300000000),
  ('fs2-02 dip 25 (his best)',   25.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  300000000),
  -- trailing stop width: his measured trail is 6.8%
  ('fs2-03 trail 4 (tight)',     18.0,  36.0,  70.0,  45.0,   4.0,  2.0,   90,  3.0,  12.0,  300000000),
  ('fs2-04 trail 11 (wide)',     18.0,  36.0,  70.0,  45.0,  11.0,  2.0,   90,  3.0,  12.0,  300000000),
  -- his literal behaviour: unarmed trail, no time cap, no dead-flow bailout
  ('fs2-05 unarmed (his exit)',  18.0,  36.0,  70.0,  45.0,   7.0, NULL,   90,  3.0,  12.0,  300000000),
  ('fs2-06 no time cap',         18.0,  36.0,  70.0,  45.0,   7.0,  2.0, NULL,  3.0,  12.0,  300000000),
  ('fs2-07 no dead-flow exit',   18.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90, NULL,  12.0,  300000000),
  -- liquidity: 40-55 was his sweet spot, 55-75 his worst
  ('fs2-08 liq wide 30-110',     18.0,  30.0, 110.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  300000000),
  -- hotness floor: his p25 gross60 is 45.6 SOL, median 85
  ('fs2-09 gross60 20 (loose)',  18.0,  36.0,  70.0,  20.0,   7.0,  2.0,   90,  3.0,  12.0,  300000000),
  -- size: 64hP sizes at 1.859% of vsol capped at 1.5 SOL gross (=> ~0.8 SOL in this
  -- band). The impact-aware cost model puts the optimal FIXED size far lower, so
  -- this is the single most important knob to sweep. pct-of-vsol sizing is not
  -- implemented (flow-scalper impl Phase 5 deferred).
  ('fs2-10 size 0.10 SOL',       18.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  100000000),
  ('fs2-11 size 0.80 SOL (his)', 18.0,  36.0,  70.0,  45.0,   7.0,  2.0,   90,  3.0,  12.0,  800000000)
) AS v(rule_name, dip, liq_lo, liq_hi, gross60, retrace, arm, held, dead, sl, buy)
CROSS JOIN (SELECT id FROM fingerprints WHERE name = 'fs2-ALL broad') f;

COMMIT;

-- ---------------------------------------------------------------------------
-- Verify
-- ---------------------------------------------------------------------------
SELECT r.rule_name,
       r.buy_amount_lamports/1e9 AS buy_sol,
       r.params->'entry'->'m_price_window'->'trail'->0->>'value'   AS dip,
       r.params->'entry'->'m_snapshot'->'liquidity'                AS liq_band,
       r.params->'exit'->'m_position'->'retrace'->0->>'value'      AS trail,
       r.params->'exit'->'m_position'->>'arm_above_pct'            AS arm,
       r.params->'exit'->'m_position'->'held'->0->>'value'         AS held_cap,
       r.params->'exit'->'m_flow_window'->0->'gross_flow'->0->>'value' AS dead_flow,
       r.params->>'stop_loss'                                      AS stop_loss
FROM strategy_rules r
WHERE r.rule_name LIKE 'fs2-%'
ORDER BY r.rule_name;

-- Arm the whole ladder in paper mode:
--   UPDATE strategy_rules SET is_active = true WHERE rule_name LIKE 'fs2-%';
-- Stop:
--   UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'fs2-%';
