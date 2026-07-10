-- 0004: wallet-identity clarity + dropped-leg positions + own-launch backfill.
--
-- Two distinct wallet id-spaces were BOTH called `wallet_id`, which was the whole
-- source of the "creator ≠ dev wallet" confusion:
--   * managed_wallets.id (UUID) — OUR wallets        → renamed managed_wallet_id
--   * wallet_dict.id     (int)  — any network wallet  → renamed wallet_ref
-- The base58 `address` is the ONE canonical, cross-subsystem, user-facing wallet
-- identity; the integer/UUID keys are storage-layer keys that never surface in the
-- UI and never correlate across subsystems (join by `address`).

-- --- trades hypertable: wallet_id (int → wallet_dict.id) → wallet_ref ---------
-- trades_priced is `SELECT t.*, …`, so it depends on the column name; drop it,
-- rename, then recreate it verbatim (the rename metadata op is compression-safe).
DROP VIEW IF EXISTS trades_priced;
ALTER TABLE trades RENAME COLUMN wallet_id TO wallet_ref;

CREATE VIEW trades_priced AS
SELECT
    t.*,
    qa.symbol   AS quote_symbol,
    qa.decimals AS quote_decimals,
    qa.usd_rate AS quote_usd_rate,
    (t.amount_quote::double precision  / NULLIF(t.amount_base, 0))  AS exec_price_quote,
    (t.reserve_quote::double precision / NULLIF(t.reserve_base, 0)) AS spot_price_quote,
    (t.amount_quote::double precision / power(10, qa.decimals))               AS amount_quote_display,
    (t.amount_quote::double precision / power(10, qa.decimals) * qa.usd_rate) AS amount_usd
FROM trades t
JOIN quote_assets qa ON qa.id = t.quote_asset_id;

-- --- token_positions: wallet_id (UUID → managed_wallets.id) → managed_wallet_id
ALTER TABLE token_positions RENAME COLUMN wallet_id TO managed_wallet_id;

-- The `UNIQUE (mint_address, wallet_id)` constraint's column list is auto-updated
-- by the RENAME; only its NAME still reads ..._wallet_id — cosmetic, left as-is.

-- Denormalize the on-chain address onto the row (like `role`) so the holdings read
-- renders the canonical identity with no join. Backfill, then enforce NOT NULL.
ALTER TABLE token_positions ADD COLUMN IF NOT EXISTS wallet_address TEXT;
UPDATE token_positions p
   SET wallet_address = w.address
  FROM managed_wallets w
 WHERE w.id = p.managed_wallet_id AND p.wallet_address IS NULL;
ALTER TABLE token_positions ALTER COLUMN wallet_address SET NOT NULL;

-- Terminal 'dropped' status for a bundler leg whose bundle never landed (it never
-- bought and never spent — zero cost basis, never reconciled to open/closed).
ALTER TABLE token_positions DROP CONSTRAINT IF EXISTS token_positions_status_check;
ALTER TABLE token_positions
    ADD CONSTRAINT token_positions_status_check CHECK (status IN ('open','closed','dropped'));

-- --- data backfills for existing rows ----------------------------------------

-- is_own_launch: any mint we hold a launch row for is ours. The ingest feed's
-- `INSERT … ON CONFLICT DO NOTHING` had raced ahead of the launcher and left the
-- feed's `is_own_launch = false` on our own launches.
UPDATE tokens SET is_own_launch = TRUE
 WHERE is_own_launch = FALSE
   AND mint_address IN (SELECT mint_address FROM launches);

-- Existing bundler positions seeded from a bundle that never landed: they never
-- bought (balance 0) and the 0.0x cost basis is money that was never spent.
-- Relabel as `dropped` at zero cost so the holdings table stops showing a phantom
-- closed buy. (A Jito bundle is atomic: not-landed ⇒ no leg bought.)
UPDATE token_positions p
   SET status = 'dropped', cost_quote = 0
  FROM launches l
  JOIN bundles b ON b.id = l.bundle_id
 WHERE p.mint_address = l.mint_address
   AND p.role = 'bundler'
   AND p.balance_base = 0
   AND b.status <> 'landed';
