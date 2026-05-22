#![allow(dead_code)]

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::{
    analyzers::{
        CreatorAnalyzer, VolumeAnalyzer, MIN_TRADES_FOR_VOLUME_ANALYSIS, VOLUME_ANALYSIS_INTERVAL,
    },
    models::events::{InternalEvent, TokenCreatedEvent, TradeExecutedEvent},
    state::{
        creator_cache::{CreatorCache, CreatorState},
        token_cache::TokenCache,
    },
    storage::repositories::{analysis_repo::AnalysisRepo, wallet_repo::WalletRepo},
};

/// Subscribes to the internal event bus and runs analysis after key events:
///
/// - `TokenCreated`  → creator analysis (seed the profile)
/// - `TradeExecuted` → volume analysis every `VOLUME_ANALYSIS_INTERVAL` trades;
///                     creator analysis if the trader is a known creator
///
/// Results are persisted to the `tokens_analysis` and `creator_profiles` tables
/// and fed back into the in-memory `CreatorCache` for live scoring.
pub struct AnalyzerService {
    token_cache: Arc<TokenCache>,
    creator_cache: Arc<CreatorCache>,
    analysis_repo: AnalysisRepo,
    // WalletRepo kept for future "flag wallet" writes
    _wallet_repo: WalletRepo,
}

impl AnalyzerService {
    pub fn new(
        pool: PgPool,
        token_cache: Arc<TokenCache>,
        creator_cache: Arc<CreatorCache>,
    ) -> Self {
        Self {
            token_cache,
            creator_cache,
            analysis_repo: AnalysisRepo::new(pool.clone()),
            _wallet_repo: WalletRepo::new(pool),
        }
    }

    pub async fn run(self, mut event_rx: broadcast::Receiver<InternalEvent>) {
        info!("AnalyzerService: starting");

        loop {
            match event_rx.recv().await {
                Ok(InternalEvent::TokenCreated(e)) => {
                    self.on_token_created(e).await;
                }
                Ok(InternalEvent::TradeExecuted(e)) => {
                    self.on_trade_executed(e).await;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("AnalyzerService lagged {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("AnalyzerService: event bus closed — stopping");
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

impl AnalyzerService {
    /// Seed the creator profile as soon as a new token appears.
    async fn on_token_created(&self, e: TokenCreatedEvent) {
        let creator = &e.token.creator_wallet;
        let mint = &e.token.mint_address;

        // Ensure creator entry exists in cache before analyzing.
        // AnalyzerService and TokenService both subscribe to the same broadcast,
        // so we may race ahead of TokenService — inserting here is idempotent
        // (DashMap entry + add_token deduplicate).
        self.creator_cache
            .entry(creator.clone())
            .or_insert_with(|| CreatorState::new(creator.clone()))
            .add_token(mint.clone());

        // Acquire the lock, compute, then DROP it before any .await
        let analysis = self
            .creator_cache
            .get(creator)
            .map(|state| CreatorAnalyzer::analyze(&state, Some(mint)));
        // Lock is released here — safe to .await below

        if let Some((result, profile)) = analysis {
            debug!(
                creator = %creator,
                score = result.score,
                "Creator analysis on token creation"
            );
            self.persist_result_and_profile(result, Some(profile), creator)
                .await;
        }
    }

    /// Run volume and creator analysis after trade events, throttled by
    /// `VOLUME_ANALYSIS_INTERVAL`.
    async fn on_trade_executed(&self, e: TradeExecutedEvent) {
        let mint = &e.trade.mint_address;
        let trader = &e.trade.wallet_address;

        // Volume analysis — compute while holding the lock, drop lock before .await
        let volume_result = self.token_cache.get(mint).and_then(|token_state| {
            let count = token_state.trade_count;
            if count >= MIN_TRADES_FOR_VOLUME_ANALYSIS && count % VOLUME_ANALYSIS_INTERVAL == 0 {
                Some(VolumeAnalyzer::analyze(&token_state))
            } else {
                None
            }
        });
        // Lock released here

        if let Some(result) = volume_result {
            debug!(
                mint = %mint,
                score = result.score,
                indicators = ?result.indicators,
                "Volume analysis"
            );
            if let Err(err) = self.analysis_repo.upsert_result(&result).await {
                warn!("Failed to persist volume analysis for {mint}: {err}");
            }
        }

        // Creator analysis — compute while holding the lock, drop lock before .await
        // Persist for any known creator (score 0.0 is valid baseline data).
        let creator_analysis = self
            .creator_cache
            .get(trader)
            .map(|creator_state| CreatorAnalyzer::analyze(&creator_state, Some(mint)));
        // Lock released here

        if let Some((result, profile)) = creator_analysis {
            debug!(
                trader = %trader,
                score = result.score,
                "Creator analysis on trade"
            );
            let new_score = result.score;
            self.persist_result_and_profile(result, Some(profile), trader)
                .await;

            // Now safe to acquire a write lock — no read lock is held
            if let Some(mut cs) = self.creator_cache.get_mut(trader) {
                cs.update_score(new_score);
            }
        }
    }

    async fn persist_result_and_profile(
        &self,
        result: crate::models::analysis::AnalysisResult,
        profile: Option<crate::models::analysis::CreatorProfile>,
        wallet: &str,
    ) {
        // Perform DB writes in a detached task to avoid blocking the
        // AnalyzerService loop. Clone the repo (holds a PgPool) and move
        // values into the background task.
        let repo = self.analysis_repo.clone();
        let wallet = wallet.to_string();
        tokio::spawn(async move {
            if let Err(err) = repo.upsert_result(&result).await {
                warn!("Failed to persist analysis result for {}: {}", wallet, err);
            }
            if let Some(p) = profile {
                if let Err(err) = repo.upsert_creator_profile(&p).await {
                    warn!("Failed to persist creator profile for {}: {}", wallet, err);
                }
            }
        });
    }
}
