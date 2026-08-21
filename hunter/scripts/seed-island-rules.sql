-- The island rules - four separate rules, one per island plus the conjunction.
--
-- Derivation, mechanisms and every measured number:
--   hunter/docs/plans/strategies/island-map.md
-- The same params JSON, validated against the live metric registry:
--   hunter/engine/tests/island_rules.rs  (`RuleParams::parse` - the rule-save gate)
--
-- Found by partitioning the WHOLE decision-point space (7.75M points, 66k mints,
-- 7 creation-day cohorts 2026-08-13..19), not by fitting a wallet. Three conditions
-- say WHERE to stand and are shared by every rule:
--     liquidity <= 64        the token has not already run (graduation sits near 85)
--     ix_count  <= 5         complex launches lose their edge at a +1 slot fill
--     gross_flow(life) <= 148  the move has not happened yet
-- The islands are WHEN to buy - the same event (demand arriving at a token that has
-- not been re-priced) read at 60s, 30s and 0.4s.
--
-- Measured at NextSlotFirst fills, 0.05 SOL, one episode per mint, virtual-reserve
-- impact, fitted on 08-13..16 and confirmed once on 08-17..19:
--
--   rule                 trades/day   week SOL   net/trade   days +
--   isl-1-absorption          1,477     +46.34      +8.96%      7/7
--   isl-2-quiet-accum           236     +10.01     +12.09%      7/7
--   isl-3-impulse             2,368     +48.71      +5.88%      7/7
--   isl-1and3-confirmed         693     +31.71     +13.06%      7/7
--
-- EXIT is `stop_loss 3` + `m_position.retrace >= 20` on all four, and there is
-- deliberately NO take-profit: the book is paid by a right tail (4-5% of episodes
-- above +100%) that a static TP removes. The exit surface is a plateau - all 40 cells
-- of stop 2..6 x trail 15..25 are positive on all seven days - so no threshold here is
-- load-bearing. Per-island exit re-fitting was tried and LOSES out of sample.
--
-- SIZE 0.05 SOL to reproduce the numbers above. Net per-trade peaks at 0.10 SOL
-- (the `sqrt(fixed_per_leg * vsol)` optimum), so raise it once the shape is confirmed.
--
-- CONCURRENCY unlimited (0) to reproduce the measurements. Live, a cap of 20 keeps 95%
-- of the union's edge and `isl-1and3-confirmed` is cap-proof at 5 (790 of its 792
-- daily trades). Set a cap before arming anything real.
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-island-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'isl-%';
DELETE FROM fingerprints   WHERE name      LIKE 'isl-%';

-- Universe-wide scope. A fingerprint with zero configured axes never matches
-- (`Fingerprint::has_any_criterion`), so "match everything" is one bucket axis with a
-- 1000 SOL bucket width - the same shape the trunoest seed uses. The token filter these
-- rules DO want is `m_snapshot.ix_count <= 5`, which is an entry condition and spans
-- many fingerprints, so it cannot be expressed as a fingerprint scope.
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('isl-ALL broad', NULL, 0, 1000.0);

-- Island 1 - absorption: a full minute of buyer-dominated tape (84% of SOL is buys)
--   on a live but not-yet-run token. A few large buys eating many small sells.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-1-absorption', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "unique_wallets": [{"operator": ">", "value": 6}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- Island 2 - quiet accumulation: real net inflow (>6.5 SOL / 30s) while almost
--   nobody is trading (<=6 distinct wallets). The purest form of the same signal.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-2-quiet-accum', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 75}] },
      {
        "window_size_sec": 30,
        "unique_wallets": [{"operator": "<=", "value": 6}],
        "net_flow": [{"operator": ">", "value": 6.5}]
      }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- Island 3 - impulse inception: a one-slot buy impulse, entered before price moves.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-3-impulse', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_window": [{ "window_size_sec": 0.4, "net_flow": [{"operator": ">=", "value": 0.5}] }],
    "m_price_window": [{ "window_size_sec": 3, "rise": [{"operator": "<=", "value": 9}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- Islands 1 AND 3 agreeing - the highest-quality, most cap-proof reading.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-1and3-confirmed', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 3,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "unique_wallets": [{"operator": ">", "value": 6}] },
      { "window_size_sec": 0.4, "net_flow": [{"operator": ">=", "value": 0.5}] }
    ],
    "m_price_window": [{ "window_size_sec": 3, "rise": [{"operator": "<=", "value": 9}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

COMMIT;

-- Verify:
--   SELECT rule_name, trade_mode, is_active, buy_amount_lamports,
--          jsonb_pretty(params) FROM strategy_rules
--    WHERE rule_name LIKE 'isl-%' ORDER BY rule_name;
