//! [`ingest_laserstream::TraderHook`] bridge for [`PumpFunTrader`].
//!
//! Rust's orphan rule forbids implementing an external trait (`TraderHook` from
//! `ingest-laserstream`) for an external type (`PumpFunTrader` from `pump-trader`)
//! directly in `backend`. The solution is a transparent newtype wrapper that is
//! local to this crate and delegates every call to the inner trader.

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

    fn observe_amm_swap_accounts(&self, mint: &str, token_program: &str, keys: &[String]) -> bool {
        self.0.observe_amm_swap_accounts(mint, token_program, keys)
    }
}
