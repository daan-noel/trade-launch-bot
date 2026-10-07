-- The curve swap instruction this leg executed. Same document as
-- `tokens.initial_buy_instruction`: a `type` plus that variant's fields
-- (`max_cost_lamports` on Buy / BuyV2, `spendable_lamports_in` and
-- `min_tokens_out` on the exact-in variants).
--
-- Per leg. Two buys in one transaction keep two documents.
-- NULL on every row written before this migration, on a sell, and on an AMM
-- swap: those layouts are later `type` values, not later columns.
-- Forward-only. `raw_txs` does not keep the bytes.

ALTER TABLE trades
    ADD COLUMN IF NOT EXISTS swap_ix JSONB;
