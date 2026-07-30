-- 0019_fix_exit_price_units.sql — correct exit_price on scale-out closes (mig 0018).
--
-- WHY. `record_sell_fill` stamped `exit_price` as total_sol / (sold_tokens / 1e6),
-- treating raw on-chain units as whole tokens. `entry_price` is always
-- SOL-per-raw-unit (`amount_sol / token_amount`), so PnL% blew up ~1e6× while
-- realized SOL PnL (exit_lamports − entry_lamports) stayed correct.

UPDATE strategy_positions
SET exit_price = (exit_sol_lamports_total::float8 / 1e9)
                 / NULLIF(sold_token_amount, 0)::float8
WHERE status = 'End'
  AND sold_token_amount > 0
  AND exit_sol_lamports_total > 0;
