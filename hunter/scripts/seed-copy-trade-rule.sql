-- Copy one target wallet: buy only on the bonding curve, sell on both venues.
--
-- Shape (pinned by hunter/engine/tests/copy_trade_rule.rs, explained in
-- hunter/docs/plans/strategies/copy-trade-plan.md):
--   trigger  entry_event = m_copy_window.buy_sol on a 1-PRINT window, so his
--            print IS the window and a split buy is two separate fires. NEVER
--            m_copy.* - the lifetime group latches and would fire on every later
--            print of the token.
--   filters  entry = the operator's AND-gate. Floors only on m_state.time: an
--            upper bound on a monotonic metric disarms the token permanently.
--   exit     his sell (same 1-print window, reads the same on curve and AMM)
--            OR a time backstop, so a target who never sells cannot strand a bag.
--   venue    entry is curve-only for free. The token is tracked from creation,
--            and Event::Migrated disarms the rule while leaving an open position
--            to ride the AMM out.
--
-- The target is matched against the address the VENUE credited, so an aggregator
-- router PDA here reads as hundreds of thousands of unrelated people (see the
-- wallet attribution rule in hunter/CLAUDE.md). ONE rule per target.
--
-- THE THRESHOLDS BELOW ARE THIS TARGET'S OWN BEHAVIOUR, NOT A RESULT. They are
-- set so the rule can FIRE on what he actually does; whether firing pays is the
-- sweep's question. 7Kgd... over 08-22..09-02 in PG:
--   * fixed buy presets 0.0494 / 0.0988 / 0.2469 SOL - p50 0.0988. A 0.5 floor
--     would fire on nothing, so the floor is 0.04 = "any buy of his".
--   * entry age p50 5.4 s, p95 36.2 s - a sniper. An age floor of 30 s would
--     drop ~90% of his buys, so there is none.
--   * hold p50 17.3 s, p95 303.3 s - the backstop is 300 s, just past p95.
--   * ONE buy and ONE sell per mint, and ZERO AMM prints in the window: his whole
--     book is on the curve. The AMM sell arm is insurance, not his normal path.
-- The age and depth gates stay authored under `disabled` rather than deleted:
-- parked conditions parse and validate like live ones but nothing compiles them,
-- so the slots stay visible in the editor without gating anything.
--
-- paper, inactive. Re-running replaces the same-named rows.

BEGIN;

DELETE FROM strategy_rules WHERE rule_name = 'copy-7Kgd';
DELETE FROM fingerprints   WHERE name      = 'copy-7Kgd';

-- Wildcard identity: the selectivity of a copy rule is the WALLET, not the
-- token's creation axes. The fingerprint exists to carry the target list.
INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'copy-7Kgd',
  true,
  '{}'::jsonb,
  '{
    "m_copy": { "target_wallets": ["7KgdneuMUaHoFhZULaDq9yLfZSs6zkSzwWwaivvvP3rf"] }
  }'::jsonb
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, tags, params)
SELECT 'copy-7Kgd', f.id, 'paper', false, true,
       100000000, 3, 0, ARRAY['fam:copy','stage:candidate'],
       $json${
  "exclusive": true,
  "priority": 10,
  "reentry": { "cooldown_sec": 0, "max_episodes_per_token": 1 },
  "entry_lock": "slot",
  "entry_event": {
    "m_copy_window": {
      "window_size_prints": 1,
      "buy_sol": [{"operator": ">=", "value": 0.04}]
    }
  },
  "entry": {
    "m_copy": { "sell_count": [{"operator": "=", "value": 0}] }
  },
  "exit": [
    { "m_copy_window": {
      "window_size_prints": 1,
      "sell_sol": [{"operator": ">", "value": 0}]
    } },
    { "m_position": { "held": [{"operator": ">=", "value": 300}] } }
  ],
  "disabled": {
    "entry": {
      "m_state": {
        "time": [{"operator": ">=", "value": 30}],
        "liquidity": [{"operator": ">=", "value": 10}]
      }
    }
  }
}$json$::jsonb
FROM fingerprints f WHERE f.name = 'copy-7Kgd';

COMMIT;

SELECT f.name AS fingerprint, f.id AS fingerprint_id,
       f.metric_config -> 'm_copy' -> 'target_wallets' AS targets,
       r.rule_name, r.id AS rule_id, r.buy_amount_lamports, r.is_active
  FROM fingerprints f
  JOIN strategy_rules r ON r.fingerprint_id = f.id
 WHERE f.name = 'copy-7Kgd';
