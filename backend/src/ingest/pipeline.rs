use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tracing::{debug, info, warn};

use crate::{
    config::constants::{POOL_REFRESH_INTERVAL_SECONDS, POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS},
    ingest::{
        db_writer::{DbWriteOp, TokenMetricsWrite},
        decoder::{DecodeOutput, HeliusDecoder},
    },
    models::{
        events::{
            CreatorActivityEvent, InternalEvent, LiquidityEvent, TokenCreatedEvent,
            TokenMigratedEvent, TradeExecutedEvent,
        },
        ingest::{IngestKind, SseEvent, StrategyPing},
        transaction::RawTransaction,
    },
    services::token_sync::derive_pump_swap_pool,
    state::token_cache::{TokenCache, TokenState},
    storage::repositories::settings_repo::AppSettings,
    trader::PumpFunTrader,
};

const DB_QUEUE_CAP: usize = 4096;
const STRATEGY_QUEUE_CAP: usize = 512;

/// Single hot-path owner: decode → filter → order → cache → queue DB → ping strategy / SSE.
pub struct IngestPipeline {
    decoder: HeliusDecoder,
    token_cache: Arc<TokenCache>,
    db_tx: mpsc::Sender<DbWriteOp>,
    strategy_tx: mpsc::Sender<StrategyPing>,
    sse_tx: broadcast::Sender<SseEvent>,
    pump_program_id: String,
    /// pool → mint index shared with the decoder (to attribute live PumpSwap
    /// swaps, which carry the pool but not the base mint) and with the WS task
    /// (to drive per-pool subscriptions). Holds migrated tokens' pools.
    pool_index: Arc<DashMap<String, String>>,
    /// Pinged whenever a token migrates and a new pool is added to `pool_index`,
    /// so the WS task subscribes to it without dropping the connection.
    pools_changed: Arc<Notify>,
    /// Live settings (persisted in `app_settings`, mutated via the settings API).
    /// `track_mayhem` gates Mayhem-mode ingestion; `track_post_migration` gates
    /// recording migrated tokens' AMM trades. Flipping either off also applies to
    /// already-tracked state (see [`Self::run`]).
    settings_rx: watch::Receiver<AppSettings>,
    /// Trader handle — fed live reserve snapshots from each tracked-token trade
    /// and asked to pre-warm a token's AMM pool caches on its first AMM trade, so
    /// the trade path reads cached reserves / a warm pool instead of RPC.
    trader: Arc<PumpFunTrader>,
    /// Mints whose AMM pool caches have already been pre-warmed (or are warming),
    /// so we spawn the warm task at most once per token. An entry is removed if
    /// its warm attempt fails, letting a later AMM trade retry.
    prewarmed_pools: Arc<DashMap<String, ()>>,
}

impl IngestPipeline {
    pub fn new(
        pump_program_id: String,
        token_cache: Arc<TokenCache>,
        db_tx: mpsc::Sender<DbWriteOp>,
        strategy_tx: mpsc::Sender<StrategyPing>,
        sse_tx: broadcast::Sender<SseEvent>,
        settings_rx: watch::Receiver<AppSettings>,
        trader: Arc<PumpFunTrader>,
    ) -> Self {
        let (track_mayhem, track_post_migration) = {
            let s = settings_rx.borrow();
            (s.track_mayhem, s.track_post_migration)
        };

        // Seed the pool→mint index for tokens already migrated AND recently
        // active before this process started, so live AMM swaps for them are
        // captured immediately — without subscribing to every pool that ever
        // graduated. Quiet pools are re-added by `run_pool_subscription_refresh`
        // once they trade again; live migrations register via `register_pool`.
        // Skipped entirely when post-migration tracking is disabled.
        let pool_index: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
        if track_post_migration {
            let now = Utc::now();
            for entry in token_cache.iter() {
                if entry.is_migrated && pool_is_live(entry.value(), now) {
                    if let Ok(pool) = derive_pump_swap_pool(entry.key(), &pump_program_id) {
                        pool_index.insert(pool, entry.key().clone());
                    }
                }
            }
        }

        // Honor the Mayhem policy against the seeded cache: if tracking is off at
        // startup, drop any Mayhem tokens the seed loaded so they aren't tracked
        // live (their historical rows stay in the DB).
        if !track_mayhem {
            let mayhem: Vec<String> = token_cache
                .iter()
                .filter(|e| e.token.is_mayhem_mode)
                .map(|e| e.key().clone())
                .collect();
            for mint in &mayhem {
                token_cache.remove(mint);
            }
            if !mayhem.is_empty() {
                info!(
                    "Tracking: Mayhem disabled — dropped {} seeded Mayhem token(s)",
                    mayhem.len()
                );
            }
        }

        let decoder =
            HeliusDecoder::new(pump_program_id.clone()).with_pool_index(pool_index.clone());

        Self {
            decoder,
            token_cache,
            db_tx,
            strategy_tx,
            sse_tx,
            pump_program_id,
            pool_index,
            pools_changed: Arc::new(Notify::new()),
            settings_rx,
            trader,
            prewarmed_pools: Arc::new(DashMap::new()),
        }
    }

