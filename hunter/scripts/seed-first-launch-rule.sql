-- The first-launch impulse rule.
--
-- Derivation, refutations and every measured number:
--   hunter/docs/history/2026-08-23-3xk2-derived-first-launch-rule.md
-- The same params JSON, validated against the live metric registry:
--   hunter/engine/tests/first_launch_rule.rs  (`RuleParams::parse` - the rule-save gate)
--
--   TOKEN    prior_launches = 0     the creator's first launch in a 30-day window
--   MOMENT   buy_share@30 >= 80     one-sided tape (PERCENT scale, not 0.8)
--            rise@10     >= 150     up 150% in ten seconds
--   EXIT     retrace     >= 30      a 30% trailing stop, and NO stop-loss
--
-- Measured on the extract at a 115 ms fill on BOTH legs, 0.75 SOL, impact charged on
-- the priced reserve:
--
--   panel                 expectancy/trade   SOL/day   days+   concurrency
--   FIT     08-13..17         +0.088809      +30.80     5/5       0.56
--   OUT     08-18..21         +0.073558      +24.48     4/4       0.52
--
-- Out-of-sample bootstrap, clustered by mint, 4000 resamples: 1,331 episodes over
-- 1,288 mints, CI95 [+0.039442, +0.108794], P(>0) = 1.0000. Win rate 34.3%, median
-- -0.14 - a convexity book, carried by its tail.
--
-- EVERY TERM IS NEGATIVE ALONE. `rise@10 >= 150` on its own is -0.0049/trade (1/5 days).
-- Dropping the creator term takes the out-of-sample expectancy from +0.0736 to +0.0023;
-- dropping the rise term, to +0.0024. This is a conjunction, not a ranking of parts -
-- deleting a term does not weaken it, it inverts it.
--
-- THE TRAIL HAS AN INTERIOR OPTIMUM AT 30%. 40% halves the expectancy (+0.0325), 50%
-- crosses zero, 60% is -0.047 on both panels. Every stop-loss tested costs money inside
-- this gate (`stop 3 / trail 5` is -0.0076, 0/4 days), which is why there is none.
--
-- FLAT IN LATENCY: 115 ms -> 235 ms (the bot's p50 -> p90) keeps 99.3% of the edge. A
-- same-slot fill artifact collapses under lag; this does not move.
--
-- SIZE 0.75 SOL. The optimum is 1.5-2.0 and the sign flips at ~3.3; 0.75 keeps ~72% of
-- the peak money with a 4x margin to the flip.
--
-- CONCURRENCY unlimited (0) to reproduce the numbers. At entry, 49% of the time nothing
-- else is open, 32% one, 20% two or more, max 5 - so a cap of 3 costs almost nothing.
-- Set one before arming anything real.
--
-- SIMULATE, NOT SWEEP: `prior_launches` reads the creator ACROSS other tokens, which the
-- lake corpus does not carry. Simulate loads it from `tokens`; the grouped sweep rejects
-- an axis on it rather than scoring every cell on zero trades.
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false. The engine loads
-- `WHERE is_active AND is_enabled`, so nothing fires until you flip is_active.
--
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-first-launch-rule.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'fl-%';
DELETE FROM fingerprints   WHERE name      LIKE 'fl-%';

-- Universe-wide scope: the rule's token filter is `prior_launches`, a metric, not a
-- creation axis. A fingerprint with zero configured axes never matches
-- (`Fingerprint::has_any_criterion`), so "match everything" is one bucket axis with a
-- 1000 SOL bucket width.
INSERT INTO fingerprints (name, ix_labels, init_buy_lamports, bucket_size_amount)
VALUES ('fl-ALL broad', NULL, 0, 1000.0);

-- A first-time creator's token, up 150% in ten seconds, on one-sided flow.
--
-- `time > 5` and `liquidity >= 3` reproduce the study's decision-print filter: every
-- measured decision was on a token at least 5 s old with a real pool behind it. They are
-- part of the tested rule, not garnish.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'fl-first-launch-impulse', f.id, 'paper', false, true,
       750000000, 0, 0, ARRAY['fam:first-launch','stage:candidate'],
       $json${
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}],
      "prior_launches": [{"operator": "=", "value": 0}]
    },
    "m_flow_window": [
      { "window_size_sec": 30, "buy_share": [{"operator": ">=", "value": 80}] }
    ],
    "m_price_window": [
      { "window_size_sec": 10, "rise": [{"operator": ">=", "value": 150}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 30}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'fl-ALL broad';

-- The control: the SAME moment terms with the creator filter removed. Seeded so the
-- claim that the token term carries the rule can be re-measured on demand rather than
-- taken on trust - it books +0.0023/trade out of sample, 3/4 days, against the full
-- rule's +0.0736. Do not arm it.
INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'fl-control-no-creator-term', f.id, 'paper', false, true,
       750000000, 0, 0, ARRAY['fam:first-launch','stage:control'],
       $json${
  "entry": {
    "m_snapshot": {
      "time": [{"operator": ">", "value": 5}],
      "liquidity": [{"operator": ">=", "value": 3}]
    },
    "m_flow_window": [
      { "window_size_sec": 30, "buy_share": [{"operator": ">=", "value": 80}] }
    ],
    "m_price_window": [
      { "window_size_sec": 10, "rise": [{"operator": ">=", "value": 150}] }
    ]
  },
  "exit": { "m_position": { "retrace": [{"operator": ">=", "value": 30}] } }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'fl-ALL broad';

COMMIT;
