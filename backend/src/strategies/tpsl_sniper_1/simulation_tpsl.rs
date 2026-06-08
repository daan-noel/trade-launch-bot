use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::trade::TradeType;
use crate::models::StrategyTPSLRule;
use crate::state::app_state::AppState;
use crate::storage::repositories::strategy_tpsl_rule_repo::StrategyTPSLRuleRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;
use super::handler_tpsl::token_matches_rule;
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
    /// "TakeProfit", "StopLoss", "TrailingStop", or "Open"
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

/// Legacy fixed take-profit / stop-loss exit. Retained for the live paper-trading
/// path (`service_tpsl`); the simulation uses [`simulate_exit`].
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

/// Exit-walk skeleton ([P-exit]). Walks post-entry trades chronologically
/// (trades are already slot/time-sorted upstream) while holding running state
/// (`peak_price`) and tests each exit feature on every trade.
///
/// Exit priority (the subset that exists today; the full ladder per the plan is
/// LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop):
///   StopLoss → TakeProfit → TrailingStop.
///
/// Every feature is inert by default: with `p_trailing_stop_pct` unset/zero this
/// reproduces the legacy [`find_exit`] result exactly (StopLoss/TakeProfit only).
pub fn simulate_exit(
    trades: &[crate::models::trade::Trade],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &StrategyTPSLRule,
) -> Option<(f64, String, DateTime<Utc>, String)> {
    if entry_price <= 0.0 {
        return None;
    }

    let take_profit_pct = rule.take_profit;
    let stop_loss_pct = rule.stop_loss;
    // E1 · Trailing stop (None when unset/zero → feature disabled).
    let trailing_stop_pct = ignore_zero_f64(rule.p_trailing_stop_pct);

    let later: Vec<&crate::models::trade::Trade> = trades
        .iter()
        .filter(|t| t.block_time > entry_time)
        .collect();

    // Running state carried across the walk; the peak starts at the entry price.
    let mut peak_price = entry_price;

    for t in later.iter() {
        let price = t.price_per_token;
        if price > peak_price {
            peak_price = price;
        }

        let pct = ((price - entry_price) / entry_price) * 100.0;

        let reason: Option<&str> = if pct <= -stop_loss_pct {
            Some("StopLoss")
        } else if pct >= take_profit_pct {
            Some("TakeProfit")
        } else if let Some(trail) = trailing_stop_pct {
            // E1: bank the reversal once price falls `trail`% below the peak.
            if peak_price > 0.0 && price <= peak_price * (1.0 - trail / 100.0) {
                Some("TrailingStop")
            } else {
                None
            }
        } else {
            None
        };

        let Some(reason) = reason else {
            continue;
        };

        let exit_slot = t.slot;

        // Exit price: lowest price in the block where the exit condition met.
        let exit_trade = later
            .iter()
            .copied()
            .filter(|x| x.slot == exit_slot)
            .min_by(|a, b| {
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

        let exit = simulate_exit(&trades, entry_time, entry_price, &rule);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A buy whose `price_per_token` equals `price` (token_amount = 1.0).
    fn buy(price: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            price, // sol_amount
            1.0,   // token_amount → price_per_token = price
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + chrono::Duration::seconds(secs),
        )
    }

    /// Minimal rule with explicit TP/SL/trailing; everything else inert.
    fn rule_with(take_profit: f64, stop_loss: f64, trailing: Option<f64>) -> StrategyTPSLRule {
        StrategyTPSLRule::new(
            "test".into(),
            None,
            None,
            None,
            serde_json::Value::Array(vec![]),
            "paper".into(),
            1.0, // buy_amount
            take_profit,
            stop_loss,
            None,
            None,
            None,
            None,
            Some(0.0),
            trailing,
        )
    }

    // +200% then reverses; trailing 30% banks the reversal below peak.
    fn moonshot_then_reversal() -> Vec<Trade> {
        vec![
            buy(2.0, 2, 1), // +100%
            buy(3.0, 3, 2), // +200% (peak)
            buy(2.5, 4, 3), // -16.7% off peak (3.0*0.7 = 2.1 floor not hit)
            buy(2.0, 5, 4), // <= 2.1 → trailing fires here
        ]
    }

    #[test]
    fn trailing_stop_fires_on_reversal() {
        let trades = moonshot_then_reversal();
        let rule = rule_with(1000.0, 90.0, Some(30.0));

        let exit = simulate_exit(&trades, base_time(), 1.0, &rule)
            .expect("trailing stop should trigger an exit, not stay Open");

        assert_eq!(exit.3, "TrailingStop");
        // Fills at the first trade at/below peak·(1 − 0.30) = 2.1.
        assert!(exit.0 <= 2.1, "exit price {} should be at/below the trailing floor", exit.0);
        assert!((exit.0 - 2.0).abs() < 1e-9, "expected fill at the 2.0 trade, got {}", exit.0);
    }

    #[test]
    fn disabled_trailing_leaves_position_open() {
        let trades = moonshot_then_reversal();
        // TP unreachable, SL unreachable, trailing disabled → no exit.
        let rule = rule_with(1000.0, 90.0, None);

        assert!(
            simulate_exit(&trades, base_time(), 1.0, &rule).is_none(),
            "with trailing disabled this series must stay Open"
        );

        // Zero behaves identically to None (ignore_zero convention).
        let rule_zero = rule_with(1000.0, 90.0, Some(0.0));
        assert!(simulate_exit(&trades, base_time(), 1.0, &rule_zero).is_none());
    }

    #[test]
    fn stop_loss_takes_priority_over_trailing() {
        // Rises to +200%, then craters past the stop-loss threshold.
        let trades = vec![
            buy(3.0, 2, 1),  // peak
            buy(0.05, 3, 2), // -95% from entry 1.0 → StopLoss (and below trailing floor)
        ];
        let rule = rule_with(1000.0, 90.0, Some(30.0));

        let exit = simulate_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.3, "StopLoss", "stop-loss outranks trailing on the same trade");
    }

    #[test]
    fn disabled_trailing_matches_legacy_find_exit() {
        // Regression guard: trailing off ⇒ identical to the legacy fixed exit.
        let trades = vec![
            buy(1.2, 2, 1),
            buy(0.7, 3, 2), // -30% → StopLoss at SL=20
            buy(2.0, 4, 3),
        ];
        let rule = rule_with(50.0, 20.0, None);

        let legacy = find_exit(&trades, base_time(), 1.0, rule.take_profit, rule.stop_loss);
        let walked = simulate_exit(&trades, base_time(), 1.0, &rule);
        assert_eq!(legacy, walked);
    }
}
