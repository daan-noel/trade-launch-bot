-- ===========================================================================
-- Part 1 (real-money & correctness) schema deltas.
--   See hunter/docs/project-audit/project-audit-and-redesign.md → "Part 1".
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- C1 — sell re-send guard. The sell-confirm loop can now leave a position
-- `ExitUnconfirmed` (the sell landed-or-is-in-flight but the feed never
-- confirmed the clear) instead of blindly re-sending on a non-revert status.
-- It is terminal-for-automation and alarmed for manual review — never
-- re-evaluated, never auto-re-sold. Add it to the status domain.
-- ---------------------------------------------------------------------------
ALTER TABLE strategy_positions DROP CONSTRAINT IF EXISTS strategy_positions_status_check;
ALTER TABLE strategy_positions
    ADD CONSTRAINT strategy_positions_status_check
    CHECK (status IN ('Arming','BuySubmitted','Holding','ExitPending',
                      'ExitUnconfirmed','End','ExitFailed'));

-- ---------------------------------------------------------------------------
-- H1 — market-cap basis. Market cap (FDV) is `current_price × total supply`,
-- NOT `current_price × initial_supply_token` (the dev's first-buy amount, which
-- the formula wrongly used as supply). pump.fun mints have a fixed total supply:
-- 1B tokens (1e15 raw units @ 6 decimals), or 2B for `create_v2` "mayhem" mints.
-- Stored so no SQL site hardcodes the magic constant; populated at ingest from
-- `config::constants::token_math::total_supply_for` (the Rust SSOT). The backfill
-- below mirrors that function for pre-existing rows.
-- ---------------------------------------------------------------------------
ALTER TABLE tokens ADD COLUMN IF NOT EXISTS total_supply_token BIGINT;

UPDATE tokens SET total_supply_token =
    CASE WHEN is_mayhem_mode THEN 2000000000000000 ELSE 1000000000000000 END
WHERE total_supply_token IS NULL;

-- Repoint the (unused-by-code but kept-consistent) full-picture view onto the
-- real supply basis. DROP+CREATE — same reason as 0001 (a replace can't rewrite
-- an existing column's expression cleanly across re-runs).
DROP VIEW IF EXISTS token_overview;
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
    t.creation_slot,
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
    i.first_slot_buy_lamports,
    i.first_slot_sell_lamports,
    i.is_dead,
    i.is_migrated,
    i.lifetime_secs,
    i.updated_at AS metrics_updated_at,
    EXTRACT(EPOCH FROM (now() - t.created_at))::bigint AS age_secs,
    (i.current_price * t.total_supply_token)           AS market_cap
FROM tokens t
LEFT JOIN tokens_info i USING (mint_address);
