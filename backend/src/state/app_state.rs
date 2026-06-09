use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use tokio::sync::{broadcast, watch, Notify};

use crate::models::ingest::SseEvent;
use crate::storage::repositories::settings_repo::AppSettings;
use crate::strategies::tpsl::TpslRuntimeCache;
use crate::trader::PumpFunTrader;

use super::token_cache::TokenCache;

/// Shared application state — passed to Actix handlers via `web::Data<AppState>`
/// and injected into services.
pub struct AppState {
    pub db: PgPool,
    pub helius_rpc_url: String,
    pub pump_program_id: String,
    pub token_cache: Arc<TokenCache>,
    /// Cold lane: SSE subscribers only (fed after cache update in ingest pipeline).
    pub sse_tx: broadcast::Sender<SseEvent>,
    pub live_mode: watch::Sender<bool>,
    /// In-memory source of truth for the persisted settings document. The PUT
    /// handler updates this (and the DB); the ingest pipeline subscribes to it.
    pub settings: watch::Sender<AppSettings>,
    pub sol_price: Arc<watch::Sender<Option<f64>>>,
    pub trader: Arc<PumpFunTrader>,
    pub tpsl_cache: Arc<TpslRuntimeCache>,
    /// Live PumpSwap pool → mint index (shared with the ingest pipeline and WS
    /// task). A token sync registers a migrated token's pool here to subscribe.
    pub pool_index: Arc<DashMap<String, String>>,
    /// Pinged when a new pool is registered, waking the WS task to subscribe.
    pub pools_changed: Arc<Notify>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: PgPool,
        helius_rpc_url: String,
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        live_mode: watch::Sender<bool>,
        settings: watch::Sender<AppSettings>,
        sol_price: Arc<watch::Sender<Option<f64>>>,
        trader: Arc<PumpFunTrader>,
        tpsl_cache: Arc<TpslRuntimeCache>,
        pool_index: Arc<DashMap<String, String>>,
        pools_changed: Arc<Notify>,
    ) -> Self {
        Self {
            db,
            helius_rpc_url,
            pump_program_id,
            token_cache,
            sse_tx,
            live_mode,
            settings,
            sol_price,
            trader,
            tpsl_cache,
            pool_index,
            pools_changed,
        }
    }

    pub fn is_live(&self) -> bool {
        *self.live_mode.borrow()
    }

    pub fn set_live(&self, live: bool) {
        let _ = self.live_mode.send(live);
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.borrow().clone()
    }

    pub fn set_settings(&self, settings: AppSettings) {
        let _ = self.settings.send(settings);
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
