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
fn find_entry(
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
        .map(|t| t.slot)?;

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
        .max_by(|a, b| {
            a.price_per_token
                .partial_cmp(&b.price_per_token)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
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

        let reason = if pct >= take_profit_pct {
            "TakeProfit"
        } else {
            "StopLoss"
        };
        let exit_slot = t.slot;

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
    max_holding_tokens: Option<usize>,
    total_max_trade_tokens: Option<usize>,
) -> Vec<SimulatedTokenResult> {
    let mut active_exits: Vec<Option<DateTime<Utc>>> = Vec::new();
    let mut results: Vec<SimulatedTokenResult> = Vec::new();
    let mut selected_count: usize = 0;

    for (entry_time, exit_time, result) in candidates {
        if let Some(total_max) = total_max_trade_tokens {
            if selected_count >= total_max {
                break;
            }
        }

        active_exits.retain(|active_exit| match active_exit {
            Some(exit_time) => *exit_time > entry_time,
            None => true,
        });

        if let Some(max_holding) = max_holding_tokens {
            if active_exits.len() >= max_holding {
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

    let has_initial_buy = rule.p_initial_buy_sol.is_some();
    let has_cu_limit = rule.p_cu_limit.is_some();
    let has_cu_price = rule.p_cu_price.is_some();
    let has_max_sol_cost = rule.p_max_sol_cost.is_some();
    let has_spendable_sol_in = rule.p_spendable_sol_in.is_some();
    let has_ix_labels = rule.p_ix_labels.as_array().map_or(false, |a| !a.is_empty());
    if !has_initial_buy
        && !has_cu_limit
        && !has_cu_price
        && !has_max_sol_cost
        && !has_spendable_sol_in
        && !has_ix_labels
    {
        return Err(anyhow!(
            "All rule criteria are empty — simulation would match every token"
        ));
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
    let max_holding = rule.p_max_holding_tokens.map(|v| v as usize);
    let total_max_trade_tokens = rule.p_total_max_trade_tokens.map(|v| v as usize);

    if total_max_trade_tokens == Some(0) || max_holding == Some(0) {
        return Ok(vec![]);
    }

    let mut results = select_simulated_tokens(candidates, max_holding, total_max_trade_tokens);

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn make_candidate(
        base: DateTime<Utc>,
        entry_offset: i64,
        exit_offset: Option<i64>,
    ) -> (DateTime<Utc>, Option<DateTime<Utc>>, SimulatedTokenResult) {
        let entry_time = base + Duration::seconds(entry_offset);
        let exit_time = exit_offset.map(|offset| base + Duration::seconds(offset));
        let exit_time_clone = exit_time.clone();

        (
            entry_time,
            exit_time_clone,
            SimulatedTokenResult {
                mint: format!("mint-{}", entry_offset),
                symbol: "SYM".to_string(),
                entry_price: 1.0,
                entry_amount: 1.0,
                entry_tx: format!("tx-{}", entry_offset),
                entry_time,
                exit_price: None,
                exit_tx: None,
                exit_time,
                holding_secs: None,
                pnl_percent: None,
                pnl_sol: None,
                exit_reason: "Open".to_string(),
                total_trades: 0,
            },
        )
    }

    #[test]
    fn select_simulated_tokens_respects_total_holding_limit() {
        let base = Utc::now();
        let candidates = vec![
            make_candidate(base, 0, Some(10)),
            make_candidate(base, 1, Some(2)),
            make_candidate(base, 2, None),
            make_candidate(base, 3, Some(6)),
            make_candidate(base, 7, Some(9)),
            make_candidate(base, 11, Some(15)),
        ];

        let selected = select_simulated_tokens(candidates, Some(2), None);
        let selected_entry_offsets: Vec<i64> = selected
            .iter()
            .map(|result| result.entry_time.timestamp() - base.timestamp())
            .collect();

        assert_eq!(selected_entry_offsets, vec![0, 1, 2, 11]);
    }

    #[test]
    fn select_simulated_tokens_respects_total_max_trade_limit() {
        let base = Utc::now();
        let candidates = vec![
            make_candidate(base, 0, Some(10)),
            make_candidate(base, 1, Some(2)),
            make_candidate(base, 2, None),
            make_candidate(base, 3, Some(6)),
        ];

        let selected = select_simulated_tokens(candidates, None, Some(2));
        let selected_entry_offsets: Vec<i64> = selected
            .iter()
            .map(|result| result.entry_time.timestamp() - base.timestamp())
            .collect();

        assert_eq!(selected_entry_offsets, vec![0, 1]);
    }

    #[test]
    fn select_simulated_tokens_respects_both_limits() {
        let base = Utc::now();
        let candidates = vec![
            make_candidate(base, 0, Some(10)),
            make_candidate(base, 1, Some(2)),
            make_candidate(base, 2, None),
            make_candidate(base, 3, Some(6)),
            make_candidate(base, 7, Some(9)),
            make_candidate(base, 11, Some(15)),
        ];

        let selected = select_simulated_tokens(candidates, Some(2), Some(3));
        let selected_entry_offsets: Vec<i64> = selected
            .iter()
            .map(|result| result.entry_time.timestamp() - base.timestamp())
            .collect();

        assert_eq!(selected_entry_offsets, vec![0, 1, 2]);
    }
}
