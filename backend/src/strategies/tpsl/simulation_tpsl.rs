use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::trade::TradeType;
use crate::state::app_state::AppState;
use crate::storage::repositories::strategy_tpsl_rule_repo::StrategyTPSLRuleRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;

/// Per-token simulation result.
#[derive(serde::Serialize)]
pub struct SimulatedTokenResult {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
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
    /// "TakeProfit", "StopLoss", or "Open"
    pub exit_reason: String,
    pub total_trades: usize,
}

/// Aggregate statistics for the whole simulation run.
#[derive(serde::Serialize)]
pub struct SimulationSummary {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub tokens_matched: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub open_count: usize,
    pub win_rate_pct: f64,
    pub total_pnl_sol: f64,
    pub avg_pnl_pct: Option<f64>,
    pub avg_holding_secs: Option<f64>,
    pub best_pnl_pct: Option<f64>,
    pub worst_pnl_pct: Option<f64>,
    pub tokens: Vec<SimulatedTokenResult>,
}

// Entry / exit helpers (copied from handlers)
fn find_entry(
    trades: &[crate::models::trade::Trade],
    second_block_cap: usize,
) -> Option<(f64, String, DateTime<Utc>)> {
    if trades.is_empty() {
        return None;
    }

    let first_slot = trades[0].slot;
    let second_slot = trades.iter().find(|t| t.slot > first_slot).map(|t| t.slot)?;

    let mut candidates: Vec<&crate::models::trade::Trade> = Vec::new();

    for t in trades.iter() {
        if t.trade_type != TradeType::Buy {
            continue;
        }
        if t.slot == first_slot {
            candidates.push(t);
        } else if t.slot == second_slot {
            let already = candidates.iter().filter(|c| c.slot == second_slot).count();
            if already < second_block_cap {
                candidates.push(t);
            }
        }
    }

    candidates
        .into_iter()
        .max_by(|a, b| a.price_per_token.partial_cmp(&b.price_per_token).unwrap_or(std::cmp::Ordering::Equal))
        .map(|t| (t.price_per_token, t.tx_signature.clone(), t.block_time))
}

fn find_exit(
    trades: &[crate::models::trade::Trade],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    take_profit_pct: f64,
    stop_loss_pct: f64,
) -> Option<(f64, String, DateTime<Utc>, String)> {
    let later: Vec<&crate::models::trade::Trade> = trades
        .iter()
        .filter(|t| t.block_time > entry_time)
        .collect();

    for t in later.iter() {
        if entry_price <= 0.0 {
            break;
        }
        let pct = ((t.price_per_token - entry_price) / entry_price) * 100.0;
        let triggered = pct >= take_profit_pct || pct <= -stop_loss_pct;
        if !triggered {
            continue;
        }

        let reason = if pct >= take_profit_pct { "TakeProfit" } else { "StopLoss" };
        let exit_slot = t.slot;

        let exit_candidates: Vec<&crate::models::trade::Trade> =
            later.iter().copied().filter(|t| t.slot == exit_slot).collect();

        let exit_trade = exit_candidates
            .into_iter()
            .min_by(|a, b| a.price_per_token.partial_cmp(&b.price_per_token).unwrap_or(std::cmp::Ordering::Equal));

        if let Some(et) = exit_trade {
            return Some((et.price_per_token, et.tx_signature.clone(), et.block_time, reason.to_string()));
        }
    }
    None
}

