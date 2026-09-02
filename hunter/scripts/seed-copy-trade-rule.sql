-- Copy one target wallet: buy only on the bonding curve, sell on both venues.
--
-- Shape (pinned by hunter/engine/tests/copy_trade_rule.rs, explained in
-- hunter/docs/plans/strategies/copy-trade-plan.md):
--   trigger  entry_event = m_copy_window.buy_sol on a 1-PRINT window, so his
--            print IS the window and a split buy is two separate fires. NEVER
--            m_copy.* — the lifetime group latches and would fire on every later
--            print of the token.
--   filters  entry = the operator's AND-gate. Floors only on m_state.time: an
--            upper bound on a monotonic metric disarms the token permanently.
--   exit     his sell (same 1-print window, reads the same on curve and AMM)
--            OR a time backstop, so a target who never sells cannot strand a bag.
--   venue    entry is curve-only for free. The token is tracked from creation,
--            and Event::Migrated disarms the rule while leaving an open position
--            to ride the AMM out.
--
-- BEFORE RUNNING: put the target's own wallet in m_copy.target_wallets. It is
-- matched against the address the VENUE credited, so an aggregator router PDA
-- there reads as hundreds of thousands of unrelated people (see the wallet
-- attribution rule in hunter/CLAUDE.md). ONE rule per target.
--
-- paper, inactive. Re-running replaces the same-named rows.

BEGIN;

DELETE FROM strategy_rules WHERE rule_name = 'copy-target-a';
DELETE FROM fingerprints   WHERE name      = 'copy-target-a';

-- Wildcard identity: the selectivity of a copy rule is the WALLET, not the
-- token's creation axes. The fingerprint exists to carry the target list.
INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'copy-target-a',
  true,
  '{}'::jsonb,
  '{
    "m_copy": { "target_wallets": ["PUT_THE_TARGET_WALLET_ADDRESS_HERE"] }
  }'::jsonb
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'copy-target-a', f.id, 'paper', false, true,
       100000000, 3, 0, ARRAY['fam:copy','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
  "entry_lock": "slot",
  "entry_event": {
    "m_copy_window": {
      "window_size_prints": 1,
      "buy_sol": [{"operator": ">=", "value": 0.5}]
    }
  },
  "entry": {
    "m_state": {
      "time": [{"operator": ">=", "value": 30}],
      "liquidity": [{"operator": ">=", "value": 10}]
    },
    "m_copy": { "sell_count": [{"operator": "=", "value": 0}] }
  },
  "exit": [
    { "m_copy_window": {
      "window_size_prints": 1,
      "sell_sol": [{"operator": ">", "value": 0}]
    } },
    { "m_position": { "held": [{"operator": ">=", "value": 600}] } }
  ]
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'copy-target-a';

COMMIT;

SELECT f.name AS fingerprint, f.id AS fingerprint_id,
       f.metric_config -> 'm_copy' -> 'target_wallets' AS targets,
       r.rule_name, r.id AS rule_id, r.buy_amount_lamports, r.is_active
  FROM fingerprints f
  JOIN strategy_rules r ON r.fingerprint_id = f.id
 WHERE f.name = 'copy-target-a';
