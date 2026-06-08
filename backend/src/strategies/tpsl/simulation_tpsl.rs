use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::trade::TradeType;
use crate::state::app_state::AppState;
use crate::storage::repositories::strategy_tpsl_rule_repo::StrategyTPSLRuleRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;
use crate::strategies::tpsl::handler_tpsl::token_matches_rule;
use super::util::{ignore_zero_f64, ignore_zero_u64};

/// Per-token simulation result.
#[derive(Clone, serde::Serialize)]
pub struct SimulatedTokenResult {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
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
    /// "TakeProfit", "StopLoss", or "Open"
    pub exit_reason: String,
    pub total_trades: usize,
}

// Entry / exit helpers (copied from handlers)
pub fn find_entry(
    trades: &[crate::models::trade::Trade],
    second_block_cap: usize,
) -> Option<(f64, String, DateTime<Utc>)> {
    if trades.is_empty() {
        return None;
    }

    let first_slot = trades[0].slot;
    let second_slot = trades
        .iter()
        .find(|t| t.slot > first_slot)
        .map(|t| t.slot);

    let mut candidates: Vec<&crate::models::trade::Trade> = Vec::new();

    for t in trades.iter() {
        if t.trade_type != TradeType::Buy {
            continue;
        }
        if t.slot == first_slot {
            candidates.push(t);
        } else if let Some(second_slot) = second_slot {
            if t.slot == second_slot {
                let already = candidates.iter().filter(|c| c.slot == second_slot).count();
                if already < second_block_cap {
                    candidates.push(t);
                }
            }
        }
    }

    // Entry price: highest price in first block and 1-5th txs of second block
    candidates
        .into_iter()
        .max_by(|a, b| {
            a.price_per_token
                .partial_cmp(&b.price_per_token)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|t| (t.price_per_token, t.tx_signature.clone(), t.block_time))
}

pub fn find_exit(
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

        let reason = if pct >= take_profit_pct {
            "TakeProfit"
        } else {
            "StopLoss"
        };
        let exit_slot = t.slot;

        // Exit price: lowest price in the block where exit condition met
        let exit_candidates: Vec<&crate::models::trade::Trade> = later
            .iter()
            .copied()
            .filter(|t| t.slot == exit_slot)
            .collect();

        let exit_trade = exit_candidates.into_iter().min_by(|a, b| {
            a.price_per_token
                .partial_cmp(&b.price_per_token)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(et) = exit_trade {
            return Some((
                et.price_per_token,
                et.tx_signature.clone(),
                et.block_time,
                reason.to_string(),
            ));
        }
    }
    None
}

fn select_simulated_tokens(
    candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, SimulatedTokenResult)>,
    max_concurrent_tokens: Option<usize>,
    max_total_tokens: Option<usize>,
) -> Vec<SimulatedTokenResult> {
    let mut active_exits: Vec<Option<DateTime<Utc>>> = Vec::new();
    let mut results: Vec<SimulatedTokenResult> = Vec::new();
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

/// Simulate a TPSL rule; returns token-level simulation results.
pub async fn run_simulation(
    app_state: actix_web::web::Data<Arc<AppState>>,
    rule_id: Uuid,
) -> Result<Vec<SimulatedTokenResult>> {
    let rule_repo = StrategyTPSLRuleRepo::new(app_state.db.clone());

    let rule = rule_repo
        .find_by_id(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .ok_or_else(|| anyhow!("Rule not found"))?;

    let token_repo = TokenRepo::new(app_state.db.clone());

    let max_concurrent_tokens = ignore_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
    let max_total_tokens = ignore_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

    let all_tokens = token_repo
        .find_all()
        .await
        .map_err(|e| anyhow!("DB error fetching tokens: {e}"))?;

    let tokens: Vec<_> = all_tokens
        .into_iter()
        .filter(|t| token_matches_rule(t, &rule))
        .collect();

    let trade_repo = TradeRepo::new(app_state.db.clone());
    let mut candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, SimulatedTokenResult)> =
        Vec::with_capacity(tokens.len());

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

        let exit = find_exit(
            &trades,
            entry_time,
            entry_price,
            rule.take_profit,
            rule.stop_loss,
        );

        let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
            match exit {
                Some((ep, et, etime, reason)) => {
                    let secs = (etime - entry_time).num_seconds();
                    let pct = ((ep - entry_price) / entry_price) * 100.0;
                    let sol = rule.buy_amount * (pct / 100.0);
                    (
                        Some(ep),
                        Some(et),
                        Some(etime),
                        reason,
                        Some(secs),
                        Some(pct),
                        Some(sol),
                    )
                }
                None => (None, None, None, "Open".to_string(), None, None, None),
            };

        let result = SimulatedTokenResult {
            mint: token.mint_address.clone(),
            symbol: token.symbol.clone(),
            entry_price,
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
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "StopLoss" => 1,
            _ => 2,
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
