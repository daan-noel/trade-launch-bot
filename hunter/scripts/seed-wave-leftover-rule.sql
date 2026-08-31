-- Sayable leftover of the 8dtx wave event (3-day tape).
-- Event: after a 20-39 slot quiet, the print that makes two new-on-mint
-- wallets in one consecutive-slot wave, with that wave summing 1-4 SOL.
-- Door: first-slot buy 0.5-2 SOL. Exit: clock-20. First per mint.
--
-- Engine group: m_burst_wave (not m_burst_slot, not 19sl@1).
-- Init < 2 including missing is uncut: a range axis fails NULL, and that
-- is a different quantity than omitting the axis.
-- Template KEEP lists are thermometer conjunction, not in this rule.
--
-- paper, inactive. Re-running replaces the same-named row.

BEGIN;

DELETE FROM strategy_rules WHERE rule_name = 'wave-leftover-clock20';
DELETE FROM fingerprints   WHERE name      = 'wave-leftover-door';

INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'wave-leftover-door',
  false,
  '{
    "first_slot_buy_lamports": {"kind": "range", "min": "500000000", "max": "1999999999"}
  }'::jsonb,
  '{}'::jsonb
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'wave-leftover-clock20', f.id, 'paper', false, true,
       100000000, 0, 0, ARRAY['fam:wave','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
  "entry_lock": "slot",
  "entry_event": {
    "m_burst_wave": {
      "this_member": [{"operator": "=", "value": 1}],
      "wallet_count": [{"operator": ">=", "value": 2}],
      "buy_sol": [
        {"operator": ">=", "value": 1},
        {"operator": "<", "value": 4}
      ],
      "gap_slots": [
        {"operator": ">=", "value": 20},
        {"operator": "<=", "value": 39}
      ],
      "all_new": [{"operator": "=", "value": 1}],
      "has_unknown": [{"operator": "=", "value": 0}]
    }
  },
  "exit": {
    "m_position": { "held": [{"operator": ">=", "value": 20}] }
  }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'wave-leftover-door';

COMMIT;

SELECT f.name AS fingerprint, f.id AS fingerprint_id,
       r.rule_name, r.id AS rule_id, r.buy_amount_lamports, r.is_active
  FROM fingerprints f
  JOIN strategy_rules r ON r.fingerprint_id = f.id
 WHERE f.name = 'wave-leftover-door';
