//! The pump.fun / SOL venue adapter.
//!
//! Resolves the interned dimension ids (`launchpads.key = 'pump_fun'`,
//! `quote_assets.symbol = 'SOL'`) from the DB at startup — so the seed ids are
//! the single source of truth and this adapter never hardcodes them. Classifies
//! the pump.fun bonding-curve vs PumpSwap AMM programs to a `MarketKind`.

use sqlx::PgPool;

use platform_core::venue::{LaunchpadAdapter, MarketKind};

/// pump.fun bonding-curve program (protocol constant, mirrored from
/// `ingest-laserstream::protocol`).
pub const CURVE_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
/// PumpSwap AMM program.
pub const AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

/// pump.fun launchpad adapter: pump.fun quotes everything in native SOL.
#[derive(Debug, Clone, Copy)]
pub struct PumpFunAdapter {
    launchpad_id: i16,
    sol_quote_asset_id: i16,
}

impl PumpFunAdapter {
    /// Resolve the interned ids from the seeded dimensions. Fails loudly if the
    /// pump_fun launchpad or the SOL quote asset is missing (migration not run).
    pub async fn resolve(pool: &PgPool) -> anyhow::Result<Self> {
        let launchpad_id: i16 = sqlx::query_scalar("SELECT id FROM launchpads WHERE key = 'pump_fun'")
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("launchpads seed 'pump_fun' missing — run migrations"))?;
        let sol_quote_asset_id: i16 =
            sqlx::query_scalar("SELECT id FROM quote_assets WHERE symbol = 'SOL'")
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| anyhow::anyhow!("quote_assets seed 'SOL' missing — run migrations"))?;
        Ok(Self { launchpad_id, sol_quote_asset_id })
    }

    /// Test-only constructor with fixed interned ids — lets the pure map reuse be
    /// exercised (e.g. the keystore-restore fixture decode test) without a DB round
    /// trip through [`Self::resolve`].
    #[cfg(test)]
    pub(crate) fn for_test(launchpad_id: i16, sol_quote_asset_id: i16) -> Self {
        Self { launchpad_id, sol_quote_asset_id }
    }
}

impl LaunchpadAdapter for PumpFunAdapter {
    fn key(&self) -> &str {
        "pump_fun"
    }

    fn launchpad_id(&self) -> i16 {
        self.launchpad_id
    }

    fn quote_asset_id(&self, _kind: MarketKind) -> i16 {
        // pump.fun (curve + PumpSwap AMM) is SOL-quoted throughout.
        self.sol_quote_asset_id
    }

    fn classify_market(&self, program_id: &str) -> Option<MarketKind> {
        match program_id {
            CURVE_PROGRAM_ID => Some(MarketKind::BondingCurve),
            AMM_PROGRAM_ID => Some(MarketKind::Amm),
            _ => None,
        }
    }
}
