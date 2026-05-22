use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::{
    models::{
        events::{CreatorActivityEvent, InternalEvent, TokenCreatedEvent, TradeExecutedEvent},
        wallet::Wallet,
    },
    state::{
        creator_cache::{CreatorCache, CreatorState},
        token_cache::{TokenCache, TokenState},
    },
    storage::repositories::{
        token_repo::TokenRepo, trade_repo::TradeRepo, wallet_repo::WalletRepo,
    },
};

/// Core service — subscribes to the internal event bus and handles the full
/// lifecycle of tokens and trades:
///
///  `TokenCreated`  → persist token + creator wallet, add to in-memory caches
///  `TradeExecuted` → persist trade + wallet, update token & creator state
///  `CreatorActivity` → update creator's last-seen timestamp in cache
///
/// Tokens created *before* the server started are never added to `TokenCache`,
/// so trades for those tokens are silently ignored (the filter is in
/// `handle_trade_executed`).
pub struct TokenService {
    token_cache: Arc<TokenCache>,
    creator_cache: Arc<CreatorCache>,
    token_repo: TokenRepo,
    token_info_repo: crate::storage::repositories::token_info_repo::TokenInfoRepo,
    trade_repo: TradeRepo,
    wallet_repo: WalletRepo,
}

impl TokenService {
    pub fn new(
        pool: PgPool,
        token_cache: Arc<TokenCache>,
        creator_cache: Arc<CreatorCache>,
    ) -> Self {
        Self {
            token_cache,
            creator_cache,
            token_repo: TokenRepo::new(pool.clone()),
            token_info_repo: crate::storage::repositories::token_info_repo::TokenInfoRepo::new(
                pool.clone(),
            ),
            trade_repo: TradeRepo::new(pool.clone()),
            wallet_repo: WalletRepo::new(pool),
        }
    }

