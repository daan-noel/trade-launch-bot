-- ===========================================================================
-- trades.virtual_*_reserves -> reserve_sol / reserve_token (venue-neutral rename)
--
-- The columns were named for the bonding-curve case, where price comes from the
-- curve's *virtual* reserves. But for AMM/migrated tokens the decoder
-- (`build_amm_trade`) copies the PumpSwap pool's REAL reserves into these same
-- fields, so post-migration the column literally named `virtual_*` holds real
-- pool reserves. The honest, venue-neutral meaning is "the reserve pair this row
-- prices from" — hence `reserve_sol` / `reserve_token`. The spot price is
-- `reserve_sol / reserve_token` for both venues (the model's `spot_price()`).
--
-- Pure rename: no continuous aggregate, view, index, compression-orderby, or
-- retention policy references these columns (OHLCV caggs derive from
-- sol_amount / token_amount), so a plain RENAME COLUMN has no dependents to drop.
-- ===========================================================================

ALTER TABLE trades RENAME COLUMN virtual_sol_reserves   TO reserve_sol;
ALTER TABLE trades RENAME COLUMN virtual_token_reserves TO reserve_token;
