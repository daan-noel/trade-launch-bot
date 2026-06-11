use chrono::Utc;
use tracing::warn;

use crate::{
    config::constants::{
        RUGGED_COHORT_EXIT_RATIO, RUGGED_COHORT_MIN_SHARE, RUGGED_EARLY_SLOT_WINDOW,
        RUGGED_MIN_PEAK_SOL, RUGGED_RESERVE_DRAWDOWN_RATIO, RUGGED_STALE_SECONDS,
    },
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

/// Decide whether a token is rugged. All signals are gated behind staleness — an
/// actively trading token is never flagged — and any one of them is sufficient.
///
/// 1. **Liquidity collapse** (spoof-proof): real SOL reserves crashed from their
///    peak. Wash trading across many wallets nets ~zero SOL, so it cannot mask
///    this; covers both bonding-curve and post-migration AMM rugs.
/// 2. **Early-buyer cohort exit**: the launch sniper/bundler cohort that
///    controlled the token has dumped almost everything it bought. Generalises
///    the legacy single-creator check to the whole insider cluster, defeating
///    the "40-50 wallets doing fake trades" pattern.
/// 3. **Creator dump** (legacy): the creator wallet itself net-sold everything.
pub async fn compute_is_rugged(
    trade_repo: &TradeRepo,
    mint: &str,
    creator_wallet: &str,
    last_trade_at: Option<chrono::DateTime<Utc>>,
) -> bool {
    let last_trade_at = match last_trade_at {
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

    // ── Signal 1: liquidity collapse ─────────────────────────────────────────
    match trade_repo.real_sol_reserve_extremes(mint).await {
        Ok(Some((peak, latest))) => {
            if peak >= RUGGED_MIN_PEAK_SOL && latest <= peak * RUGGED_RESERVE_DRAWDOWN_RATIO {
                return true;
            }
        }
        Ok(None) => {}
        Err(err) => warn!("rugged reserve check {}: {err}", mint),
    }

    // ── Signal 2: early-buyer cohort exit ────────────────────────────────────
    match trade_repo
        .early_buyer_cohort_net(mint, RUGGED_EARLY_SLOT_WINDOW)
        .await
    {
        Ok((cohort_bought, cohort_net, total_bought)) => {
            let dominant =
                total_bought > 0.0 && cohort_bought >= total_bought * RUGGED_COHORT_MIN_SHARE;
            let exited =
                cohort_bought > 0.0 && cohort_net <= cohort_bought * RUGGED_COHORT_EXIT_RATIO;
            if dominant && exited {
                return true;
            }
        }
        Err(err) => warn!("rugged cohort check {}: {err}", mint),
    }

    // ── Signal 3: creator wallet dump (legacy) ───────────────────────────────
    if creator_wallet.is_empty() {
        return false;
    }

    match trade_repo
        .net_token_amount_by_wallet_and_mint(creator_wallet, mint)
        .await
    {
        Ok(balance) => balance <= 0.0,
        Err(err) => {
            warn!("rugged check {}: {err}", mint);
            false
        }
    }
}