    /// Shared pool → mint index, for the WS task's per-pool subscriptions.
    pub fn pool_index(&self) -> Arc<DashMap<String, String>> {
        self.pool_index.clone()
    }

    /// Migration signal, pinged when a new pool is added to [`Self::pool_index`].
    pub fn pools_changed(&self) -> Arc<Notify> {
        self.pools_changed.clone()
    }

    pub fn channel_pair() -> (
        mpsc::Sender<DbWriteOp>,
        mpsc::Receiver<DbWriteOp>,
        mpsc::Sender<StrategyPing>,
        mpsc::Receiver<StrategyPing>,
    ) {
        let (db_tx, db_rx) = mpsc::channel(DB_QUEUE_CAP);
        let (strategy_tx, strategy_rx) = mpsc::channel(STRATEGY_QUEUE_CAP);
        (db_tx, db_rx, strategy_tx, strategy_rx)
    }

    pub async fn run(self, mut raw_rx: mpsc::Receiver<String>) {
        info!("IngestPipeline: starting");

        // Independent receiver for change notifications, so the hot path's
        // `borrow()` reads (in the event handlers) never contend with `changed()`.
        // Track the previous values of the two flags we act on, so an unrelated
        // settings change (e.g. timezone) never triggers cache/pool work.
        let mut settings_changed = self.settings_rx.clone();
        let (mut prev_mayhem, mut prev_post_migration) = {
            let s = settings_changed.borrow();
            (s.track_mayhem, s.track_post_migration)
        };

        loop {
            tokio::select! {
                maybe_raw = raw_rx.recv() => {
                    let Some(raw) = maybe_raw else { break };
                    match self.decoder.decode(&raw) {
                        DecodeOutput::Subscribed(id) => {
                            info!("Helius subscription confirmed — id={id}");
                        }
                        DecodeOutput::Transaction { raw_tx, mut events } => {
                            sort_events(&mut events);
                            let (events, save_raw) = filter_events(
                                events,
                                &self.token_cache,
                                self.settings_rx.borrow().track_mayhem,
                            );
                            if events.is_empty() {
                                continue;
                            }

                            if save_raw {
                                self.enqueue_db(DbWriteOp::Raw(raw_tx.clone()));
                            }

                            for event in events {
                                self.apply_event(event, &raw_tx).await;
                            }
                        }
                        DecodeOutput::Ignored => {}
                    }
                }

                // A setting changed — act only on genuine transitions of the two
                // flags this pipeline reacts to.
                _ = settings_changed.changed() => {
                    let (mayhem, post_migration) = {
                        let s = settings_changed.borrow();
                        (s.track_mayhem, s.track_post_migration)
                    };

                    // Mayhem flipped off → evict already-tracked Mayhem tokens
                    // (go-forward for new ones is enforced in `filter_events`).
                    if prev_mayhem && !mayhem {
                        self.evict_mayhem_tokens();
                    }
                    prev_mayhem = mayhem;

                    // Post-migration toggled → clear subscribed pools (off) so the
                    // decoder stops attributing AMM swaps, or re-seed live pools
                    // (on) so recording resumes for active migrated tokens.
                    if prev_post_migration != post_migration {
                        if post_migration {
                            self.reseed_live_pools();
                        } else {
                            self.clear_pools();
                        }
                        prev_post_migration = post_migration;
                    }
                }
            }
        }

        info!("IngestPipeline: raw_rx closed — stopping");
    }

    /// Drop every Mayhem-mode token from the cache. Their historical DB rows are
    /// left intact; they simply stop receiving live trade/metric updates (and
    /// subsequent trades for them early-return on the cache miss). Re-enabling is
    /// go-forward — evicted tokens require a manual re-sync to track again.
    fn evict_mayhem_tokens(&self) {
        let mints: Vec<String> = self
            .token_cache
            .iter()
            .filter(|e| e.token.is_mayhem_mode)
            .map(|e| e.key().clone())
            .collect();
        for mint in &mints {
            self.token_cache.remove(mint);
        }
        info!(
            "Tracking: Mayhem disabled — evicted {} Mayhem token(s) from cache",
            mints.len()
        );
    }

