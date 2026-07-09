//! [`ingest_laserstream::TraderHook`] bridge for [`PumpFunTrader`].
//!
//! Rust's orphan rule forbids implementing an external trait (`TraderHook` from
//! `ingest-laserstream`) for an external type (`PumpFunTrader` from `pump-trader`)
//! directly in `backend`. The solution is a transparent newtype wrapper that is
//! local to this crate and delegates every call to the inner trader.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trading_core::ingest::TraderHook;
use pump_trader::PumpFunTrader;

/// Newtype wrapper so `backend` can implement the external `TraderHook` trait
/// for the external `PumpFunTrader` without violating the orphan rule.
pub struct TraderHookBridge(pub Arc<PumpFunTrader>);

impl TraderHook for TraderHookBridge {
    fn update_live_reserves(&self, mint: &str, token_reserves: f64, sol_reserves: f64, is_amm: bool) {
        self.0.update_live_reserves(mint, token_reserves, sol_reserves, is_amm);
    }

    fn prewarm_amm_pool<'a>(
        &'a self,
        mint: &'a str,
        token_program: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        // `prewarm_amm_pool` now returns `pump_trader::TradeError`; map it into the
        // `anyhow::Result` the `TraderHook` trait expects.
        Box::pin(async move {
            self.0
                .prewarm_amm_pool(mint, token_program)
                .await
                .map_err(anyhow::Error::from)
        })
    }
}
