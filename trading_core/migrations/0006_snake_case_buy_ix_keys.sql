-- ===========================================================================
-- tokens.initial_buy_instruction: camelCase -> snake_case JSON keys
--
-- The ingest writer (`buy_ix_to_json` in live/src/ingest/consumer.rs) persisted
-- this JSONB column with camelCase keys (tokenAmount, maxSolCost,
-- spendableSolIn, minTokensOut) copied straight from the on-chain IDL naming.
-- Every reader (grouping.rs, tokens.rs, sql.rs, strategy_repo.rs) then had to
-- carry a snake_case/camelCase fallback. The writer now emits snake_case
-- directly; this one-time backfill converts every existing row so the
-- fallback code can be deleted outright instead of kept forever.
--
-- `tokens` is one row per token (not a high-volume trades/raw_txs table), so a
-- full-table backfill is bounded and safe to run inline at boot via sqlx
-- migrate. Guarded by `?|` so rows already snake_case (or with no buy-ix) are
-- left untouched, making this idempotent.
-- ===========================================================================

UPDATE tokens
SET initial_buy_instruction =
    (initial_buy_instruction - 'tokenAmount' - 'maxSolCost' - 'spendableSolIn' - 'minTokensOut')
    || jsonb_strip_nulls(jsonb_build_object(
        'token_amount', initial_buy_instruction->'tokenAmount',
        'max_sol_cost', initial_buy_instruction->'maxSolCost',
        'spendable_sol_in', initial_buy_instruction->'spendableSolIn',
        'min_tokens_out', initial_buy_instruction->'minTokensOut'
    ))
WHERE initial_buy_instruction ?| array['tokenAmount', 'maxSolCost', 'spendableSolIn', 'minTokensOut'];
