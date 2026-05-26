use std::sync::Arc;

use sqlx::PgPool;
use tracing::info;

use crate::{
    state::{
        creator_cache::{CreatorCache, CreatorState},
        token_cache::{TokenCache, TokenState, RECENT_TRADES_WINDOW},
    },
    storage::repositories::{
        analysis_repo::AnalysisRepo, token_info_repo::TokenInfoRepo, token_repo::TokenRepo,
        trade_repo::TradeRepo,
    },
};

/// Populate `token_cache` from the database so that historical tokens and their
/// trade stats are immediately available after a server restart.
///
/// Strategy (3 queries total — no N+1):
///   1. Load all tokens.
///   2. Load per-token aggregates (count, volume, last_trade_at) in one GROUP BY.
///   3. Load the most recent `RECENT_TRADES_WINDOW` trades per token using a
///      window function, ordered oldest-first for correct VecDeque ordering.
pub async fn seed_token_cache(pool: &PgPool, token_cache: Arc<TokenCache>) -> anyhow::Result<()> {
    let token_repo = TokenRepo::new(pool.clone());
    let trade_repo = TradeRepo::new(pool.clone());

    // 1. All tokens
    let tokens = token_repo.find_all().await?;
    let total = tokens.len();

    if total == 0 {
        info!("Cache seed: no tokens in DB — nothing to load");
        return Ok(());
    }

    // Seed a TokenState for every token (zero stats for now)
    for token in tokens {
        let mint = token.mint_address.clone();
        token_cache.insert(mint, TokenState::new(token));
    }

    // Prefer values persisted in `tokens_info` to avoid recomputing market cap
    // where unnecessary. Load all stored metrics and populate cache fields.
    let token_info_repo = TokenInfoRepo::new(pool.clone());
    let infos = token_info_repo.list_all().await?;
    for info in infos {
        if let Some(mut state) = token_cache.get_mut(&info.mint_address) {
            if let Some(ath_price) = info.ath_price {
                state.ath_price = Some(ath_price);
                state.ath_timestamp = info.ath_timestamp;
            }
            state.volume_sol_total = info.volume;
            state.trade_count = info.trade_count as u64;
            state.last_trade_at = info.last_trade_at;
            state.market_cap = info.market_cap;
            state.current_price = info.current_price;
            state.is_migrated = info.is_migrated;
        }
    }

    // 2. Trade aggregates — update counts/volumes, reserve baselines, and market cap in cache
    let aggregates = trade_repo.load_all_aggregates().await?;
    for (
        mint,
        trade_count,
        volume_sol_total,
        last_trade_at,
        market_cap,
        current_virtual_token_reserves,
    ) in aggregates
    {
        if let Some(mut state) = token_cache.get_mut(&mint) {
            state.trade_count = trade_count;
            state.volume_sol_total = volume_sol_total;
            state.last_trade_at = last_trade_at;
            // Use configured static initial reserve when seeding.
            state.initial_virtual_token_reserves = state.initial_virtual_token_reserves.or(Some(
                crate::config::constants::INITIAL_VIRTUAL_TOKEN_RESERVES,
            ));
            if state.current_virtual_token_reserves.is_none() {
                state.current_virtual_token_reserves = current_virtual_token_reserves;
            }

            // Only set market_cap from aggregates if we don't already have a
            // persisted value in `tokens_info` (avoid overwriting DB value).
            if state.market_cap.is_none() {
                state.market_cap = market_cap;
            }
        }
    }

    // 3. Recent trades per token — fill the sliding window
    let recent_trades = trade_repo
        .load_recent_per_token(RECENT_TRADES_WINDOW)
        .await?;
    for trade in recent_trades {
        if let Some(mut state) = token_cache.get_mut(&trade.mint_address) {
            state.recent_trades.push_back(trade);
        }
    }

    info!("Cache seed complete: {} tokens loaded from DB", total);

    Ok(())
}

/// Populate `creator_cache` from the `creator_profiles` table so that
/// historical creators are recognised immediately after a server restart.
///
/// Strategy (2 queries):
///   1. Load all creator profiles → seed `CreatorState` with saved scores.
///   2. Load all tokens → populate each creator's `created_tokens` list.
///      (trade_history is not restored — it rebuilds from live events)
pub async fn seed_creator_cache(
    pool: &PgPool,
    creator_cache: Arc<CreatorCache>,
) -> anyhow::Result<()> {
    let analysis_repo = AnalysisRepo::new(pool.clone());
    let token_repo = TokenRepo::new(pool.clone());

    // 1. Load all stored creator profiles
    let (total, profiles) = analysis_repo.list_creator_profiles(i64::MAX, 0).await?;

    if total == 0 {
        info!("Creator cache seed: no profiles in DB — nothing to load");
        return Ok(());
    }

    for profile in &profiles {
        let mut cs = creator_cache
            .entry(profile.wallet_address.clone())
            .or_insert_with(|| CreatorState::new(profile.wallet_address.clone()));
        cs.suspiciousness_score = profile.suspiciousness_score;
    }

    // 2. Populate created_tokens for each cached creator
    let tokens = token_repo.find_all().await?;
    for token in tokens {
        if let Some(mut cs) = creator_cache.get_mut(&token.creator_wallet) {
            cs.add_token(token.mint_address);
        }
    }

    info!(
        "Creator cache seed complete: {} creator profiles loaded from DB",
        total
    );
    Ok(())
}
