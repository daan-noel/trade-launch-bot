use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use sqlx::PgPool;
use uuid::Uuid;
use tokio::sync::{broadcast, watch, Notify, OwnedSemaphorePermit, RwLock, Semaphore};

use crate::api::handlers::system::SseFrame;
use crate::models::ingest::SseEvent;
use crate::storage::repositories::settings_repo::AppSettings;
use crate::storage::repositories::{
    analysis_repo::AnalysisRepo, creation_stats_repo::CreationStatsRepo,
    settings_repo::SettingsRepo, token_repo::TokenRepo,
    trade_repo::TradeRepo, tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo,
    tpsl1_position_repo::Tpsl1PositionRepo, tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo,
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
    tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo, wallet_profile_repo::WalletProfileRepo,
    wallet_profile_tag_repo::WalletProfileTagRepo, wallet_repo::WalletRepo,
};
use crate::strategies::tpsl_sniper_1::Tpsl1RuntimeCache;
use crate::strategies::tpsl_sniper_2::Tpsl2RuntimeCache;
use crate::trader::PumpFunTrader;

use crate::sweep::corpus::TokenTrades;

use super::backtest_trade_cache::BacktestTradeCache;
use super::job_progress::ProgressCell;
use super::swing_run_cache::SwingRunCache;
use super::token_cache::TokenCache;
use super::token_list_cache::TokenListCache;
use super::trade_signals::TradeSignals;

/// Option A: the fully-loaded corpus (trades + fingerprints) from the most recent
/// sweep run, keyed by its corpus hash. Lets `list_token_results` skip both the
/// Parquet read and the `attach_fingerprints` DB queries on the warm path — the
/// common case where a user drills into a combo right after running a sweep.
pub struct SweepCorpusCache {
    pub corpus_hash: String,
    /// All tokens with fingerprints already attached, as `Arc<Vec<_>>` so
    /// cloning into the handler is a refcount bump (no copy of the trade data).
    pub tokens: Arc<Vec<TokenTrades>>,
}

