-- ===========================================================================
-- strategy_positions.token_account — persist the wallet's token account address
-- for the position's mint so bot buys/sells reuse ONE account across restarts.
--
-- The snipe buy creates a non-canonical account via the create-with-seed template
-- pool and previously only cached its address in-memory. A process restart between
-- a first and second buy into the same mint lost that cache → the second buy made a
-- second non-canonical account, and a later "sell all" could orphan funds in it.
-- Persisting the address lets a subsequent buy reuse the same account and the sell
-- read it from the row (no cache dependency, survives restarts).
--
-- Nullable + additive: existing open rows backfill lazily (sell falls back to the
-- cache-first resolver when NULL), so this needs no data migration.
-- ===========================================================================

ALTER TABLE strategy_positions
    ADD COLUMN IF NOT EXISTS token_account TEXT;
