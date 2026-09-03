-- Wave leftover: 2nd Axiom-program buy after a gap, this tip in [1e5, 1e6).
-- Leftover on that same print: wave tx_index hole + tip band already seen.
-- Working list is the Axiom Trade program, not the two CU+ATA grains
-- (mid tips land on Axiom Trade|ATA|F) and not the eight-tool harvest list.
-- First episode per token. Harvest exit DNF. Paper, idle.
--
-- Mapping: hunter/docs/plans/strategies/ix-live-rule.md
-- Compile-pinned: hunter/engine/tests/ax2_midtips_rules.rs
-- Python book: hunter/docs/plans/strategies/ix7-forward.json
--
-- SAFE BY DEFAULT: trade_mode='paper', is_active=false.
-- Idempotent: re-running replaces the same-named rows.
--
-- Run:
--   psql "$DATABASE_URL" -f hunter/scripts/seed-ax2-midtips-rule.sql

BEGIN;

DELETE FROM strategy_rules WHERE rule_name = 'ax2-midtips';
DELETE FROM fingerprints   WHERE name      = 'ax2-midtips-door';

INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'ax2-midtips-door',
  false,
  '{
    "create_ata": {"kind": "range", "min": "1", "max": "1"},
    "init_buy_lamports": {"kind": "range", "min": "200000000"},
    "first_slot_buy_lamports": {"kind": "range", "min": "500000000"}
  }'::jsonb,
  '{
    "m_burst_slot": {
      "working_templates": [],
      "working_programs": ["Axiom Trade"]
    }
  }'::jsonb
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'ax2-midtips', f.id, 'paper', false, true,
       100000000, 0, 0, ARRAY['fam:wave','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
  "entry_lock": "slot",
  "entry_event": {
    "m_burst_wave": {
      "this_member": [{"operator": "=", "value": 1}],
      "this_working": [{"operator": "=", "value": 1}],
      "working_buy_count": [{"operator": "=", "value": 2}],
      "gap_slots": [{"operator": ">=", "value": 2}],
      "this_tip": [
        {"operator": ">=", "value": 100000},
        {"operator": "<", "value": 1000000}
      ]
    }
  },
  "entry": {
    "m_burst_wave": {
      "hole": [{"operator": "=", "value": 1}],
      "tip_seen": [{"operator": "=", "value": 1}]
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
FROM fingerprints f WHERE f.name = 'ax2-midtips-door';

COMMIT;
