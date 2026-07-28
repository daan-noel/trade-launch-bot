-- Seed the flow-scalper fingerprint experiment: 6 fingerprints x 1 base rule each.
--
-- Purpose: isolate whether omego's ix_labels groups carry any tradeable signal.
-- Every rule shares ONE metric core (calibrated per group from his own entries) so
-- the only difference between rules is the fingerprint -> a clean A/B.
--
-- Analysis behind the numbers:
--   hunter/docs/roadmap/flow-reversion-scalper.md  ("Token filtering" + "Fingerprint-axis grouping")
-- Rule spec + how to run:
--   hunter/docs/roadmap/flow-scalper-fingerprint-rules.md
--
-- !! STALE EXIT (2026-07-28) -- do not run this as-is for a real experiment.
-- The `m_position.retrace >= 5` below is UNARMED: m_position's peak seeds at the
-- entry fill, so before any run-up `retrace` measures the drop from entry and this
-- is really a hard -5% stop. Against a rule that enters ON a 14% dip it stops out on
-- the continuation (measured: median hold 1.0 s, median -2.2%). Replayed over
-- omego's own 2,974 episodes an unarmed trail turns 21% of his winners into losers.
-- The fix is the `m_position.arm_above_pct` strict param shipped 2026-07-28; see
-- "Exit-shape correction" in flow-scalper-fingerprint-rules.md and the reference
-- hunter/docs/plans/strategies/armed-trailing-stop.md. The per-group liquidity /
-- gross-flow floors below are also pre-recalibration (top-5 IX sequences now cover
-- only ~71% of his entries) -- re-derive them before trusting the A/B.
--
-- SAFE BY DEFAULT: every rule is inserted with trade_mode='paper' and is_active=false.
-- The engine loads `WHERE is_active AND is_enabled` (core/src/storage/repositories/
-- rule_repo.rs), so nothing fires until you flip is_active yourself.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-flow-scalper-rules.sql

BEGIN;

-- ---------------------------------------------------------------------------
-- 1. Fingerprints
-- ---------------------------------------------------------------------------
DELETE FROM strategy_rules  WHERE rule_name LIKE 'fs-%';
DELETE FROM fingerprints    WHERE name      LIKE 'fs-%';

INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount) VALUES
  -- CONTROL: matches every token that has any dev-buy under 1000 SOL. A fingerprint
  -- with zero configured axes never matches (Fingerprint::has_any_criterion), so the
  -- broad match is expressed as one bucket axis with a 1000 SOL bucket width.
  ('fs-ALL  control (broad)', NULL, 0, 1000.0),

  -- IX1 .. IX5: exact ordered creation instruction-label sequences, ranked by his
  -- entry count. Together they cover 83% of his 1,013 buys.
  ('fs-IX1  BuyV2 tool-create', ARRAY[
      'Compute Budget: SetComputeUnitLimit',
      'Compute Budget: SetComputeUnitPrice',
      'Pump.Fun: Create_v2',
      'Associated Token: CreateIdempotent',
      'Pump.Fun: BuyV2'], NULL, 0.1),

  ('fs-IX2  Buy + 1 transfer', ARRAY[
      'Compute Budget: SetComputeUnitLimit',
      'Compute Budget: SetComputeUnitPrice',
      'Pump.Fun: Create_v2',
      'Associated Token: CreateIdempotent',
      'Pump.Fun: Buy',
      'System Program: Transfer'], NULL, 0.1),

  ('fs-IX3  ExtendAccount BuyExactSolIn', ARRAY[
      'System Program: Transfer',
      'System Program: Transfer',
      'Pump.Fun: Create_v2',
      'Pump.Fun: ExtendAccount',
      'Associated Token: CreateIdempotent',
      'Pump.Fun: BuyExactSolIn'], NULL, 0.1),

  ('fs-IX4  Buy + 2 transfers', ARRAY[
      'Compute Budget: SetComputeUnitLimit',
      'Compute Budget: SetComputeUnitPrice',
      'Pump.Fun: Create_v2',
      'Associated Token: CreateIdempotent',
      'Pump.Fun: Buy',
      'System Program: Transfer',
      'System Program: Transfer'], NULL, 0.1),

  ('fs-IX5  BuyExactQuoteInV2', ARRAY[
      'System Program: Transfer',
      'System Program: Transfer',
      'Pump.Fun: Create_v2',
      'Associated Token: CreateIdempotent',
      'Pump.Fun: BuyExactQuoteInV2'], NULL, 0.1);

