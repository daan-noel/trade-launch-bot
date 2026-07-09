use std::sync::Arc;

use chrono::Utc;

use crate::state::token_cache::TokenState;

/// Aggregate token metrics produced from a [`TokenState`] (by [`metrics_from_state`])
/// and persisted by the ingest db_writer. Lives in core so both the cache-metrics
/// producer and the ingest crate share one definition.
#[derive(Debug, Clone)]
pub struct TokenMetricsWrite {
    pub mint: String,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<chrono::DateTime<Utc>>,
    pub age_seconds: Option<i64>,
    pub volume_sol: f64,
    pub market_cap: Option<f64>,
    pub trade_count: i64,
    pub last_trade_at: Option<chrono::DateTime<Utc>>,
    pub current_price: Option<f64>,
    pub is_migrated: bool,
    /// Cheap in-memory dead-token verdict computed at metrics time (see
    /// [`crate::state::token_cache::TokenState::is_dead`]); persisted as-is.
    pub is_dead: bool,
    /// Seconds from token creation to the last meaningful trade (`amount_sol >=
    /// DEAD_MEANINGFUL_TRADE_SOL`). `Some` only when `is_dead = true`; `None` while
    /// the token is still alive.
    pub lifetime_secs: Option<i64>,
    /// Total buy SOL across trades in the token's creation slot (human SOL;
    /// lamports-scaled at the repo boundary).
    pub first_slot_buy_sol: f64,
    /// Total sell SOL across trades in the token's creation slot.
    pub first_slot_sell_sol: f64,
}

/// Replay all trades in chronological order and rebuild aggregate metrics.
pub fn recompute_token_state(state: &mut TokenState) {
    let token = state.token.clone();
    let trades = std::mem::take(&mut state.trades);
    let is_migrated = state.is_migrated;

    let mut fresh = TokenState::new(token);
    fresh.is_migrated = is_migrated;
    // Carry the wallet interner over: the retained `CachedTrade`s hold `u32` ids in
    // the *old* interner's namespace, and `add_cached_trade` does not re-intern, so
    // `fresh` must keep the same `u32 → address` table or those ids would dangle.
    fresh.interner = std::mem::take(&mut state.interner);
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
    // Compute is_dead once so lifetime_secs can reuse it without a double call.
    let is_dead = state.is_dead(now);
    let lifetime_secs = state.lifetime_secs(now);
    TokenMetricsWrite {
        mint: mint.to_string(),
        ath_price: state.ath_price,
        ath_timestamp: state.ath_timestamp,
        age_seconds: Some(age_seconds),
        volume_sol: state.volume_sol_total,
        market_cap: state.market_cap,
        trade_count: state.trade_count as i64,
        last_trade_at: state.last_trade_at,
        current_price: state.current_price,
        is_migrated: state.is_migrated,
        is_dead,
        lifetime_secs,
        first_slot_buy_sol: state.first_slot_buy_sol,
        first_slot_sell_sol: state.first_slot_sell_sol,
    }
}
