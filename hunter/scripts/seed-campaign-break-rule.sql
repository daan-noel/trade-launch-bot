-- Campaign-break rule v0 (paper) - the derivation and holdout are in
-- docs/plans/strategies/campaign-break-money.md.
--
-- Door      : the campaign machine's exact ordered build, as an `ix_patterns`
--             classifier on a WILDCARD fingerprint (the machine, not the token,
--             is the door). Contagion and creator-tagging OFF: the claim is
--             structural - THIS print is that build - not "this wallet once was".
-- Event     : that build's buy ends a >= 10-slot buy silence.
-- Permission: curve in the committed band, the machine has worked this token
--             before, the tape is not a buying climax, price off its 30-min high.
-- Exit      : +36% net (a +40% price move after costs) or a 600 s clock. No stop
--             (stops lose to no-stop on this tape), re-entry on the next break.
BEGIN;

DELETE FROM strategy_rules WHERE rule_name LIKE 'campaign-break-%';
DELETE FROM fingerprints   WHERE name      LIKE 'campaign-break-%';

INSERT INTO fingerprints (name, wildcard, criteria, metric_config)
VALUES (
  'campaign-break-29d9aacb',
  true,
  '{}'::jsonb,
  jsonb_build_object(
    'm_flow_ix', jsonb_build_object(
      'ix_patterns', jsonb_build_array(
        jsonb_build_array(
          'Compute Budget: SetComputeUnitPrice',
          'Compute Budget: SetComputeUnitLimit',
          'Associated Token: CreateIdempotent',
          'Pump.Fun: Buy'
        )
      ),
      'wallet_contagion',  false,
      'creator_is_tagged', false
    )
  )
);

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params
)
SELECT
  'campaign-break-v0', f.id, 'paper', false, true,
  50000000,          -- 0.05 SOL, the clip the study is priced at
  0,                 -- unlimited concurrent tokens
  0,
  jsonb_build_object(
    'entry', jsonb_build_object(
      -- the machine has worked this token before (lifetime tagged buys)
      'm_flow_ix', jsonb_build_object(
        'tagged_buy_count', jsonb_build_array(
          jsonb_build_object('operator', '>=', 'value', 15))),
      -- curve committed but below the wall: vsol 65..100 = real reserve 35..70
      'm_state', jsonb_build_object(
        'liquidity', jsonb_build_array(
          jsonb_build_object('operator', '>=', 'value', 35),
          jsonb_build_object('operator', '<',  'value', 70))),
      -- not a buying climax: the 120 s tape before this print is not buy-heavy
      'm_flow_window', jsonb_build_object(
        'window_size_sec', 120,
        'window_lag', 1,
        'buy_share', jsonb_build_array(
          jsonb_build_object('operator', '<=', 'value', 60))),
      -- off the 30-min high: vsol < 97% of max is price < 94.1% => trail > 5.9%
      'm_price_window', jsonb_build_object(
        'window_size_sec', 1800,
        'trail', jsonb_build_array(
          jsonb_build_object('operator', '>', 'value', 5.9)))
    ),
    'entry_event', jsonb_build_object(
      -- THIS print is the machine's build
      'm_flow_ix_window', jsonb_build_object(
        'window_size_prints', 1,
        'tagged_buy_count', jsonb_build_array(
          jsonb_build_object('operator', '>=', 'value', 1))),
      -- and it ends a >= 10-slot buy silence (lagged, so it cannot see itself)
      'm_flow_window', jsonb_build_object(
        'window_size_slots', 10,
        'window_lag', 1,
        'buy_count', jsonb_build_array(
          jsonb_build_object('operator', '=', 'value', 0)))
    ),
    'take_profit', 36.0,
    'exit', jsonb_build_object(
      'm_position', jsonb_build_object(
        'held', jsonb_build_array(
          jsonb_build_object('operator', '>=', 'value', 600)))),
    'reentry', jsonb_build_object(
      'cooldown_sec', 0.0,
      'max_episodes_per_token', 500)
  )
FROM fingerprints f WHERE f.name = 'campaign-break-29d9aacb';

COMMIT;
