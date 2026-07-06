-- Rename strategy_positions.mint -> mint_address.
--
-- Token-data SSOT: every token-data column/field/JSON key in the project names the
-- unit-free mint the SAME way — `mint_address` (matches tokens.mint_address,
-- trades.mint_address, and the TokenRecord/DTO wire contract). `strategy_positions`
-- was the ONE physical column still called `mint`; this aligns it so the positions
-- rows expose `mint_address` on the wire without a SELECT alias.
ALTER TABLE strategy_positions RENAME COLUMN mint TO mint_address;

-- Keep the supporting index name in step with its column.
ALTER INDEX idx_strategy_positions_mint_status
    RENAME TO idx_strategy_positions_mint_address_status;
