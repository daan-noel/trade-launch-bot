use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, watch};

use crate::models::events::InternalEvent;
use crate::trader::PumpFunTrader;

use super::{creator_cache::CreatorCache, token_cache::TokenCache};

/// Shared application state — passed to Actix handlers via `web::Data<AppState>`
/// and injected into services.
pub struct AppState {
    pub db: PgPool,
    pub token_cache: Arc<TokenCache>,
    pub creator_cache: Arc<CreatorCache>,
    /// Clone the sender to subscribe services; broadcast internally.
    pub event_tx: broadcast::Sender<InternalEvent>,
    pub live_mode: watch::Sender<bool>,
    pub sol_price: Arc<watch::Sender<Option<f64>>>,
    pub trader: Arc<PumpFunTrader>,
}

impl AppState {
    pub fn new(
        db: PgPool,
        token_cache: Arc<TokenCache>,
        creator_cache: Arc<CreatorCache>,
        event_tx: broadcast::Sender<InternalEvent>,
        live_mode: watch::Sender<bool>,
        sol_price: Arc<watch::Sender<Option<f64>>>,
        trader: Arc<PumpFunTrader>,
    ) -> Self {
        Self {
            db,
            token_cache,
            creator_cache,
            event_tx,
            live_mode,
            sol_price,
            trader,
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
