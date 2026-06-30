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

-- Idempotent: `0001_init.sql` was later updated to create the columns with their
-- venue-neutral names directly, so on a FRESH database the columns already exist
-- as reserve_sol/reserve_token and there is nothing to rename. Only a legacy DB
-- created by the pre-rename 0001 still has virtual_*; rename only in that case so
-- this migration is a safe no-op everywhere else.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'trades' AND column_name = 'virtual_sol_reserves'
    ) THEN
        ALTER TABLE trades RENAME COLUMN virtual_sol_reserves   TO reserve_sol;
    END IF;
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'trades' AND column_name = 'virtual_token_reserves'
    ) THEN
        ALTER TABLE trades RENAME COLUMN virtual_token_reserves TO reserve_token;
    END IF;
END $$;
