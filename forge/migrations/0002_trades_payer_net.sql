-- ============================================================================
-- 0002_trades_payer_net — the trading wallet's whole-transaction SOL flow.
--
-- `trades.payer_net_lamports`: signed lamports (post - pre) the transaction moved
-- for its fee payer - the swap, the venue fee, the signature + priority fee and
-- any tip - with SOL parked in the payer's OWN token accounts counted as still
-- the payer's (a fresh ATA's rent is a deposit, not a spend). Stamped on every
-- leg of the tx (per-transaction: collapse by `tx_signature` before summing).
-- Filled only when the tx's fee payer IS the trade's wallet, else NULL; NULL
-- too when the source carried no balances. It is what a managed wallet's
-- position PnL books, so the PnL equals what the wallet moved.
--
-- Native lamports, like `managed_wallets.balance_lamports`: fees and tips are
-- always SOL whatever the trade's quote asset, so no `_quote` unit applies.
--
-- Idempotent (a ledger reconcile re-runs every version above 1): the column add
-- is guarded, and the view is dropped + recreated so `t.*` picks the column up.
-- ============================================================================

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'trades' AND column_name = 'payer_net_lamports'
    ) THEN
        ALTER TABLE trades ADD COLUMN payer_net_lamports BIGINT;
    END IF;
END $$;

DROP VIEW IF EXISTS trades_priced;
CREATE VIEW trades_priced AS
SELECT
    t.*,
    COALESCE(w.address, '#' || t.wallet_ref)                        AS wallet_address,
    qa.symbol   AS quote_symbol,
    qa.decimals AS quote_decimals,
    qa.usd_rate AS quote_usd_rate,
    (t.amount_quote::double precision  / NULLIF(t.amount_base, 0))  AS exec_price_quote,
    (t.reserve_quote::double precision / NULLIF(t.reserve_base, 0)) AS spot_price_quote,
    (t.amount_quote::double precision / power(10, qa.decimals))                    AS amount_quote_display,
    (t.amount_quote::double precision / power(10, qa.decimals) * qa.usd_rate)      AS amount_usd
FROM trades t
JOIN quote_assets qa ON qa.id = t.quote_asset_id
LEFT JOIN wallet_dict w ON w.id = t.wallet_ref;