    /// Drop all subscribed PumpSwap pools. The decoder attributes live AMM swaps
    /// via `pool_index`, so clearing it stops recording post-migration trades
    /// immediately; the WS connection's now-orphaned subscriptions are trimmed on
    /// its next natural reconnect.
    fn clear_pools(&self) {
        let n = self.pool_index.len();
        self.pool_index.clear();
        info!("Tracking: post-migration disabled — cleared {n} pool(s); AMM trades no longer recorded");
    }

    /// Re-seed `pool_index` with the pools of currently-active migrated tokens and
    /// wake the WS task to subscribe. Inverse of [`Self::clear_pools`], used when
    /// post-migration tracking is re-enabled.
    fn reseed_live_pools(&self) {
        let now = Utc::now();
        let mut added = false;
        for entry in self.token_cache.iter() {
            if entry.is_migrated && pool_is_live(entry.value(), now) {
                if let Ok(pool) = derive_pump_swap_pool(entry.key(), &self.pump_program_id) {
                    if self.pool_index.insert(pool, entry.key().clone()).is_none() {
                        added = true;
                    }
                }
            }
        }
        if added {
            self.pools_changed.notify_one();
        }
        info!("Tracking: post-migration enabled — re-seeded live pools");
    }

    async fn apply_event(&self, event: InternalEvent, raw_tx: &RawTransaction) {
        match event {
            InternalEvent::TokenCreated(e) => self.on_token_created(e).await,
            InternalEvent::TradeExecuted(e) => self.on_trade_executed(e).await,
            InternalEvent::TokenMigrated(e) => self.on_token_migrated(e).await,
            InternalEvent::CreatorActivityDetected(e) => self.on_creator_activity(e),
            InternalEvent::LiquidityAdded(e) => self.on_liquidity(e, true, raw_tx),
            InternalEvent::LiquidityRemoved(e) => self.on_liquidity(e, false, raw_tx),
        }
    }

    fn enqueue_db(&self, op: DbWriteOp) {
        if let Err(e) = self.db_tx.try_send(op) {
            warn!("DbWriter queue full — dropping write: {e}");
        }
    }

    fn ping_strategy(&self, mint: String, kind: IngestKind) {
        if let Err(e) = self.strategy_tx.try_send(StrategyPing { mint, kind }) {
            warn!("Strategy queue full — dropping ping: {e}");
        }
    }

    fn emit_sse(&self, event: SseEvent) {
        let _ = self.sse_tx.send(event);
    }

    /// Register a migrated token's canonical PumpSwap pool in the pool→mint
    /// index so the decoder can attribute live AMM swaps (which carry the pool,
    /// not the base mint) back to this token, and wake the WS task to subscribe
    /// to it. Idempotent; only a genuinely new pool pings `pools_changed`.
    fn register_pool(&self, mint: &str) {
        match derive_pump_swap_pool(mint, &self.pump_program_id) {
            Ok(pool) => {
                if self.pool_index.insert(pool, mint.to_string()).is_none() {
                    self.pools_changed.notify_one();
                }
            }
            Err(e) => warn!("Pool derivation failed for {mint}: {e}"),
        }
    }

    async fn on_token_created(&self, e: TokenCreatedEvent) {
        let mint = e.token.mint_address.clone();
        if self.token_cache.contains_key(&mint) {
            debug!("Duplicate TokenCreated for {mint} — skipping");
            return;
        }

        info!(
            name = %e.token.name,
            symbol = %e.token.symbol,
            mint = %mint,
            "New token tracked"
        );

        let creator = e.token.creator_wallet.clone();
        self.enqueue_db(DbWriteOp::Token(e.token.clone()));
        self.enqueue_db(DbWriteOp::Wallet(creator));

        let token_state = TokenState::new(e.token);
        let metrics = metrics_from_state(&mint, &token_state, false);
        self.token_cache.insert(mint.clone(), token_state);
        self.enqueue_db(DbWriteOp::Metrics(metrics));

        self.ping_strategy(mint.clone(), IngestKind::TokenCreated);
        self.emit_sse(SseEvent::TokenCreated {
            mint,
            tx_signature: e.tx_signature,
            slot: e.slot,
            timestamp: e.timestamp,
        });
    }