/// Shared application state — passed to Actix handlers via `web::Data<AppState>`
/// and injected into services.
pub struct AppState {
    pub db: PgPool,
    pub helius_rpc_url: String,
    /// LaserStream gRPC endpoint + API key, used by the token-sync replay fast
    /// path (Fetch New). Empty URL ⇒ replay disabled, RPC path only.
    pub helius_laserstream_url: String,
    pub helius_api_key: String,
    pub pump_program_id: String,
    pub token_cache: Arc<TokenCache>,
    /// Shared, staleness-bounded snapshot of the token list backing
    /// `GET /api/tokens`. Lets every client's poll read one pre-sorted, pre-built
    /// view instead of each request cloning + sorting the whole cache.
    pub token_list: Arc<TokenListCache>,
    /// Cold lane: producers publish typed `SseEvent`s here (ingest pipeline,
    /// strategy services, tpsl handlers). A single render bridge consumes this.
    pub sse_tx: broadcast::Sender<SseEvent>,
    /// Pre-rendered SSE frames fanned out to HTTP subscribers. The render bridge
    /// serializes each event to bytes exactly ONCE (reading live stats from the
    /// cache once per event) and broadcasts the shared `Arc<SseFrame>`; each
    /// connection clones the ref-counted frame instead of re-serializing per
    /// subscriber. See `stream::run_sse_render_bridge`.
    pub sse_frame_tx: broadcast::Sender<Arc<SseFrame>>,
    pub live_mode: watch::Sender<bool>,
    /// In-memory source of truth for the persisted settings document. The PUT
    /// handler updates this (and the DB); the ingest pipeline subscribes to it.
    pub settings: watch::Sender<AppSettings>,
    pub sol_price: Arc<watch::Sender<Option<f64>>>,
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
    /// Cross-run cache of per-mint trade history for backtests. Reuses immutable
    /// history across simulation runs (keyed on `TokenState::trade_count` for
    /// exact, free freshness) so re-tuning a rule re-fetches nothing. Constructed
    /// empty; only the backtest reads/writes it.
    pub backtest_trade_cache: Arc<BacktestTradeCache>,
    /// Single-flight gate for the CPU-heavy grouped sweep
    /// (`POST /api/strategies/sweeps`): while one is running the handler returns
    /// 409, so a sweep can never pile more rayon work onto the live trading
    /// process while one is already in flight.
    pub sweep_running: Arc<AtomicBool>,
    /// Cooperative cancel flag for the in-flight grouped sweep. The cancel
    /// endpoint sets it; the sweep engine polls it between groups / in the
    /// per-token loop and bails. Reset to `false` when a sweep starts. One flag
    /// suffices because the sweep is single-flight (see `sweep_running`).
    pub sweep_cancel: Arc<AtomicBool>,
    /// Per-rule cooperative cancel flags for in-flight simulations (backtests).
    /// The simulate handler inserts a flag when a run starts and removes it when
    /// it ends; the cancel endpoint flips it; `run_backtest` polls it per chunk.
    pub sim_cancels: Arc<DashMap<Uuid, Arc<AtomicBool>>>,
    /// Live `processed / total` snapshot of the in-flight grouped sweep, written
    /// by `SweepProgress` alongside its SSE frames and read by `/api/jobs/status`
    /// so a dashboard that mounts mid-run (or after a refresh) can recover the
    /// bar. Reset to `0 / 0` when the sweep ends.
    pub sweep_progress: Arc<ProgressCell>,
    /// Per-rule `processed / total` snapshots of in-flight simulations, the
    /// per-rule analogue of `sweep_progress`. The simulate handler inserts an
    /// entry when a run starts and removes it when it ends (keyed like
    /// `sim_cancels`); `/api/jobs/status` reads them for recovery.
    pub sim_progress: Arc<DashMap<Uuid, Arc<ProgressCell>>>,
    /// Raw swings from recent "Swing Detection All" runs, keyed by client run id.
    /// Lets the server-side-paged tokens list sort by the chain columns and
    /// re-group on chain-latency changes without re-running detection.
    pub swing_runs: Arc<SwingRunCache>,
    /// Option A: single-entry in-memory corpus cache. Written after each sweep
    /// completes (trades + fingerprints in hand); read by `list_token_results` to
    /// skip Parquet I/O and `attach_fingerprints` on the warm path. Single entry —
    /// a new sweep overwrites the previous.
    pub sweep_corpus_cache: Arc<RwLock<Option<SweepCorpusCache>>>,
}