-- ---------------------------------------------------------------------------
-- 2. Rules -- one BASE rule per fingerprint.
--
-- Per-group liquidity band and gross-flow floor come from the p25/p90 of omego's
-- own entries inside that group (hunter/docs/roadmap/flow-reversion-scalper.md);
-- everything else is identical so the fingerprint is the only variable.
-- ---------------------------------------------------------------------------
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params)
SELECT
  v.rule_name,
  f.id,
  'paper',
  false,          -- <- flip to true to arm
  true,
  1000000000,     -- 1.0 SOL; for a first REAL run drop to 100000000 (0.1 SOL)
  4,              -- his p90 concurrency is 4 (max 6)
  0,              -- uncapped total
  jsonb_build_object(
    'stop_loss', 25,
    'entry', jsonb_build_object(
      'm_snapshot', jsonb_build_object(
        'time',      jsonb_build_array(jsonb_build_object('operator','>=','value',150)),
        'liquidity', jsonb_build_array(
                       jsonb_build_object('operator','>=','value',v.liq_lo),
                       jsonb_build_object('operator','<=','value',v.liq_hi))),
      'm_price_window', jsonb_build_object(
        'window_size_sec', 30,
        'trail', jsonb_build_array(jsonb_build_object('operator','>=','value',v.dip))),
      -- multi-window: hot gate (60 s gross) AND sell-exhaustion floor (2 s net)
      'm_flow_window', jsonb_build_array(
        jsonb_build_object('window_size_sec', 60,
          'gross_flow', jsonb_build_array(jsonb_build_object('operator','>=','value',v.gross))),
        jsonb_build_object('window_size_sec', 2,
          'net_flow',   jsonb_build_array(jsonb_build_object('operator','>=','value',0))))),
    'exit', jsonb_build_object(
      'm_position',       jsonb_build_object(
        'retrace', jsonb_build_array(jsonb_build_object('operator','>=','value',5))),
      'm_price_lifetime', jsonb_build_object(
        'stall',   jsonb_build_array(jsonb_build_object('operator','>=','value',15)))),
    'reentry', jsonb_build_object('cooldown_sec', 5, 'max_episodes_per_token', 12))
FROM (VALUES
  --  rule_name                    fp name prefix   liq_lo liq_hi  dip  gross60
  ('fs-ALL base', 'fs-ALL',   55.0, 105.0, 14.0, 14.0),
  ('fs-IX1 base', 'fs-IX1',   50.0,  90.0, 12.0,  8.0),
  ('fs-IX2 base', 'fs-IX2',   55.0, 105.0, 15.0, 15.0),
  ('fs-IX3 base', 'fs-IX3',   55.0, 105.0, 15.0, 16.0),
  ('fs-IX4 base', 'fs-IX4',   48.0, 100.0, 18.0, 15.0),
  ('fs-IX5 base', 'fs-IX5',   58.0,  95.0, 13.0, 12.0)
) AS v(rule_name, fp_prefix, liq_lo, liq_hi, dip, gross)
JOIN fingerprints f ON f.name LIKE v.fp_prefix || '%';

COMMIT;

-- ---------------------------------------------------------------------------
-- Verify
-- ---------------------------------------------------------------------------
SELECT r.rule_name, f.name AS fingerprint, r.trade_mode, r.is_active,
       r.buy_amount_lamports/1e9 AS buy_sol,
       r.params->'entry'->'m_snapshot'->'liquidity' AS liquidity_band,
       r.params->'entry'->'m_price_window'->'trail' AS dip
FROM strategy_rules r JOIN fingerprints f ON f.id = r.fingerprint_id
WHERE r.rule_name LIKE 'fs-%'
ORDER BY r.rule_name;

-- To arm the whole experiment in paper mode:
--   UPDATE strategy_rules SET is_active = true WHERE rule_name LIKE 'fs-%';
-- To stop:
--   UPDATE strategy_rules SET is_active = false WHERE rule_name LIKE 'fs-%';
