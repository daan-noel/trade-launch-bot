use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::{broadcast, watch};

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
