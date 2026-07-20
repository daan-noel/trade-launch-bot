use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use tokio::sync::{watch, Notify, OwnedSemaphorePermit, RwLock, Semaphore};

use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use crate::strategies::engine::{ArmedRegistry, EngineHandle};
use crate::trader::PumpFunTrader;

use ingest_laserstream::slot_anchor::SlotAnchor;

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
    /// The unified `strategy_positions`/`strategy_runs`/`strategy_rules` repo the
    /// position-read handlers page over. The generic engine owns the *lifecycle*
    /// (rule CRUD, arming, closes); this is just the durable read/write surface the
    /// HTTP layer shares with it. Cheaply `Clone` (Arc-backed pool).
    pub strategy_repo: StrategyRepo,
    /// Handle to the generic fingerprint+metrics engine loop — rule/fingerprint
    /// CRUD handlers ping it to reload, and manual position closes route through it.
    pub engine: EngineHandle,
    /// Live snapshot of armed (token, rule) pairs (the `GET /api/strategies/armed`
    /// source; the engine sink writes it).
    pub armed: ArmedRegistry,
    /// Generic `strategy_rules` repo (new-engine rule CRUD handlers).
    pub rule_repo: RuleRepo,
    /// `fingerprints` repo (new-engine fingerprint CRUD handlers).
    pub fingerprint_repo: FingerprintRepo,
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
    /// Pinned (slot, time) pair from `getBlockTime` at startup/reconnect.
    /// Used by token-sync replay paths to estimate accurate `block_time` for
    /// historical frames that carry only a slot number (not a real blockTime).
    pub slot_anchor: Arc<RwLock<Option<SlotAnchor>>>,
    /// Short-TTL cache of the composed wallet holdings so the server-paged Holdings
    /// table pages/sorts/filters over one scan per window (see
    /// [`crate::services::portfolio::HoldingsCache`]).
    pub holdings_cache: crate::services::portfolio::HoldingsCache,
    /// Short-TTL cache of the two cashback pots so the dashboard's `/api/cashback/status`
    /// poll doesn't re-read on-chain each time (see [`crate::services::cashback::CashbackCache`]).
    pub cashback_cache: crate::services::cashback::CashbackCache,
}

impl DeployState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Arc<CoreState>,
        trader: Arc<PumpFunTrader>,
        strategy_repo: StrategyRepo,
        engine: EngineHandle,
        armed: ArmedRegistry,
        rule_repo: RuleRepo,
        fingerprint_repo: FingerprintRepo,
        pool_index: Arc<DashMap<String, String>>,
        pools_changed: Arc<Notify>,
        trade_signals: Arc<TradeSignals>,
        live_mode: watch::Sender<bool>,
    ) -> Self {
        Self {
            core,
            trader,
            strategy_repo,
            engine,
            armed,
            rule_repo,
            fingerprint_repo,
            pool_index,
            pools_changed,
            trade_signals,
            sync_gate: Arc::new(SyncGate::new(MAX_CONCURRENT_SYNCS)),
            live_mode,
            slot_anchor: Arc::new(RwLock::new(None)),
            holdings_cache: Default::default(),
            cashback_cache: Default::default(),
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

    /// Replace the slot anchor used by replay paths for block_time estimation.
    /// Called once at startup and after each reconnect (best-effort; no-op on RPC error).
    pub async fn set_slot_anchor(&self, anchor: SlotAnchor) {
        *self.slot_anchor.write().await = Some(anchor);
    }

    /// Read the current slot anchor (cloned; None until first RPC pin).
    pub async fn get_slot_anchor(&self) -> Option<SlotAnchor> {
        *self.slot_anchor.read().await
    }
}

impl std::ops::Deref for DeployState {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        &self.core
    }
}

/// Guards `POST /api/token/sync` against two failure modes: a burst of requests
/// spawning unbounded RPC-heavy tasks, and two concurrent syncs of the *same*
/// mint racing on the sync-watermark write.
///
/// Usage: `try_begin(mint)` synchronously (reject with 409 if it returns false),
/// then the spawned task `acquire_permit().await` for the run and `end(mint)`
/// when finished.
pub struct SyncGate {
    /// Bounds total concurrent backfills.
    permits: Arc<Semaphore>,
    /// Mints with a sync currently in flight (dedup set).
    in_flight: DashSet<String>,
}

impl SyncGate {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(max_concurrent)),
            in_flight: DashSet::new(),
        }
    }

    /// Reserve this mint's slot. Returns `false` if a sync for it is already in
    /// flight — the caller should reject the request (409) without starting one.
    pub fn try_begin(&self, mint: &str) -> bool {
        self.in_flight.insert(mint.to_string())
    }

    /// Release this mint's slot. Call once the backfill task finishes (whatever
    /// its outcome) so the mint can be synced again.
    pub fn end(&self, mint: &str) {
        self.in_flight.remove(mint);
    }

    /// Acquire a global concurrency permit, held for the backfill's duration.
    /// `None` only if the semaphore was closed (shutdown).
    pub async fn acquire_permit(&self) -> Option<OwnedSemaphorePermit> {
        self.permits.clone().acquire_owned().await.ok()
    }

    /// Currently free concurrency permits (test/observability helper).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }
}

#[cfg(test)]
mod sync_gate_tests {
    use super::SyncGate;

    /// The dedup contract the `/api/token/sync` handler relies on: the first
    /// `try_begin` for a mint wins, a second (concurrent) one is rejected — that
    /// rejection is the handler's 409 — and after `end` the mint is syncable
    /// again. Distinct mints never interfere.
    #[test]
    fn try_begin_dedups_per_mint_until_end() {
        let gate = SyncGate::new(4);

        assert!(gate.try_begin("MINT"), "first sync of a mint is admitted");
        assert!(
            !gate.try_begin("MINT"),
            "a concurrent second sync of the same mint is rejected (handler returns 409)"
        );
        assert!(
            gate.try_begin("OTHER"),
            "a different mint is unaffected by MINT's in-flight sync"
        );

        gate.end("MINT");
        assert!(
            gate.try_begin("MINT"),
            "after the in-flight sync ends, the mint can be synced again"
        );
    }

    /// `acquire_permit` bounds global concurrency to the configured cap; permits
    /// free up as their guards drop.
    #[tokio::test]
    async fn permits_bound_global_concurrency() {
        let gate = SyncGate::new(2);
        assert_eq!(gate.available_permits(), 2);

        let p1 = gate.acquire_permit().await.expect("permit 1");
        let p2 = gate.acquire_permit().await.expect("permit 2");
        assert_eq!(gate.available_permits(), 0, "both permits in use");

        drop(p1);
        assert_eq!(gate.available_permits(), 1, "dropping a guard frees its permit");
        drop(p2);
        assert_eq!(gate.available_permits(), 2);
    }
}
