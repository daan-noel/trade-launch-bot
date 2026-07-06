-- ============================================================================
-- 0002_own_launch — Domain D: the OWN-LAUNCH domain (the tables token creation
-- needs first). The platform both CREATES tokens (these tables) and OBSERVES the
-- market (Domain A–C); `tokens.is_own_launch` is the flag that joins the two.
--
-- SECURITY (locked, see plan §5): `managed_wallets.key_ref` is a REFERENCE
-- (keystore path / KMS key id / envelope-encrypted blob id), NEVER a raw private
-- key. Signing goes through pump-trader's `Arc<dyn Signer>`. No secret material
-- lives in Postgres.
--
-- Amounts follow the suffix rule: `dev_buy_quote` / `tip_quote` are quote base
-- units. `params` / `legs` are the JSONB "brain" (typed columns + extensible JSON).
-- ============================================================================

-- OUR wallets (dev / bundler / treasury / trading). key_ref only — no key bytes.
CREATE TABLE IF NOT EXISTS managed_wallets (
    id               UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    address          TEXT         NOT NULL UNIQUE,
    label            TEXT,
    role             TEXT         NOT NULL CHECK (role IN ('dev','bundler','treasury','trading')),
    key_ref          TEXT         NOT NULL,        -- external keystore/KMS ref, NEVER a raw key
    derivation_index INTEGER,
    is_active        BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_managed_wallets_role   ON managed_wallets(role);
CREATE INDEX IF NOT EXISTS idx_managed_wallets_active ON managed_wallets(is_active);

-- Authored launch specs (typed lifecycle + JSONB brain). `variant` selects an
-- AUDITED create builder (e.g. 'pumpfun.create_v1'); `params` holds
-- name/symbol/uri/image/dev_buy and the leg_structures pool the bundler draws
-- from (§3e). Reusable — one template, many launches.
CREATE TABLE IF NOT EXISTS launch_templates (
    id             UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_name  TEXT         NOT NULL,
    launchpad_id   SMALLINT     NOT NULL REFERENCES launchpads(id),
    variant        TEXT         NOT NULL,          -- 'pumpfun.create_v1' | '…create_v2'
    quote_asset_id SMALLINT     NOT NULL REFERENCES quote_assets(id),
    params         JSONB        NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_launch_templates_launchpad ON launch_templates(launchpad_id);

-- Executed launch record. `dev_buy_quote` = the dev-buy amount in quote base
-- units. `bundle_id` is a soft back-ref to the phase-2 `bundles` seam (bare UUID,
-- no FK — launches doesn't depend on bundles ordering). `status` is open TEXT
-- (default 'pending') so the phase-2 execution flow can name its own states.
CREATE TABLE IF NOT EXISTS launches (
    id              UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_id     UUID         REFERENCES launch_templates(id) ON DELETE SET NULL,
    mint_address    TEXT         NOT NULL,
    launchpad_id    SMALLINT     NOT NULL REFERENCES launchpads(id),
    variant         TEXT         NOT NULL,
    quote_asset_id  SMALLINT     NOT NULL REFERENCES quote_assets(id),
    dev_wallet_id   UUID         REFERENCES managed_wallets(id),
    create_signature TEXT,
    dev_buy_quote   BIGINT,                        -- quote base units
    bundle_id       UUID,                          -- phase-2 seam (soft ref → bundles.id)
    status          TEXT         NOT NULL DEFAULT 'pending',
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_launches_mint      ON launches(mint_address);
CREATE INDEX IF NOT EXISTS idx_launches_template  ON launches(template_id);
CREATE INDEX IF NOT EXISTS idx_launches_status    ON launches(status, created_at DESC);

-- Phase-2 seam — atomic Jito bundle of a launch's buy legs. Schema lands now;
-- the composer/executor lands in phase 2 (`launcher`). `legs` is the per-leg
-- structure descriptor pool (§3e): each leg selects an AUDITED buy variant +
-- randomized budget/tip, NEVER an arbitrary account list:
--   legs = [{ wallet_id, amount_quote, structure:{ variant, slippage_bps,
--             cu_limit, cu_price, tip_account_ix, tip_quote, ix_order } }]
CREATE TABLE IF NOT EXISTS bundles (
    id         UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    launch_id  UUID         NOT NULL REFERENCES launches(id) ON DELETE CASCADE,
    status     TEXT         NOT NULL DEFAULT 'pending',
    tip_quote  BIGINT,                             -- total Jito tip, quote base units
    legs       JSONB        NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_bundles_launch ON bundles(launch_id);