    async fn on_trade_executed(&self, e: TradeExecutedEvent) {
        let mint = e.trade.mint_address.clone();
        let wallet = e.trade.wallet_address.clone();

        if !self.token_cache.contains_key(&mint) {
            return;
        }

        debug!(
            mint = %mint,
            wallet = %wallet,
            kind = ?e.trade.trade_type,
            "Trade applied"
        );

        self.enqueue_db(DbWriteOp::Trade(e.trade.clone()));
        self.enqueue_db(DbWriteOp::Wallet(wallet.clone()));

        if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
            token_state.add_trade(e.trade.clone());
            let metrics = metrics_from_state(&mint, &token_state, true);
            drop(token_state);
            self.enqueue_db(DbWriteOp::Metrics(metrics));
        }

        // Feed the trader's live reserve cache from this trade's post-trade
        // snapshot, so a subsequent buy/sell skips the on-chain reserve read. The
        // venue tag keeps curve and AMM snapshots from being mixed up.
        let is_amm = e.trade.venue == "amm";
        if let (Some(token_reserves), Some(sol_reserves)) =
            (e.trade.virtual_token_reserves, e.trade.virtual_sol_reserves)
        {
            self.trader
                .update_live_reserves(&mint, token_reserves, sol_reserves, is_amm);
        }

        // On a token's first AMM trade, warm its PumpSwap pool caches (pool facts
        // + fee-share marker + global config) in the background. This swap gives
        // the marker scan something to read, and moves that cold fetch off the
        // bot's eventual exit path. Only when the token program is known, so we
        // never cache a guessed pool layout; spawned once per mint.
        if is_amm && self.prewarmed_pools.insert(mint.clone(), ()).is_none() {
            match self
                .token_cache
                .get(&mint)
                .and_then(|s| s.token.token_program_id.clone())
            {
                Some(token_program) => {
                    let trader = self.trader.clone();
                    let prewarmed = self.prewarmed_pools.clone();
                    let warm_mint = mint.clone();
                    tokio::spawn(async move {
                        if let Err(err) = trader.prewarm_amm_pool(&warm_mint, &token_program).await {
                            debug!("AMM pool prewarm failed for {warm_mint}: {err}");
                            prewarmed.remove(&warm_mint); // let a later AMM trade retry
                        }
                    });
                }
                None => {
                    // Unknown token program — don't warm with a guess; allow retry.
                    self.prewarmed_pools.remove(&mint);
                }
            }
        }

        self.ping_strategy(mint.clone(), IngestKind::Trade);
        self.emit_sse(SseEvent::TradeExecuted {
            mint,
            wallet,
            trade_type: e.trade.trade_type,
            sol_amount: e.trade.sol_amount,
            token_amount: e.trade.token_amount,
            price_per_token: e.trade.price_per_token,
            tx_signature: e.tx_signature,
            slot: e.slot,
            timestamp: e.timestamp,
        });
    }

    async fn on_token_migrated(&self, e: TokenMigratedEvent) {
        let mint = e.mint_address.clone();
        if !self.token_cache.contains_key(&mint) {
            return;
        }

        // Register the pool now (before AMM swaps begin) so the decoder can
        // attribute them and the WS task subscribes to this pool — unless
        // post-migration tracking is disabled, in which case the migration is
        // still recorded but its AMM trades are not.
        if self.settings_rx.borrow().track_post_migration {
            self.register_pool(&mint);
        }

        if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
            token_state.is_migrated = true;
            let metrics = metrics_from_state(&mint, &token_state, false);
            drop(token_state);
            self.enqueue_db(DbWriteOp::Metrics(metrics));
        }

        self.enqueue_db(DbWriteOp::Migration { mint: mint.clone() });
        self.ping_strategy(mint, IngestKind::Migrated);
    }

    fn on_creator_activity(&self, e: CreatorActivityEvent) {
        if !self.token_cache.contains_key(&e.mint_address) {
            return;
        }
        self.ping_strategy(e.mint_address, IngestKind::CreatorActivity);
    }

    fn on_liquidity(&self, e: LiquidityEvent, added: bool, _raw_tx: &RawTransaction) {
        let mint = e.mint_address.clone();
        if !self.token_cache.contains_key(&mint) {
            return;
        }
        let sse = if added {
            SseEvent::LiquidityAdded {
                mint: mint.clone(),
                wallet: e.wallet_address,
                sol_amount: e.sol_amount,
                token_amount: e.token_amount,
                tx_signature: e.tx_signature,
                slot: e.slot,
                timestamp: e.timestamp,
            }
        } else {
            SseEvent::LiquidityRemoved {
                mint: mint.clone(),
                wallet: e.wallet_address,
                sol_amount: e.sol_amount,
                token_amount: e.token_amount,
                tx_signature: e.tx_signature,
                slot: e.slot,
                timestamp: e.timestamp,
            }
        };
        self.emit_sse(sse);
    }
}