    /// Drive the service until the event bus is closed.
    pub async fn run(self, mut event_rx: broadcast::Receiver<InternalEvent>) {
        info!("TokenService: starting");

        loop {
            match event_rx.recv().await {
                Ok(InternalEvent::TokenCreated(e)) => {
                    self.handle_token_created(e).await;
                }
                Ok(InternalEvent::TradeExecuted(e)) => {
                    self.handle_trade_executed(e).await;
                }
                Ok(InternalEvent::CreatorActivityDetected(e)) => {
                    self.handle_creator_activity(e);
                }
                // LiquidityAdded / LiquidityRemoved — handled by a future service
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        "TokenService lagged {n} events — \
                         consider increasing the broadcast channel capacity"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("TokenService: event bus closed — stopping");
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

impl TokenService {
    async fn handle_token_created(&self, e: TokenCreatedEvent) {
        let mint = e.token.mint_address.clone();
        let creator = e.token.creator_wallet.clone();

        // Idempotency guard — shouldn't fire twice but safe to check
        if self.token_cache.contains_key(&mint) {
            debug!("Duplicate TokenCreated for {mint} — skipping");
            return;
        }

        info!(
            name = %e.token.name,
            symbol = %e.token.symbol,
            mint = %mint,
            creator = %creator,
            "New token tracked"
        );

        // Persist token
        if let Err(err) = self.token_repo.insert(&e.token).await {
            warn!("Failed to persist token {mint}: {err}");
        }

        // Upsert creator wallet
        let now = Utc::now();
        if let Err(err) = self
            .wallet_repo
            .upsert(&Wallet::new(creator.clone(), now))
            .await
        {
            warn!("Failed to upsert creator wallet {creator}: {err}");
        }

        // Update creator entry in cache
        self.creator_cache
            .entry(creator.clone())
            .or_insert_with(|| CreatorState::new(creator))
            .add_token(mint.clone());

        // Mark this token as tracked — any future trade event will check this
        let token_state = TokenState::new(e.token);
        let mint_copy = mint.clone();
        let creator_wallet = token_state.token.creator_wallet.clone();
        let last_trade_at = token_state.last_trade_at;
        let is_rugged = self
            .compute_is_rugged(&mint_copy, last_trade_at, &creator_wallet)
            .await;
        if let Err(err) = self
            .token_info_repo
            .upsert_metrics(
                &mint_copy,
                token_state.ath_price,
                token_state.ath_timestamp,
                Some(0),
                0.0,
                token_state.market_cap,
                token_state.trade_count as i64,
                token_state.last_trade_at,
                token_state.current_price,
                is_rugged,
            )
            .await
        {
            warn!("Failed to seed token metrics for {mint_copy}: {err}");
        }
        self.token_cache.insert(mint, token_state);
    }

    async fn handle_trade_executed(&self, e: TradeExecutedEvent) {
        // Clone the fields we need before potentially moving e.trade
        let mint = e.trade.mint_address.clone();
        let wallet = e.trade.wallet_address.clone();

        // Filter: only process trades for tokens we started tracking this session
        if !self.token_cache.contains_key(&mint) {
            return;
        }

        debug!(
            mint = %mint,
            wallet = %wallet,
            kind = ?e.trade.trade_type,
            sol = e.trade.sol_amount,
            "Trade received"
        );

        // Persist trade
        if let Err(err) = self.trade_repo.insert(&e.trade).await {
            warn!("Failed to persist trade {}: {err}", e.trade.tx_signature);
        }

        // Upsert trader wallet
        let now = Utc::now();
        if let Err(err) = self
            .wallet_repo
            .upsert(&Wallet::new(wallet.clone(), now))
            .await
        {
            warn!("Failed to upsert trader wallet {wallet}: {err}");
        }

        // Update token's sliding trade window and compute metrics
        if let Some(mut token_state) = self.token_cache.get_mut(&mint) {
            token_state.add_trade(e.trade.clone());

            // Compute metrics to persist
            let trade_price = e.trade.price_per_token;
            let volume = token_state.volume_sol_total;
            let age_seconds = Utc::now()
                .signed_duration_since(token_state.token.created_at)
                .num_seconds();
            let age_seconds_opt = Some(age_seconds as i64);
            let market_cap = token_state.market_cap;
            let last_trade_at = token_state.last_trade_at;
            let creator_wallet = token_state.token.creator_wallet.clone();
            let trade_count = token_state.trade_count as i64;
            drop(token_state);
            let is_rugged = self
                .compute_is_rugged(&mint, last_trade_at, &creator_wallet)
                .await;

            // Upsert metrics. DB keeps ATH as the highest-ever trade price using GREATEST.
            if let Err(err) = self
                .token_info_repo
                .upsert_metrics(
                    &mint,
                    Some(trade_price),
                    Some(e.trade.block_time),
                    age_seconds_opt,
                    volume,
                    market_cap,
                    trade_count,
                    last_trade_at,
                    Some(trade_price),
                    is_rugged,
                )
                .await
            {
                warn!("Failed to upsert token metrics for {mint}: {err}");
            }
        }

        // If the trader is a known creator, record the trade in their profile too
        if let Some(mut creator_state) = self.creator_cache.get_mut(&wallet) {
            creator_state.add_trade(e.trade);
        }
    }

    async fn compute_is_rugged(
        &self,
        mint: &str,
        last_trade_at: Option<chrono::DateTime<chrono::Utc>>,
        creator_wallet: &str,
    ) -> bool {
        let last_trade_at = match last_trade_at {
            Some(ts) => ts,
            None => return false,
        };

        if Utc::now()
            .signed_duration_since(last_trade_at)
            .num_seconds()
            < crate::config::constants::RUGGED_STALE_SECONDS
        {
            return false;
        }

        if creator_wallet.is_empty() {
            return false;
        }

        match self
            .trade_repo
            .net_token_amount_by_wallet_and_mint(creator_wallet, mint)
            .await
        {
            Ok(balance) => balance <= 0.0,
            Err(err) => {
                warn!("Failed to compute rugged status for {mint}: {err}");
                false
            }
        }
    }

    /// Update the creator's `last_updated` timestamp when any creator activity
    /// is detected (buy/sell on their own token, liquidity moves, etc.)
    fn handle_creator_activity(&self, e: CreatorActivityEvent) {
        if let Some(mut creator_state) = self.creator_cache.get_mut(&e.creator_wallet) {
            creator_state.last_updated = Utc::now();
        }
    }
}
