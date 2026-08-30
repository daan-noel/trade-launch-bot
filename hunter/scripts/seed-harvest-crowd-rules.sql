-- Crowd-harvest door + two exclusive rules.
--
-- Mapping: hunter/docs/plans/strategies/ix-live-rule.md
-- Compile-pinned: hunter/engine/tests/harvest_crowd_rules.rs
-- Previous Python book: hunter/docs/plans/strategies/ix-crowd-island.md
--
-- Door: create tx contains ATA; init_buy_lamports >= 0.2 SOL;
-- first_slot_buy_lamports >= 0.5 SOL. Working-template list lives here.
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false.
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-harvest-crowd-rules.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'hvt-%';
DELETE FROM fingerprints   WHERE name      LIKE 'hvt-%';

INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'hvt-door',
  false,
  '{
    "create_ata": {"kind": "range", "min": "1", "max": "1"},
    "init_buy_lamports": {"kind": "range", "min": "200000000"},
    "first_slot_buy_lamports": {"kind": "range", "min": "500000000"}
  }'::jsonb,
  '{
    "m_burst_slot": {
      "working_templates": [
        "Axiom Trade|CU|ATA|F",
        "Axiom Trade|CU|ATA|N|F",
        "Photon|CU|ATA|F",
        "Terminal|CU|ATA|F",
        "GMGN Bot|CU|ATA|F",
        "GMGN|CU|ATA|F",
        "Bloom Router|CU|F",
        "Bloom|CU|F"
      ]
    }
  }'::jsonb
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'hvt-a-same-template', f.id, 'paper', false, true,
       100000000, 0, 0, ARRAY['fam:harvest','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 100 },
  "entry": {
    "m_state": { "time": [{"operator": ">=", "value": 20}] },
    "m_flow_window": {
      "window_size_slots": 4,
      "window_lag": 1,
      "buy_count": [{"operator": "=", "value": 0}]
    },
    "m_burst_slot": {
      "working_template": [{"operator": "=", "value": 1}],
      "new_on_mint_wallets": [{"operator": ">=", "value": 1}],
      "pre_slot_liquidity": [{"operator": "<", "value": 16}],
      "pre_print_trail": [{"operator": ">=", "value": 15}],
      "slot_template_count": [{"operator": "=", "value": 1}],
      "template_buy_count": [{"operator": ">=", "value": 2}],
      "template_buy_sol": [
        {"operator": ">=", "value": 0.9},
        {"operator": "<", "value": 4}
      ],
      "template_wallet_count": [{"operator": ">=", "value": 2}]
    }
  },
  "exit": [
    { "m_position": {
        "armed": [{"operator": "=", "value": 1}],
        "retrace": [{"operator": ">=", "value": 18}],
        "arm_above_pct": 10
    } },
    {
      "m_position": { "armed": [{"operator": "=", "value": 0}] },
      "m_flow_window": {
        "window_size_sec": 8,
        "buy_count": [{"operator": "=", "value": 0}]
      }
    }
  ]
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'hvt-door';

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'hvt-b-mixed', f.id, 'paper', false, true,
       100000000, 0, 0, ARRAY['fam:harvest','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 100 },
  "entry": {
    "m_state": { "time": [{"operator": ">=", "value": 20}] },
    "m_flow_window": {
      "window_size_slots": 4,
      "window_lag": 1,
      "buy_count": [{"operator": "=", "value": 0}]
    },
    "m_burst_slot": {
      "working_template": [{"operator": "=", "value": 1}],
      "new_on_mint_wallets": [{"operator": ">=", "value": 1}],
      "pre_slot_liquidity": [{"operator": "<", "value": 16}],
      "pre_print_trail": [{"operator": ">=", "value": 15}],
      "slot_template_count": [{"operator": ">=", "value": 2}],
      "slot_buy_sol": [
        {"operator": ">=", "value": 0.9},
        {"operator": "<", "value": 4}
      ],
      "slot_wallet_count": [{"operator": ">=", "value": 2}]
    }
  },
  "exit": [
    { "m_position": {
        "armed": [{"operator": "=", "value": 1}],
        "retrace": [{"operator": ">=", "value": 18}],
        "arm_above_pct": 10
    } },
    {
      "m_position": { "armed": [{"operator": "=", "value": 0}] },
      "m_flow_window": {
        "window_size_sec": 8,
        "buy_count": [{"operator": "=", "value": 0}]
      }
    }
  ]
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'hvt-door';

COMMIT;

SELECT f.name AS fingerprint, f.id AS fingerprint_id,
       r.rule_name, r.id AS rule_id, r.buy_amount_lamports, r.is_active
  FROM fingerprints f
  JOIN strategy_rules r ON r.fingerprint_id = f.id
 WHERE f.name = 'hvt-door'
 ORDER BY r.rule_name;
