use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, watch};

use crate::api::handlers::system::SseFrame;
use crate::models::ingest::SseEvent;
use crate::storage::repositories::settings_repo::AppSettings;
use crate::storage::repositories::{
    creation_stats_repo::CreationStatsRepo, fingerprint_repo::FingerprintRepo,
    raw_tx_repo::RawTxRepo,
    settings_repo::SettingsRepo, strategy_repo::StrategyRepo, token_repo::TokenRepo,
    trade_repo::TradeRepo,
    wallet_dict_repo::WalletDictRepo,
    wallet_profile_repo::WalletProfileRepo,
    wallet_profile_tag_repo::WalletProfileTagRepo, wallet_repo::WalletRepo,
};

use super::token_cache::TokenCache;
use super::token_list_cache::TokenListCache;

/// Mode-agnostic shared state: DB pools, in-memory caches, SSE channels, settings,
/// SOL price, and every repository accessor. Both [`super::deploy_state::DeployState`]
/// and [`super::local_state::LocalState`] hold an `Arc<CoreState>` and `Deref` to it,
/// so a handler reads `state.token_repo()` / `state.token_cache` whichever state it
/// was injected with. Every field is an `Arc`/`PgPool`/`watch`/`broadcast` handle, so
/// constructing the per-mode states from one `CoreState` is a cheap refcount bump.
pub struct CoreState {
    /// **API** pool — fast dashboard handlers (list/detail/count reads, settings,
    /// mutations). Use this for every cheap, latency-sensitive handler query.
    pub db: PgPool,
    /// **Batch** pool — long, DB-heavy jobs only (grouped sweep corpus load +
    /// per-group writer, tpsl backtests). Routed here so they can't starve the
    /// dashboard reads on `db`. See [`crate::storage::postgres::DbPools`].
    pub batch_db: PgPool,
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
    /// In-memory source of truth for the persisted settings document. The PUT
    /// handler updates this (and the DB); the ingest pipeline subscribes to it.
    pub settings: watch::Sender<AppSettings>,
    pub sol_price: Arc<watch::Sender<Option<f64>>>,
}

impl CoreState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: PgPool,
        batch_db: PgPool,
        helius_rpc_url: String,
        helius_laserstream_url: String,
        helius_api_key: String,
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        settings: watch::Sender<AppSettings>,
        sol_price: Arc<watch::Sender<Option<f64>>>,
    ) -> Self {
        // Seed the shared list snapshot from the (DB-seeded) cache before the
        // borrow of `token_cache` is moved into the struct below.
        let token_list = Arc::new(TokenListCache::new(&token_cache));
        // The frame channel is derived here (not a constructor arg) so the render
        // bridge can be spawned with an `Arc<CoreState>` once construction completes.
        let (sse_frame_tx, _) = broadcast::channel(512);
        Self {
            db,
            batch_db,
            helius_rpc_url,
            helius_laserstream_url,
            helius_api_key,
            pump_program_id,
            token_cache,
            token_list,
            sse_tx,
            sse_frame_tx,
            settings,
            sol_price,
        }
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.borrow().clone()
    }

    /// Atomically apply `f` to the in-memory settings snapshot. Uses the watch
    /// channel's `send_modify` so the read-modify-write happens under the
    /// channel lock — a concurrent settings POST (or one racing `set_live`)
    /// can't clobber the other's fields, unlike the clone → mutate → overwrite
    /// pattern which is last-writer-wins on the whole struct.
    pub fn modify_settings(&self, f: impl FnOnce(&mut AppSettings)) {
        self.settings.send_modify(f);
    }

    pub fn latest_sol_price(&self) -> Option<f64> {
        *self.sol_price.borrow()
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

    // --- New (live/lab remake) data-layer repos -------------------------------
    // Unified strategy domain (strategy_rules/runs/run_metrics/positions),
    // per-venue sync watermarks, the raw_txs source feed, and the trades
    // wallet-interning dictionary.

    /// Unified strategy repo spanning `strategy_rules` / `strategy_runs` /
    /// `strategy_run_metrics` / `strategy_positions` — the ONE strategy repo; there
    /// are no per-strategy rule/position/paper repos.
    pub fn strategy_repo(&self) -> StrategyRepo {
        StrategyRepo::new(self.db.clone())
    }

    /// Source-of-truth raw transaction feed (`raw_txs`, BYTEA payloads).
    pub fn raw_tx_repo(&self) -> RawTxRepo {
        RawTxRepo::new(self.db.clone())
    }

    /// Wallet-address interning dictionary (`wallet_dict`) backing the trades
    /// `wallet_id` column. Hot-path writes go through this on the `hot` pool;
    /// exposed here for read/resolve on the api pool.
    pub fn wallet_dict_repo(&self) -> WalletDictRepo {
        WalletDictRepo::new(self.db.clone())
    }

    pub fn settings_repo(&self) -> SettingsRepo {
        SettingsRepo::new(self.db.clone())
    }

    pub fn creation_stats_repo(&self) -> CreationStatsRepo {
        CreationStatsRepo::new(self.db.clone())
    }

    pub fn fingerprint_repo(&self) -> FingerprintRepo {
        FingerprintRepo::new(self.db.clone())
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
    // The old per-strategy `tpsl{1,2}_{rule,position,paper}_repo()` accessors were
    // removed with the strategy-table merge; callers use `strategy_repo()` (above)
    // over the unified `strategy_rules`/`strategy_runs`/`strategy_positions`.
}
