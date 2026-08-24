-- The island rules.
--
-- Derivation, mechanisms and every measured number:
--   hunter/docs/plans/strategies/island-map.md
-- The same params JSON, validated against the live metric registry:
--   hunter/engine/tests/island_rules.rs  (`RuleParams::parse` - the rule-save gate)
--
-- ONE rule here is confirmed on the real kernel: `isl-a-continuation`. The other three
-- are seeded ONLY so a refutation can be re-measured, and are marked in their comments.
-- Do not arm them.
--
-- EVERY NUMBER IS BOOKED AT THE BOT'S MEASURED FILL, ON BOTH LEGS. The bot reacts in
-- ~95 ms (808 real fills, p50 94, p90 233, stable across 25 days) and its exit reaction
-- is the same reaction as its entry, so `FillModel::LagMs` lags both. Charging only the
-- entry is what made an earlier version of this file report a working map: on absorption
-- that asymmetry alone is +13.91 -> -8.24 SOL over three days.
--
-- Measured on `hunter-engine::reduce` via POST /api/strategies/simulate, fill
-- {"lag_ms": 95}, cost pumpfun_impact, copycat guard pinned OFF, 0.05 SOL:
--
--   rule                  fit 08-13..15        forward 08-20..21     verdict
--   isl-a-continuation    +2.53 (624, 3/3)     +0.89 (421, 2/2)      CONFIRMED
--   isl-ab-confirmed      +1.16 (182, 3/3)     -0.74 (191, 0/2)      fails forward
--   isl-absorption        -26.88 (4,658)       -                     dead, 0/165 exits
--   isl-b-quiet-pause     -2.62 (585)          -                     not established
--
-- THE EXIT IS A CLOCK, NOT A STOP. Same entry, same trades, same fill:
--   stop 5  + retrace >= 20   ->  -2.56
--   stop 20 + held    >= 40   ->  +2.53
-- A reactive exit fires right after an adverse move and then waits 95 ms, into the
-- continuation of that move; a clock fires at an instant the market did not choose. The
-- wide stop is a disaster brake, not a working part - tightening it to 8 costs 12%
-- in-sample and 51% forward.
--
-- SIZE 0.05 SOL to reproduce. CONCURRENCY unlimited (0) to reproduce; set a cap before
-- arming anything real.
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
-- 1000 SOL bucket width.
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('isl-ALL broad', NULL, 0, 1000.0);

-- REFUTED - engine -26.88 SOL / 3 days on 4,658 trades, and 0 of 165 exit policies
--   positive once the exit leg pays the same 95 ms the entry does. Kept to re-measure.
-- Absorption: a full minute of buyer-dominated tape (84% of SOL is buys) on a live but
--   not-yet-run token. A few large buys eating many small sells. It is the highest-volume
--   entry in the map and the most thoroughly refuted: no exit shape rescues it.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-absorption', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}, {"operator": "<=", "value": 64}],
      "ix_count": [{"operator": "<=", "value": 5}]
    },
    "m_flow_lifetime": { "gross_flow": [{"operator": "<=", "value": 148}] },
    "m_flow_window": [
      { "window_size_sec": 60, "buy_share": [{"operator": ">", "value": 84}] },
      { "window_size_sec": 30, "trade_count": [{"operator": ">", "value": 8}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- Island A - continuation: buy what has ALREADY tripled and is still being bought.
--   The inversion of the refuted impulse island, which required rise(3) <= 9.
--   Anticipating a move needs a reaction the bot does not have; joining one does not.
--   Carries no liquidity ceiling: rise(30) >= 207 already selects a token that has run.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-a-continuation', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:candidate'],
       $json${
  "stop_loss": 20,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      {
        "window_size_sec": 30,
        "net_flow": [{"operator": ">=", "value": 26.9}],
        "buy_share": [{"operator": ">=", "value": 92.1}]
      }
    ],
    "m_price_window": [{ "window_size_sec": 30, "rise": [{"operator": ">=", "value": 207}] }]
  },
  "exit": { "m_position": { "held": [{"operator": ">=", "value": 40}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- NOT ESTABLISHED - engine -2.62 / 3 days; only 58 of 165 exit policies positive.
-- Island B - the quiet pause: a large move, then the tape stops. Two terms, and flat
--   across the lag ladder, but it does not clear cost on the kernel under any exit.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-b-quiet-pause', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      { "window_size_sec": 10, "trade_count": [{"operator": "<=", "value": 22}] }
    ],
    "m_price_window": [{ "window_size_sec": 60, "rise": [{"operator": ">=", "value": 322}] }]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

-- FAILS FORWARD - best in-sample per-trade of any cell (+12.7%/position on 182 trades)
--   and -0.74 SOL, 0/2 days, on cohorts the search never saw. The shape of a selection
--   artifact on a small sample.
-- A and B agreeing - island A's entry narrowed by island B's quiet-tape term. Highest
--   per-trade of anything measured in-sample, and negative on both forward days.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'isl-ab-confirmed', f.id, 'paper', false, true,
       50000000, 0, 0, ARRAY['fam:island','stage:experiment'],
       $json${
  "stop_loss": 5,
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      {
        "window_size_sec": 30,
        "net_flow": [{"operator": ">=", "value": 26.9}],
        "buy_share": [{"operator": ">=", "value": 92.1}]
      },
      { "window_size_sec": 10, "trade_count": [{"operator": "<=", "value": 22}] }
    ],
    "m_price_window": [
      { "window_size_sec": 30, "rise": [{"operator": ">=", "value": 207}] },
      { "window_size_sec": 60, "rise": [{"operator": ">=", "value": 322}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 20}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'isl-ALL broad';

COMMIT;

-- Verify:
--   SELECT rule_name, trade_mode, is_active, buy_amount_lamports,
--          jsonb_pretty(params) FROM strategy_rules
--    WHERE rule_name LIKE 'isl-%' ORDER BY rule_name;