/// Simulate a TPSL rule; returns an HttpResponse with the serialized summary.
pub async fn run_simulation(
    app_state: actix_web::web::Data<Arc<AppState>>,
    rule_id: Uuid,
) -> Result<SimulationSummary> {
    let rule_repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    let rule = rule_repo
        .find_by_id(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .ok_or_else(|| anyhow!("Rule not found"))?;

    let token_repo = TokenRepo::new(app_state.db.clone());

    let has_initial_buy = rule.p_initial_buy_sol.is_some();
    let has_cu_limit = rule.p_cu_limit.is_some();
    let has_cu_price = rule.p_cu_price.is_some();
    let has_ix_labels = rule.p_ix_labels.as_array().map_or(false, |a| !a.is_empty());
    if !has_initial_buy && !has_cu_limit && !has_cu_price && !has_ix_labels {
        return Err(anyhow!("All rule criteria are empty — simulation would match every token"));
    }

    let tokens = token_repo
        .find_by_rule_criteria(
            rule.p_initial_buy_sol,
            Some(rule.tolerance_pct),
            rule.p_cu_limit,
            rule.p_cu_price,
            rule.p_max_sol_cost,
            rule.p_spendable_sol_in,
            Some(&rule.p_ix_labels),
            None,
        )
        .await
        .map_err(|e| anyhow!("DB error fetching tokens: {e}"))?;

    let trade_repo = TradeRepo::new(app_state.db.clone());
    let mut results: Vec<SimulatedTokenResult> = Vec::with_capacity(tokens.len());

    for token in &tokens {
        let trades = match trade_repo.find_by_mint_all(&token.mint_address).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Skipping {}: trade fetch failed: {e}", token.mint_address);
                continue;
            }
        };

        let Some((entry_price, entry_tx, entry_time)) = find_entry(&trades, 1) else {
            continue;
        };

        let exit = find_exit(&trades, entry_time, entry_price, rule.take_profit, rule.stop_loss);

        let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
            match exit {
                Some((ep, et, etime, reason)) => {
                    let secs = (etime - entry_time).num_seconds();
                    let pct = ((ep - entry_price) / entry_price) * 100.0;
                    let sol = rule.buy_amount * (pct / 100.0);
                    (Some(ep), Some(et), Some(etime), reason, Some(secs), Some(pct), Some(sol))
                }
                None => (None, None, None, "Open".to_string(), None, None, None),
            };

        results.push(SimulatedTokenResult {
            mint: token.mint_address.clone(),
            symbol: token.symbol.clone(),
            entry_price,
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
        });
    }

    // Aggregate
    let tokens_matched = results.len();
    let win_count = results.iter().filter(|r| r.exit_reason == "TakeProfit").count();
    let loss_count = results.iter().filter(|r| r.exit_reason == "StopLoss").count();
    let open_count = results.iter().filter(|r| r.exit_reason == "Open").count();

    let closed: Vec<&SimulatedTokenResult> = results.iter().filter(|r| r.exit_reason != "Open").collect();

    let win_rate_pct = if !closed.is_empty() {
        (win_count as f64 / closed.len() as f64) * 100.0
    } else {
        0.0
    };

    let total_pnl_sol: f64 = results.iter().filter_map(|r| r.pnl_sol).sum();

    let avg_pnl_pct = if !closed.is_empty() {
        let sum: f64 = closed.iter().filter_map(|r| r.pnl_percent).sum();
        Some(sum / closed.len() as f64)
    } else {
        None
    };

    let avg_holding_secs = if !closed.is_empty() {
        let sum: f64 = closed.iter().filter_map(|r| r.holding_secs).map(|s| s as f64).sum();
        Some(sum / closed.len() as f64)
    } else {
        None
    };

    let best_pnl_pct = results.iter().filter_map(|r| r.pnl_percent).reduce(f64::max);
    let worst_pnl_pct = results.iter().filter_map(|r| r.pnl_percent).reduce(f64::min);

    results.sort_by(|a, b| {
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "StopLoss" => 1,
            _ => 2,
        };
        rank(&a.exit_reason)
            .cmp(&rank(&b.exit_reason))
            .then_with(|| b.pnl_percent.unwrap_or(0.0).partial_cmp(&a.pnl_percent.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
    });

    let summary = SimulationSummary {
        rule_id,
        rule_name: rule.rule_name,
        tokens_matched,
        win_count,
        loss_count,
        open_count,
        win_rate_pct,
        total_pnl_sol,
        avg_pnl_pct,
        avg_holding_secs,
        best_pnl_pct,
        worst_pnl_pct,
        tokens: results,
    };

    Ok(summary)
}
