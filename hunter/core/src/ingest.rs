//! Transport-agnostic ingest contract shared by every ingest crate.
//!
//! The decoded event model + `pipeline`/`db_writer` stay inside the concrete
//! transport crate (`ingest-laserstream`); only the cross-transport surface
//! lives here so both `ingest-laserstream` and `ingest-websocket` can expose the
//! same `spawn(...) -> IngestHandles` and `live` can swap transports without
//! touching its wiring.
//!
//! Surface:
//! - [`IngestHandles`] — what `spawn` returns to the supervising `select!`.
//! - [`TraderHook`] — the minimal trader surface the pipeline calls (IoC, keeps
//!   transport crates free of any `pump-trader` dependency).
//! - re-exports of [`StrategyPing`] and [`TradeSignals`], the two event/wakeup
//!   types that cross the contract boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Notify;

pub use crate::models::ingest::StrategyPing;
pub use crate::state::trade_signals::TradeSignals;

/// Handles returned from a transport's `spawn` for the supervising `select!` in
/// a bin's `main`.
pub struct IngestHandles {
    /// Pool → mint index shared with the live state. A token sync registers a
    /// migrated token's pool here so live ingest subscribes immediately.
    pub pool_index: Arc<DashMap<String, String>>,
    /// Pinged when `pool_index` changes; consumed by the transport client to
    /// resubscribe without dropping the connection.
    pub pools_changed: Arc<Notify>,
    /// Strategy ping receiver — feed into the StrategyRunner.
    pub strategy_rx: tokio::sync::mpsc::Receiver<StrategyPing>,
    /// Transport producer task (gRPC / websocket).
    pub producer_task: tokio::task::JoinHandle<()>,
    /// Ingest pipeline decode task.
    pub pipeline_task: tokio::task::JoinHandle<()>,
    /// DB writer flush task.
    pub db_writer_task: tokio::task::JoinHandle<()>,
}

/// Minimal trader surface the ingest pipeline calls.
///
/// Inversion-of-control so the transport crates depend only on `trading_core`,
/// never on `pump-trader`. `live` implements this for its `PumpFunTrader` and
/// passes the impl in via `Arc<dyn TraderHook>` when calling `spawn`.
///
/// - `update_live_reserves` — called on every tracked-token trade to feed the
///   reserve cache, avoiding an on-chain read on the hot exit path.
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
