use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, watch};

use crate::models::ingest::SseEvent;
use crate::strategies::tpsl::TpslRuntimeCache;
use crate::trader::PumpFunTrader;

use super::token_cache::TokenCache;

/// Shared application state — passed to Actix handlers via `web::Data<AppState>`
/// and injected into services.
pub struct AppState {
    pub db: PgPool,
    pub token_cache: Arc<TokenCache>,
    /// Cold lane: SSE subscribers only (fed after cache update in ingest pipeline).
    pub sse_tx: broadcast::Sender<SseEvent>,
    pub live_mode: watch::Sender<bool>,
    pub sol_price: Arc<watch::Sender<Option<f64>>>,
    pub trader: Arc<PumpFunTrader>,
    pub tpsl_cache: Arc<TpslRuntimeCache>,
}

impl AppState {
    pub fn new(
        db: PgPool,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        live_mode: watch::Sender<bool>,
        sol_price: Arc<watch::Sender<Option<f64>>>,
        trader: Arc<PumpFunTrader>,
        tpsl_cache: Arc<TpslRuntimeCache>,
    ) -> Self {
        Self {
            db,
            token_cache,
            sse_tx,
            live_mode,
            sol_price,
            trader,
            tpsl_cache,
        }
    }

    pub fn is_live(&self) -> bool {
        *self.live_mode.borrow()
    }

    pub fn set_live(&self, live: bool) {
        let _ = self.live_mode.send(live);
    }

    pub fn set_sol_price(&self, price: Option<f64>) {
        let _ = self.sol_price.send(price);
    }

    pub fn latest_sol_price(&self) -> Option<f64> {
        *self.sol_price.borrow()
    }

    pub fn subscribe_sol_price(&self) -> watch::Receiver<Option<f64>> {
        self.sol_price.subscribe()
    }
}
