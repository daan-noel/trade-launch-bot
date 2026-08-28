-- Momentum-ignition pump-rider rules calibrated from wallet `trunoest`
-- (ardinRsN1mNYVeoJWTBsWeYeXvuR9UUDGMsCDKpb6AT), analyzed 2026-07-31.
--
-- Analysis: hunter/docs/plans/strategies/wallet-analysis.md, section
-- "`trunoest` - momentum-IGNITION pump-rider, a different family (2026-07-31)".
--
-- NOT the fs2/fs4 dip-reversion family: trunoest enters a ~1-min-old, very hot
-- token on a pullback INSIDE a strong 30s buy wave, one shot per token, and
-- exits with one full sell ~-30% off the since-entry peak once the dump starts.
-- Two things about him we deliberately do NOT copy:
--   * the ignition mechanic (his ~4%-of-vsol buy + 0.0098 SOL tape-painting
--     partly CAUSES the +50% median peak he rides) - we cannot assume our entry
--     triggers the crowd, so these rules must be validated via simulate first;
--   * his missing loss containment (21 bags / 60 SOL sunk) - the unarmed
--     `retrace` doubles as a -28% catastrophe floor from entry, plus a hard
--     `held` time cap and a dump-burst flow exit he effectively uses.
--
-- Knob -> measured value map (all from the 07-22..07-31 window, 245 big buys):
--   time 10..600        age at entry p10 8.8s / med 69s / p90 422s
--   liquidity 45..85    vsol p25 48 / med 60 / p90 84
--   trail(30) >= 10     entry -19.1% (med) off the 30s high, p75 -8.5
--   gross_flow(60)>=50  prior-60s gross med 70 SOL (105 trades / 58 wallets)
--   net_flow(30) >= 3   the BUY-WAVE signature: med +8.3 SOL (dip-reversion
--                       wallets enter into NEGATIVE 30s flow - this gate is
--                       what makes the rule trunoest and not fs2/fs4)
--   net_flow(2) <= 0.5  the micro-pullback: med -0.45, p75 +0.59
--   retrace >= 28       exits med -29.5% off the episode peak (unarmed = also
--                       a -28% floor from entry, since peak seeds at entry)
--   net_flow(2) <= -5   exit: dump-burst confirmation (his pre-sell net2 p25
--                       is -3.1; -5 avoids firing on routine mid-pump wobble)
--   held >= 300         safety cap (his hold p90 is 127s; bags are his defect)
--   buy 1.9556 SOL      his best tier (76% win / +10.3 med); 2.93+ did WORSE.
--                       tru-01 is the impact-aware size (optimal fixed buy
--                       ~sqrt(fixed_per_leg * vsol) ~ 0.3 SOL on this band).
--   no reentry block    omitted = one-shot per token (his literal behavior)
--   max_concurrent 2    his concurrency: avg 1.01, max 2
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-trunoest-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'tru-%';
DELETE FROM fingerprints   WHERE name      LIKE 'tru-%';

-- Broad fingerprint: a fingerprint with zero configured axes never matches
-- (Fingerprint::has_any_criterion), so "match everything" is expressed as one
-- bucket axis with a 1000 SOL bucket width. Creation shape carries no signal
-- for this wallet either - he picks by market state, not launch shape.
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('tru-ALL broad', NULL, 0, 1000.0);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params)
SELECT
  v.rule_name, f.id, 'paper', false, true,
  v.buy, 2, 0,
  jsonb_build_object(
    'entry', jsonb_build_object(
      'm_state', jsonb_build_object(
        'time',      jsonb_build_array(
                       jsonb_build_object('operator','>=','value',10),
                       jsonb_build_object('operator','<=','value',600)),
        'liquidity', jsonb_build_array(
                       jsonb_build_object('operator','>=','value',45),
                       jsonb_build_object('operator','<=','value',85))),
      'm_price_window', jsonb_build_object(
        'window_size_sec', 30,
        'trail', jsonb_build_array(jsonb_build_object('operator','>=','value',10))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 60,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','>=','value',50))),
        jsonb_build_object('window_size_sec', 30,
          'net_flow',   jsonb_build_array(jsonb_build_object('operator','>=','value',3))),
        jsonb_build_object('window_size_sec', 2,
          'net_flow',   jsonb_build_array(jsonb_build_object('operator','<=','value',0.5))))),
    'exit', jsonb_build_object(
      'm_position', jsonb_build_object(
        'retrace', jsonb_build_array(jsonb_build_object('operator','>=','value',28)),
        'held',    jsonb_build_array(jsonb_build_object('operator','>=','value',300))),
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 2,
          'net_flow', jsonb_build_array(jsonb_build_object('operator','<=','value',-5))))))
FROM (VALUES
  --  rule_name                          buy (lamports)
  ('tru-00 trunoest base (his 1.96)',    1955555555),
  ('tru-01 size 0.30 (impact-optimal)',   300000000)
) AS v(rule_name, buy)
CROSS JOIN (SELECT id FROM fingerprints WHERE name = 'tru-ALL broad') f;

COMMIT;

-- ---------------------------------------------------------------------------
-- Verify
-- ---------------------------------------------------------------------------
SELECT r.rule_name,
       r.buy_amount_lamports/1e9 AS buy_sol,
       r.max_concurrent_tokens   AS conc,
       r.params->'entry'->'m_state'->'time'                          AS age_band,
       r.params->'entry'->'m_state'->'liquidity'                     AS liq_band,
       r.params->'entry'->'m_price_window'->'trail'->0->>'value'        AS dip,
       r.params->'entry'->'m_flow_window'                               AS entry_flow,
       r.params->'exit'->'m_position'->'retrace'->0->>'value'           AS trail,
       r.params->'exit'->'m_position'->'held'->0->>'value'              AS held_cap,
       r.params->'exit'->'m_flow_window'->0->'net_flow'->0->>'value'    AS dump_exit
FROM strategy_rules r
WHERE r.rule_name LIKE 'tru-%'
ORDER BY r.rule_name;

-- Arm in paper mode:
--   UPDATE strategy_rules SET is_active = true WHERE rule_name LIKE 'tru-%';
-- Stop:
--   UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'tru-%';