fn sort_events(events: &mut [InternalEvent]) {
    events.sort_by_key(|e| match e {
        InternalEvent::TokenCreated(_) => 0,
        InternalEvent::TokenMigrated(_) => 1,
        InternalEvent::TradeExecuted(_) => 2,
        InternalEvent::CreatorActivityDetected(_) => 3,
        InternalEvent::LiquidityAdded(_) => 4,
        InternalEvent::LiquidityRemoved(_) => 5,
    });
}

fn filter_events(
    events: Vec<InternalEvent>,
    cache: &TokenCache,
    track_mayhem: bool,
) -> (Vec<InternalEvent>, bool) {
    let mut out = Vec::new();
    let mut save_raw = false;

    for event in events {
        match &event {
            InternalEvent::TokenCreated(e) => {
                // Tracking policy: drop Mayhem-mode creates entirely when
                // disabled, so neither the token nor its raw tx is persisted.
                if e.token.is_mayhem_mode && !track_mayhem {
                    continue;
                }
                save_raw = true;
                out.push(event);
            }
            InternalEvent::TradeExecuted(e) => {
                if cache.contains_key(&e.trade.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::TokenMigrated(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::CreatorActivityDetected(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
            InternalEvent::LiquidityAdded(e) | InternalEvent::LiquidityRemoved(e) => {
                if cache.contains_key(&e.mint_address) {
                    save_raw = true;
                    out.push(event);
                }
            }
        }
    }

    (out, save_raw)
}

fn metrics_from_state(mint: &str, state: &TokenState, recompute_rugged: bool) -> TokenMetricsWrite {
    let age_seconds = Utc::now()
        .signed_duration_since(state.token.created_at)
        .num_seconds();
    TokenMetricsWrite {
        mint: mint.to_string(),
        ath_price: state.ath_price,
        ath_timestamp: state.ath_timestamp,
        age_seconds: Some(age_seconds as i64),
        volume: state.volume_sol_total,
        market_cap: state.market_cap,
        trade_count: state.trade_count as i64,
        last_trade_at: state.last_trade_at,
        current_price: state.current_price,
        is_migrated: state.is_migrated,
        creator_wallet: state.token.creator_wallet.clone(),
        recompute_rugged,
    }
}

/// True when a migrated token traded recently enough to keep its PumpSwap pool
/// in the live subscription set. A token with no recorded trades, or quiet
/// beyond the window, is treated as not live (and pruned from the subscription).
fn pool_is_live(state: &TokenState, now: DateTime<Utc>) -> bool {
    state
        .last_trade_at
        .map(|t| {
            now.signed_duration_since(t).num_seconds() < POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS
        })
        .unwrap_or(false)
}

/// Periodic revival sweep: subscribe to the PumpSwap pools of migrated tokens
/// that have become active since the last pass but aren't yet covered. This is
/// what keeps a pool pruned from the startup set (or whose live `Migrate` event
/// was missed) from going blind permanently — once fresh trades land in the
/// cache (e.g. via a manual sync that refreshes `last_trade_at`), its pool is
/// added and the WS task subscribes on the next ping. Only ever adds; the live
/// subscription set is trimmed back to active pools on the next reconnect.
pub async fn run_pool_subscription_refresh(
    token_cache: Arc<TokenCache>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
    pump_program_id: String,
    settings_rx: watch::Receiver<AppSettings>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(POOL_REFRESH_INTERVAL_SECONDS));
    loop {
        tick.tick().await;

        // Respect the tracking policy: never revive pools while post-migration
        // tracking is disabled (it would undo `clear_pools`).
        if !settings_rx.borrow().track_post_migration {
            continue;
        }

        let now = Utc::now();
        // Mints already covered (pool_index values), so we only derive pools for
        // the newly-active delta.
        let covered: HashSet<String> = pool_index.iter().map(|e| e.value().clone()).collect();

        let mut added = false;
        for entry in token_cache.iter() {
            if !entry.is_migrated
                || covered.contains(entry.key())
                || !pool_is_live(entry.value(), now)
            {
                continue;
            }
            if let Ok(pool) = derive_pump_swap_pool(entry.key(), &pump_program_id) {
                if pool_index.insert(pool, entry.key().clone()).is_none() {
                    added = true;
                }
            }
        }

        if added {
            pools_changed.notify_one();
        }
    }
}
