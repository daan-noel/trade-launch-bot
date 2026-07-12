-- Atomic launch bundle: the create (+ dev-buy) leg now rides INSIDE the Jito
-- bundle (tx0), so a dropped bundle means the token was never created and the
-- confirm watcher must rebuild the WHOLE bundle (create included) on a re-bid.
-- The re-bid runs in the background with only the bundle + launch rows in hand —
-- it no longer has the original launch request's metadata — so the create leg's
-- build inputs (name/symbol/uri/flags/variant/dev-buy) are persisted here.
-- Nullable: a bundle planned before this cutover (create already landed on its
-- own tx) has no create_args and stays on the legacy co-buy-only submit path.
ALTER TABLE bundles ADD COLUMN IF NOT EXISTS create_args JSONB;
