use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{watch, Notify};

use crate::storage::repositories::settings_repo::AppSettings;
use crate::strategies::tpsl_sniper_1::Tpsl1RuntimeCache;
use crate::strategies::tpsl_sniper_2::Tpsl2RuntimeCache;
use crate::trader::PumpFunTrader;

use super::app_state::SyncGate;
use super::core_state::CoreState;
use super::trade_signals::TradeSignals;

/// Max concurrent token-sync backfills (each is RPC- and DB-heavy).
const MAX_CONCURRENT_SYNCS: usize = 4;

/// Live-mode (deploy) state: the shared [`CoreState`] plus the handles only the
/// trading process needs — the trader, strategy runtime caches, the live
/// pool→mint index, the persisted-trade wakeup hub, the sync gate, and the
/// live-mode toggle. `Deref`s to `CoreState` so deploy handlers reach core
/// fields/accessors (`state.token_repo()`, `state.token_cache`) transparently.
pub struct DeployState {
    pub core: Arc<CoreState>,
    pub trader: Arc<PumpFunTrader>,
    pub tpsl1_cache: Arc<Tpsl1RuntimeCache>,
    pub tpsl2_cache: Arc<Tpsl2RuntimeCache>,
    /// Live PumpSwap pool → mint index (shared with the ingest pipeline and WS
    /// task). A token sync registers a migrated token's pool here to subscribe.
    pub pool_index: Arc<DashMap<String, String>>,
    /// Pinged when a new pool is registered, waking the WS task to subscribe.
    pub pools_changed: Arc<Notify>,
    /// Persisted-trade wakeup hub: lets live buy/sell confirm loops (incl. the
    /// manual-close sell spawned from lifecycle handlers) react to the feed.
    pub trade_signals: Arc<TradeSignals>,
    /// Concurrency + per-mint dedup gate for `POST /api/token/sync` backfills.
    pub sync_gate: Arc<SyncGate>,
    pub live_mode: watch::Sender<bool>,
}

impl DeployState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Arc<CoreState>,
        trader: Arc<PumpFunTrader>,
        tpsl1_cache: Arc<Tpsl1RuntimeCache>,
        tpsl2_cache: Arc<Tpsl2RuntimeCache>,
        pool_index: Arc<DashMap<String, String>>,
        pools_changed: Arc<Notify>,
        trade_signals: Arc<TradeSignals>,
        live_mode: watch::Sender<bool>,
    ) -> Self {
        Self {
            core,
            trader,
            tpsl1_cache,
            tpsl2_cache,
            pool_index,
            pools_changed,
            trade_signals,
            sync_gate: Arc::new(SyncGate::new(MAX_CONCURRENT_SYNCS)),
            live_mode,
        }
    }

    pub fn is_live(&self) -> bool {
        *self.live_mode.borrow()
    }

    pub fn set_live(&self, live: bool) {
        let _ = self.live_mode.send(live);
    }

    pub fn settings(&self) -> AppSettings {
        self.core.settings()
    }

    pub fn modify_settings(&self, f: impl FnOnce(&mut AppSettings)) {
        self.core.modify_settings(f);
    }
}

impl std::ops::Deref for DeployState {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        &self.core
    }
}
