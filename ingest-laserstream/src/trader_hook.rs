//! Inversion-of-control trait for the trader interactions the ingest pipeline
//! needs. Decouples `ingest-laserstream` from the `pump-trader` crate so the
//! transport crate has no deploy-side dependency.
//!
//! `backend` implements this trait for `PumpFunTrader` and passes the impl in
//! via `Arc<dyn TraderHook>` when calling `spawn`.

use std::future::Future;
use std::pin::Pin;

/// Minimal trader surface the ingest pipeline calls.
///
/// - `update_live_reserves` — called on every tracked-token trade to feed
///   the reserve cache, avoiding an on-chain read on the hot exit path.
/// - `prewarm_amm_pool` — called (once, in the background) on a token's first
///   AMM trade to warm PumpSwap pool caches ahead of the eventual exit.
pub trait TraderHook: Send + Sync + 'static {
    /// Feed a post-trade reserve snapshot into the live cache.
    /// `token_reserves` and `sol_reserves` are in the same units the trade
    /// carries (`f64`); `is_amm` tags curve vs AMM venue.
    fn update_live_reserves(&self, mint: &str, token_reserves: f64, sol_reserves: f64, is_amm: bool);

    /// Warm PumpSwap pool caches for `mint` ahead of the next AMM sell.
    /// Called once per token on its first AMM trade; returns a boxed future
    /// so the trait is object-safe without `async-trait`.
    fn prewarm_amm_pool<'a>(
        &'a self,
        mint: &'a str,
        token_program: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;
}
