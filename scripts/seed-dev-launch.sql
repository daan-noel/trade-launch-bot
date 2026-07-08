-- Dev launch seed — run AFTER wallet-encrypt + updating addresses below.
--
--   1. cargo run -p live -- wallet-encrypt .\dev.json dev-01.enc
--   2. cargo run -p live -- wallet-encrypt .\bundler.json bundler-01.enc
--   3. Replace DEV_PUBKEY and BUNDLER_PUBKEY with the pubkey strings from those keypairs.
--   4. psql $DATABASE_URL -f scripts/seed-dev-launch.sql
--
-- Template launches with a 2-leg sniper bundle (auto-submit on execute).

BEGIN;

INSERT INTO managed_wallets (address, label, role, key_ref)
VALUES
    ('DEV_PUBKEY', 'dev-01', 'dev', 'dev-01.enc'),
    ('BUNDLER_PUBKEY', 'bundler-01', 'bundler', 'bundler-01.enc')
ON CONFLICT (address) DO UPDATE SET
    label = EXCLUDED.label,
    role = EXCLUDED.role,
    key_ref = EXCLUDED.key_ref,
    status = 'funded';

-- Unlink templates first (FK is ON DELETE SET NULL), then re-seed their metadata
-- presets. Token identity (name/symbol/uri) now lives in `metadata_templates`
-- and a launch template REFERENCES one via `metadata_template_id` — no inlined
-- name/symbol/uri in `params` (migration 0007).
DELETE FROM launch_templates  WHERE template_name IN ('dev-sniper-2leg', 'my-first-launch');
DELETE FROM metadata_templates WHERE template_name IN ('Dev Sniper Test', 'test');

-- Bundler wallets are no longer template-listed: launch execution claims N
-- `funded` `role=bundler` wallets atomically from the pool (wallet-pool Phase 3,
-- ManagedWalletRepo::claim_funded). `bundle_leg_count` here is just the
-- template's default leg count; the Launch Console's "use N bundlers" control
-- can override it per launch.
WITH md AS (
    INSERT INTO metadata_templates (template_name, name, symbol, uri)
    VALUES ('Dev Sniper Test', 'Dev Sniper Test', 'SNIP', 'https://example.com/meta.json')
    RETURNING id
)
INSERT INTO launch_templates (
    template_name,
    launchpad_id,
    variant,
    quote_asset_id,
    metadata_template_id,
    params
)
SELECT
    'dev-sniper-2leg',
    1,
    'pumpfun.create_v2',
    1,
    md.id,
    jsonb_build_object(
        'dev_buy_quote', 0,
        'slippage_bps', 500,
        'is_mayhem_mode', false,
        'cashback_enabled', false,
        'bundle_leg_count', 2,
        'bundle_quote_per_leg', 50000000,
        'bundle_tip_quote', 100000,
        'leg_structures', jsonb_build_array(
            jsonb_build_object(
                'variant', 'buy_exact_sol_in',
                'slippage_bps_min', 300,
                'slippage_bps_max', 800,
                'cu_limit_min', 120000,
                'cu_limit_max', 180000,
                'cu_price_min', 150000,
                'cu_price_max', 400000,
                'tip_quote_min', 10000,
                'tip_quote_max', 100000
            ),
            jsonb_build_object(
                'variant', 'buy',
                'slippage_bps_min', 300,
                'slippage_bps_max', 800
            )
        )
    )
FROM md;

-- Sample template linked to the 'test' metadata preset.
WITH md AS (
    INSERT INTO metadata_templates (template_name, name, symbol, uri)
    VALUES ('test', 'test', 'test', 'ipfs://QmTNUVzpKjTMfYXVAQtFYZ81vj4cR99XbWiNKfk58Kod47')
    RETURNING id
)
INSERT INTO launch_templates (
    template_name,
    launchpad_id,
    variant,
    quote_asset_id,
    metadata_template_id,
    params
)
SELECT
    'my-first-launch',
    1,
    'pumpfun.create_v2',
    1,
    md.id,
    jsonb_build_object(
        'dev_buy_quote', 50000000,
        'slippage_bps', 500,
        'is_mayhem_mode', false,
        'cashback_enabled', false,
        'bundle_leg_count', 2,
        'bundle_quote_per_leg', 50000000,
        'bundle_tip_quote', 100000,
        'leg_structures', jsonb_build_array(
            jsonb_build_object(
                'variant', 'buy_exact_sol_in',
                'slippage_bps_min', 300,
                'slippage_bps_max', 800,
                'cu_limit_min', 120000,
                'cu_limit_max', 180000,
                'cu_price_min', 150000,
                'cu_price_max', 400000,
                'tip_quote_min', 10000,
                'tip_quote_max', 100000
            ),
            jsonb_build_object(
                'variant', 'buy'
            )
        )
    )
FROM md;

COMMIT;

-- Quick sanity:
-- SELECT id, template_name, params->'bundle_leg_count' FROM launch_templates;
-- SELECT id, label, role, address, status FROM managed_wallets WHERE status != 'retired';