/// Max concurrent token-sync backfills (each is RPC- and DB-heavy).
const MAX_CONCURRENT_SYNCS: usize = 4;

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
    pub fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: PgPool,
        helius_rpc_url: String,
        helius_laserstream_url: String,
        helius_api_key: String,
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        live_mode: watch::Sender<bool>,
        settings: watch::Sender<AppSettings>,
        sol_price: Arc<watch::Sender<Option<f64>>>,
        trader: Arc<PumpFunTrader>,
        tpsl1_cache: Arc<Tpsl1RuntimeCache>,
        tpsl2_cache: Arc<Tpsl2RuntimeCache>,
        pool_index: Arc<DashMap<String, String>>,
        pools_changed: Arc<Notify>,
        trade_signals: Arc<TradeSignals>,
    ) -> Self {
        // Seed the shared list snapshot from the (DB-seeded) cache before the
        // borrow of `token_cache` is moved into the struct below.
        let token_list = Arc::new(TokenListCache::new(&token_cache));
        // The frame channel is derived here (not a constructor arg) so every
        // existing call site stays unchanged; the render bridge is spawned with
        // an `Arc<AppState>` once construction completes.
        let (sse_frame_tx, _) = broadcast::channel(512);
        Self {
            db,
            helius_rpc_url,
            helius_laserstream_url,
            helius_api_key,
            pump_program_id,
            token_cache,
            token_list,
            sse_tx,
            sse_frame_tx,
            live_mode,
            settings,
            sol_price,
            trader,
            tpsl1_cache,
            tpsl2_cache,
            pool_index,
            pools_changed,
            trade_signals,
            sync_gate: Arc::new(SyncGate::new(MAX_CONCURRENT_SYNCS)),
            backtest_trade_cache: Arc::new(BacktestTradeCache::new()),
            sweep_running: Arc::new(AtomicBool::new(false)),
            sweep_cancel: Arc::new(AtomicBool::new(false)),
            sim_cancels: Arc::new(DashMap::new()),
            sweep_progress: Arc::new(ProgressCell::default()),
            sim_progress: Arc::new(DashMap::new()),
            // Keep a few recent runs so re-runs / multiple tabs don't accumulate.
            swing_runs: Arc::new(SwingRunCache::new(3)),
            sweep_corpus_cache: Arc::new(RwLock::new(None)),
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

    /// Atomically apply `f` to the in-memory settings snapshot. Uses the watch
    /// channel's `send_modify` so the read-modify-write happens under the
    /// channel lock — a concurrent settings POST (or one racing `set_live`)
    /// can't clobber the other's fields, unlike the clone → mutate → overwrite
    /// pattern which is last-writer-wins on the whole struct.
    pub fn modify_settings(&self, f: impl FnOnce(&mut AppSettings)) {
        self.settings.send_modify(f);
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

    // --- Repository accessors -------------------------------------------------
    // Each repo is a thin handle over a cloned `PgPool` (itself an Arc-backed,
    // cheap-to-clone pool handle). These let handlers write `state.token_repo()`
    // instead of repeating `TokenRepo::new(state.db.clone())` at every call site.

    pub fn token_repo(&self) -> TokenRepo {
        TokenRepo::new(self.db.clone())
    }

    pub fn trade_repo(&self) -> TradeRepo {
        TradeRepo::new(self.db.clone())
    }

    pub fn settings_repo(&self) -> SettingsRepo {
        SettingsRepo::new(self.db.clone())
    }

    pub fn analysis_repo(&self) -> AnalysisRepo {
        AnalysisRepo::new(self.db.clone())
    }

    pub fn creation_stats_repo(&self) -> CreationStatsRepo {
        CreationStatsRepo::new(self.db.clone())
    }

    pub fn wallet_repo(&self) -> WalletRepo {
        WalletRepo::new(self.db.clone())
    }

    pub fn wallet_profile_repo(&self) -> WalletProfileRepo {
        WalletProfileRepo::new(self.db.clone())
    }

    pub fn wallet_tag_repo(&self) -> WalletProfileTagRepo {
        WalletProfileTagRepo::new(self.db.clone())
    }

    pub fn tpsl1_rule_repo(&self) -> Tpsl1StrategyRuleRepo {
        Tpsl1StrategyRuleRepo::new(self.db.clone())
    }

    pub fn tpsl1_position_repo(&self) -> Tpsl1PositionRepo {
        Tpsl1PositionRepo::new(self.db.clone())
    }

    pub fn tpsl1_paper_repo(&self) -> Tpsl1PaperTradingRepo {
        Tpsl1PaperTradingRepo::new(self.db.clone())
    }

    pub fn tpsl2_rule_repo(&self) -> Tpsl2StrategyRuleRepo {
        Tpsl2StrategyRuleRepo::new(self.db.clone())
    }

    pub fn tpsl2_position_repo(&self) -> Tpsl2PositionRepo {
        Tpsl2PositionRepo::new(self.db.clone())
    }

    pub fn tpsl2_paper_repo(&self) -> Tpsl2PaperTradingRepo {
        Tpsl2PaperTradingRepo::new(self.db.clone())
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
