use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use super::util::none_if_zero_u64;
use crate::state::app_state::AppState;
use crate::storage::repositories::tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;

/// Per-token simulation result.
#[derive(Clone, serde::Serialize)]
pub struct BacktestTokenResult {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
    /// All-time-high price across every one of the token's trades.
    pub ath_price: f64,
    pub entry_amount: f64,
    pub entry_tx: String,
    pub entry_time: DateTime<Utc>,
    pub exit_price: Option<f64>,
    pub exit_tx: Option<String>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Seconds from entry to exit (None if still open).
    pub holding_secs: Option<i64>,
    pub pnl_percent: Option<f64>,
    /// PnL in SOL based on the rule's buy_amount.
    pub pnl_sol: Option<f64>,
    /// "LiquidityExit", "TakeProfit", "StopLoss", "TrailingStop", "Stall",
    /// "TimeStop", or "Open"
    pub exit_reason: String,
    pub total_trades: usize,
}

/// Apply the rule's concurrency / total-token caps to entry-time-sorted
/// candidates, mirroring how the live run admits tokens: a token is skipped when
/// `max_concurrent_tokens` are still open at its entry, and admission stops once
/// `max_total_tokens` have been selected.
fn select_simulated_tokens(
    candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)>,
    max_concurrent_tokens: Option<usize>,
    max_total_tokens: Option<usize>,
) -> Vec<BacktestTokenResult> {
    let mut active_exits: Vec<Option<DateTime<Utc>>> = Vec::new();
    let mut results: Vec<BacktestTokenResult> = Vec::new();
    let mut selected_count: usize = 0;

    for (entry_time, exit_time, result) in candidates {
        if let Some(total_max) = max_total_tokens {
            if selected_count >= total_max {
                break;
            }
        }

        active_exits.retain(|active_exit| match active_exit {
            Some(exit_time) => *exit_time > entry_time,
            None => true,
        });

        if let Some(max_open) = max_concurrent_tokens {
            if active_exits.len() >= max_open {
                continue;
            }
        }

        active_exits.push(exit_time);
        results.push(result);
        selected_count += 1;
    }

    results
}

/// Simulate a TPSL rule over historical DB data; returns token-level results.
/// Shares the exit ladder with the live path via [`exit::find_trade_driven_exit`],
/// so a backtest and a live run resolve identical exits.
pub async fn run_backtest(
    app_state: actix_web::web::Data<Arc<AppState>>,
    rule_id: Uuid,
) -> Result<Vec<BacktestTokenResult>> {
    let rule_repo = Tpsl1StrategyRuleRepo::new(app_state.db.clone());

    let rule = rule_repo
        .find_by_id(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .ok_or_else(|| anyhow!("Rule not found"))?;

    let token_repo = TokenRepo::new(app_state.db.clone());

    let max_concurrent_tokens = none_if_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
    let max_total_tokens = none_if_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

    let all_tokens = token_repo
        .find_all()
        .await
        .map_err(|e| anyhow!("DB error fetching tokens: {e}"))?;

    // Never trade Mayhem-Mode tokens: they carry an AI random-walk agent (2B supply,
    // net-sell drift, ±300% noise) for their first 24h — manufactured chaos, not a
    // snipeable edge. Exclude them outright (legacy-only policy, 2026-06 regime).
    let tokens: Vec<_> = all_tokens
        .into_iter()
        .filter(|t| !t.is_mayhem_mode && entry::token_matches_buy_rule(t, &rule))
        .collect();

    let trade_repo = TradeRepo::new(app_state.db.clone());
    let mut candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> =
        Vec::with_capacity(tokens.len());

    for token in &tokens {
        let trades = match trade_repo.find_by_mint_all(&token.mint_address).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Skipping {}: trade fetch failed: {e}", token.mint_address);
                continue;
            }
        };

        let Some(entry) = entry::find_entry_fill_in_trades(&trades, 1) else {
            continue;
        };
        let (entry_price, entry_tx, entry_time) = (entry.price, entry.tx_signature, entry.block_time);

        // All-time high across the token's full trade history.
        let ath_price = trades
            .iter()
            .map(|t| t.price_per_token)
            .fold(entry_price, f64::max);

        let exit = exit::find_trade_driven_exit(&trades, entry_time, entry_price, &rule);

        let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
            match exit {
                Some(fill) => {
                    let secs = (fill.block_time - entry_time).num_seconds();
                    let pct = ((fill.price - entry_price) / entry_price) * 100.0;
                    let sol = rule.buy_amount * (pct / 100.0);
                    (
                        Some(fill.price),
                        Some(fill.tx_signature),
                        Some(fill.block_time),
                        fill.reason.to_string(),
                        Some(secs),
                        Some(pct),
                        Some(sol),
                    )
                }
                None => (None, None, None, "Open".to_string(), None, None, None),
            };

        let result = BacktestTokenResult {
            mint: token.mint_address.clone(),
            symbol: token.symbol.clone(),
            entry_price,
            ath_price,
            entry_amount: rule.buy_amount,
            entry_tx,
            entry_time,
            exit_price,
            exit_tx,
            exit_time,
            holding_secs,
            pnl_percent,
            pnl_sol,
            exit_reason,
            total_trades: trades.len(),
        };

        candidates.push((entry_time, exit_time, result));
    }

    candidates.sort_by_key(|(entry_time, _, _)| *entry_time);

    let mut results = select_simulated_tokens(candidates, max_concurrent_tokens, max_total_tokens);

    results.sort_by(|a, b| {
        // TakeProfit first, then any other closed exit (StopLoss / TrailingStop /
        // future ladder reasons), then still-Open positions last.
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "Open" => 2,
            _ => 1,
        };
        rank(&a.exit_reason)
            .cmp(&rank(&b.exit_reason))
            .then_with(|| {
                b.pnl_percent
                    .unwrap_or(0.0)
                    .partial_cmp(&a.pnl_percent.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    Ok(results)
}
