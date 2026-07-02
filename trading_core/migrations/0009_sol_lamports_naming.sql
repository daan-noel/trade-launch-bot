-- 0009_sol_lamports_naming.sql
-- End-to-end SOL/lamports naming pass (see sol-lamports-naming-plan.md).
--
-- Locked rule: every column that denotes an amount of SOL makes its unit
-- unambiguous from its name alone. Lamports columns (exact BIGINT) end in
-- `_lamports`; human-SOL columns (float) end in `_sol`. Pure renames + one
-- JSONB key rewrite + three view rebuilds — no data is transformed except the
-- JSONB key rename (same proven pattern as 0006_snake_case_buy_ix_keys.sql).
--
-- Views that expose a renamed column freeze their OUTPUT column names at
-- creation and `CREATE OR REPLACE VIEW` cannot rename an existing output
-- column, so the three affected views are dropped and recreated. None of them
-- has downstream dependents (verified). The `trades_ohlcv_*` continuous
-- aggregates reference `trades.sol_amount` internally; RENAME COLUMN propagates
-- into their real-time view definitions automatically (verified).

-- Drop views that expose renamed columns (recreated at the bottom).
DROP VIEW IF EXISTS trades_priced;
DROP VIEW IF EXISTS strategy_position_pnl;
DROP VIEW IF EXISTS token_overview;

-- trades ---------------------------------------------------------------------
ALTER TABLE trades RENAME COLUMN sol_amount  TO amount_lamports;
ALTER TABLE trades RENAME COLUMN reserve_sol TO reserve_lamports;

-- tokens ---------------------------------------------------------------------
ALTER TABLE tokens RENAME COLUMN initial_buy_sol TO initial_buy_lamports;

-- tokens.initial_buy_instruction JSONB keys (mirrors migration 0006).
-- Only object-typed rows carrying either key are touched; scalar/null rows are
-- skipped by the `?|` predicate.
UPDATE tokens
SET initial_buy_instruction =
    (initial_buy_instruction - 'max_sol_cost' - 'spendable_sol_in')
    || jsonb_strip_nulls(jsonb_build_object(
        'max_cost_lamports',     initial_buy_instruction->'max_sol_cost',
        'spendable_lamports_in', initial_buy_instruction->'spendable_sol_in'
    ))
WHERE initial_buy_instruction ?| array['max_sol_cost', 'spendable_sol_in'];

-- tokens_info ----------------------------------------------------------------
ALTER TABLE tokens_info RENAME COLUMN volume              TO volume_sol;
ALTER TABLE tokens_info RENAME COLUMN first_slot_buy_sol  TO first_slot_buy_lamports;
ALTER TABLE tokens_info RENAME COLUMN first_slot_sell_sol TO first_slot_sell_lamports;
ALTER INDEX IF EXISTS idx_tokens_info_volume RENAME TO idx_tokens_info_volume_sol;

-- strategy_rules -------------------------------------------------------------
ALTER TABLE strategy_rules RENAME COLUMN buy_amount TO buy_amount_sol;

-- strategy_positions ---------------------------------------------------------
ALTER TABLE strategy_positions RENAME COLUMN entry_sol TO entry_lamports;
ALTER TABLE strategy_positions RENAME COLUMN exit_sol  TO exit_lamports;

-- Recreate views with corrected column names ---------------------------------

-- token_overview: full token picture; age/market_cap derived (never stored).
-- Column list matches the pre-0009 definition with volume->volume_sol and
-- initial_buy_sol->initial_buy_lamports.
CREATE VIEW token_overview AS
SELECT
    t.mint_address,
    t.creator_wallet,
    t.name,
    t.symbol,
    t.bonding_curve_address,
    t.token_program_id,
    t.initial_supply_token,
    t.initial_buy_lamports,
    t.cu_limit,
    t.cu_price,
    t.is_mayhem_mode,
    t.is_cashback_enabled,
    t.creation_tx_signature,
    t.ix_labels,
    t.initial_buy_instruction,
    t.meta,
    t.created_at,
    i.current_price,
    i.ath_price,
    i.ath_timestamp,
    i.volume_sol,
    i.trade_count,
    i.last_trade_at,
    i.is_dead,
    i.is_migrated,
    i.lifetime_secs,
    i.updated_at AS metrics_updated_at,
    EXTRACT(EPOCH FROM (now() - t.created_at))::bigint AS age_secs,
    (i.current_price * t.initial_supply_token)         AS market_cap
FROM tokens t
LEFT JOIN tokens_info i USING (mint_address);

-- trades_priced: derived price. price_per_token is now SOL per raw token unit
-- (÷1e9), matching every other price_per_token in the codebase (Decision D1 —
-- was lamports/token before, a silent unit mismatch).
CREATE VIEW trades_priced AS
SELECT
    t.*,
    (t.amount_lamports::double precision / 1e9 / NULLIF(t.token_amount, 0)) AS price_per_token
FROM trades t;

-- strategy_position_pnl: derived per-position PnL (never stored).
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    ((p.exit_lamports - p.entry_lamports)::float8 / 1e9)                AS realized_pnl_sol,
    CASE WHEN p.entry_price > 0
         THEN (p.exit_price - p.entry_price) / p.entry_price * 100.0 END AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END       AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                       AS is_closed
FROM strategy_positions p;
