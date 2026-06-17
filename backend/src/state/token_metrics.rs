use std::sync::Arc;

use chrono::Utc;

use crate::{
    ingest_laserstream::db_writer::TokenMetricsWrite, state::token_cache::TokenState,
};

/// Replay all trades in chronological order and rebuild aggregate metrics.
pub fn recompute_token_state(state: &mut TokenState) {
    let token = state.token.clone();
    let trades = std::mem::take(&mut state.trades);
    let is_migrated = state.is_migrated;

    let mut fresh = TokenState::new(token);
    fresh.is_migrated = is_migrated;
    // `trades` is the only Arc holder here (taken out of `state`), so unwrap moves
    // the Vec without copying; fall back to a clone only if somehow shared.
    let trades = Arc::try_unwrap(trades).unwrap_or_else(|a| (*a).clone());
    for trade in trades {
        fresh.add_cached_trade(trade);
    }
    *state = fresh;
}

pub fn metrics_from_state(mint: &str, state: &TokenState) -> TokenMetricsWrite {
    let now = Utc::now();
    let age_seconds = now
        .signed_duration_since(state.token.created_at)
        .num_seconds();
    TokenMetricsWrite {
        mint: mint.to_string(),
        ath_price: state.ath_price,
        ath_timestamp: state.ath_timestamp,
        age_seconds: Some(age_seconds),
        volume: state.volume_sol_total,
        market_cap: state.market_cap,
        trade_count: state.trade_count as i64,
        last_trade_at: state.last_trade_at,
        current_price: state.current_price,
        is_migrated: state.is_migrated,
        // Cheap in-memory verdict (no DB scan) — see `TokenState::is_dead`.
        is_dead: state.is_dead(now),
    }
}
