-- The venue fee a PumpSwap swap charged, in bps of the pool's own constant-product
-- amount: every fee the user side paid beyond it (lp + protocol + creator), read off
-- the swap event. A pool's fee follows its market-cap tier, so this is the only
-- record of what a leg on it cost - the cost model prices an AMM leg with it.
--
-- NULL on every curve row (the curve's fee is a protocol constant) and on every
-- amm row written before this migration. Forward-only; never coalesce to a fee.

ALTER TABLE trades
    ADD COLUMN IF NOT EXISTS venue_fee_bps REAL;
