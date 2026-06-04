use chrono::Utc;

use crate::{
    config::constants::RUGGED_STALE_SECONDS,
    ingest::db_writer::TokenMetricsWrite,
    state::token_cache::TokenState,
    storage::repositories::trade_repo::TradeRepo,
};

/// Replay all trades in chronological order and rebuild aggregate metrics.
pub fn recompute_token_state(state: &mut TokenState) {
    let token = state.token.clone();
    let trades = std::mem::take(&mut state.trades);
    let is_migrated = state.is_migrated;

    let mut fresh = TokenState::new(token);
    fresh.is_migrated = is_migrated;
    for trade in trades {
        fresh.add_trade(trade);
    }
    *state = fresh;
}

pub fn metrics_from_state(mint: &str, state: &TokenState, recompute_rugged: bool) -> TokenMetricsWrite {
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

pub async fn compute_is_rugged(trade_repo: &TradeRepo, m: &TokenMetricsWrite) -> bool {
    let last_trade_at = match m.last_trade_at {
        Some(ts) => ts,
        None => return false,
    };

    if Utc::now()
        .signed_duration_since(last_trade_at)
        .num_seconds()
        < RUGGED_STALE_SECONDS
    {
        return false;
    }

    if m.creator_wallet.is_empty() {
        return false;
    }

    match trade_repo
        .net_token_amount_by_wallet_and_mint(&m.creator_wallet, &m.mint)
        .await
    {
        Ok(balance) => balance <= 0.0,
        Err(_) => false,
    }
}
