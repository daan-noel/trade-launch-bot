-- Exit variants of campaign-break-v0, same door/event/permissions, for engine
-- simulate A/B. Pre-registered from the measured path shape (green-zone p75
-- up-move ~+90-105% price, median drawdown ~20-25%):
--   trail : armed trail (arm +20% net, give back 15%) capped by TP +80, 600 s clock
--   tp60  : TP +55 net (~+60% price), 600 s clock
BEGIN;
DELETE FROM strategy_rules WHERE rule_name IN ('campaign-break-v0-trail','campaign-break-v0-tp60');

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params
)
SELECT 'campaign-break-v0-trail', r.fingerprint_id, 'paper', false, true,
       r.buy_amount_lamports, r.max_concurrent_tokens, r.max_total_tokens,
       (r.params - 'exit' - 'take_profit')
         || jsonb_build_object(
              'take_profit', 80.0,
              'exit', jsonb_build_array(
                jsonb_build_object('m_position', jsonb_build_object(
                  'arm_above_pct', 20.0,
                  'armed',   jsonb_build_array(jsonb_build_object('operator','=','value',1.0)),
                  'retrace', jsonb_build_array(jsonb_build_object('operator','>=','value',15.0)))),
                jsonb_build_object('m_position', jsonb_build_object(
                  'held', jsonb_build_array(jsonb_build_object('operator','>=','value',600.0))))))
FROM strategy_rules r WHERE r.rule_name = 'campaign-break-v0';

INSERT INTO strategy_rules (
  rule_name, fingerprint_id, trade_mode, is_active, is_enabled,
  buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params
)
SELECT 'campaign-break-v0-tp60', r.fingerprint_id, 'paper', false, true,
       r.buy_amount_lamports, r.max_concurrent_tokens, r.max_total_tokens,
       r.params || jsonb_build_object('take_profit', 55.0)
FROM strategy_rules r WHERE r.rule_name = 'campaign-break-v0';
COMMIT;
