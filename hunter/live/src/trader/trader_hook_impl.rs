//! [`ingest_pumpfun::TraderHook`] bridge for the loaded wallets' traders.
//!
//! Rust's orphan rule forbids implementing an external trait (`TraderHook` from
//! `ingest-laserstream`) for an external type (`PumpFunTrader` from `pump-trader`)
//! directly in `backend`. The solution is a newtype wrapper local to this crate.
//!
//! Every loaded wallet has its own trader with its own reserve and AMM
//! pool-facts caches, so each feed observation fans out to all of them. A wallet
//! whose cache the feed never fills reads reserves and pool layouts over RPC on
//! every sell.

use std::sync::Arc;

use trading_core::ingest::TraderHook;

use crate::wallets::WalletRuntime;

pub struct TraderHookBridge(pub Arc<WalletRuntime>);

impl TraderHook for TraderHookBridge {
    fn update_live_reserves(
        &self,
        mint: &str,
        token_reserves: f64,
        sol_reserves: f64,
        is_amm: bool,
        venue_fee_bps: Option<f64>,
    ) {
        self.0.for_each_trader(|trader| {
            trader.update_live_reserves(mint, token_reserves, sol_reserves, is_amm, venue_fee_bps);
        });
    }

    /// Warm only once every loaded trader is: the caller stops observing the
    /// mint after `true`, so one cold trader must keep it observing.
    fn observe_amm_swap_accounts(&self, mint: &str, token_program: &str, keys: &[String]) -> bool {
        let mut warm: Option<bool> = None;
        self.0.for_each_trader(|trader| {
            let this = trader.observe_amm_swap_accounts(mint, token_program, keys);
            warm = Some(warm.unwrap_or(true) && this);
        });
        warm.unwrap_or(false)
    }
}
