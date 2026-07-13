-- Surface the canonical wallet ADDRESS on the priced-trade read model.
--
-- `trades.wallet_ref` is an interned `wallet_dict` int key (internal storage id).
-- The SSOT rule is that the on-chain address is the canonical cross-subsystem
-- wallet identity, so read paths resolve `wallet_ref` → `wallet_dict.address`
-- rather than leaking the interned id to callers/UI.
--
-- LEFT JOIN + COALESCE fallback (NOT an inner join): a `wallet_ref` with no
-- `wallet_dict` row (e.g. db-incremental-sync id-space divergence on the local
-- mirror) must still yield its trade — degrade to a '#<ref>' marker, never drop
-- the row.

DROP VIEW IF EXISTS trades_priced;
CREATE VIEW trades_priced AS
SELECT
    t.*,
    COALESCE(w.address, '#' || t.wallet_ref)                       AS wallet_address,
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
