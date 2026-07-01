use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use crate::models::trade::Trade;
use crate::models::Token;
use crate::state::local_state::LocalState;
use crate::strategies::sim_progress::SimProgress;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;
// swing1 borrows tpsl1's `none_if_zero_u64` (there is no swing1 `util` module).
use trading_core::models::trade::TradeRow;
use trading_core::strategies::registry::swing1_decision_rule;
use trading_core::strategies::tpsl_sniper_1::util::none_if_zero_u64;

// Reuse tpsl1's per-token result shape verbatim (swing1 has no trigger snapshot,
// exactly like tpsl1) so the shared frontend card/table renders both identically.
pub use crate::strategies::tpsl_sniper_1::backtest::BacktestTokenResult;

/// Candidate mints fetched per batched `trades` query. One round-trip pulls a
/// whole chunk (`find_by_mints_all`) instead of one query per token.
const BACKTEST_FETCH_CHUNK: usize = 16;

/// Concurrent chunk queries. Deliberately small: the PgPool (default 20) is
/// shared with the live ingest pipeline, so a backtest must leave it headroom.
const BACKTEST_FETCH_CONCURRENCY: usize = 3;

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

/// Simulate a swing1 rule over historical DB data; returns token-level results.
/// Shares the entry latch + exit ladder with the live path via
/// [`entry::find_phase_entry`] / [`exit::find_trade_driven_exit`], so a backtest
/// and a live run resolve identical entries and exits.
pub async fn run_backtest(
    app_state: actix_web::web::Data<Arc<LocalState>>,
    rule_id: Uuid,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress_cell: Arc<crate::state::job_progress::ProgressCell>,
) -> Result<Vec<BacktestTokenResult>> {
    // Bound concurrent backtests so overlapping runs can't drain the `batch` pool.
    let _permit = app_state
        .backtest_sem
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow!("backtest concurrency semaphore closed"))?;

    // Load the rule from the unified `strategy_rules` table and rebuild the
    // `Swing1Rule` the decision layer consumes. A rule_id that resolves to a
    // different strategy is "not found" for this swing1 endpoint.
    let strategy_rule = app_state
        .strategy_repo()
        .find_rule(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .filter(|r| r.strategy_id == "swing_1")
        .ok_or_else(|| anyhow!("Rule not found"))?;
    let rule =
        swing1_decision_rule(&strategy_rule).map_err(|e| anyhow!("invalid swing1 rule params: {e}"))?;

    let max_concurrent_tokens = none_if_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
    let max_total_tokens = none_if_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

    // Candidate tokens are scanned from the whole `tokens` table (keyset-streamed
    // within the optional `[since, until)` window). swing1 has no token-creation
    // gate (`token_matches_buy_rule` is always true), so this admits every non-
    // Mayhem fingerprinted candidate; the entry latch decides from the trade stream.
    let repo = TokenRepo::new(app_state.batch_db.clone());
    let tokens: Vec<Token> = crate::strategies::analysis::collect_matching_tokens(
        &repo,
        since,
        until,
        |t| !t.is_mayhem_mode && entry::token_matches_buy_rule(t, &rule),
    )
    .await
    .map_err(|e| anyhow!("candidate token scan failed: {e}"))?;

    let progress = Arc::new(SimProgress::new(
        app_state.sse_tx.clone(),
        rule_id,
        tokens.len(),
        progress_cell,
    ));
    progress.start();

    let rule = Arc::new(rule);
    let chunks: Vec<Vec<Token>> = tokens.chunks(BACKTEST_FETCH_CHUNK).map(<[Token]>::to_vec).collect();
    let per_chunk: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            let trade_repo = TradeRepo::new(app_state.batch_db.clone());
            let rule = rule.clone();
            let progress = progress.clone();
            let token_cache = app_state.token_cache.clone();
            let cache = app_state.backtest_trade_cache.clone();
            let cancel = cancel.clone();
            async move {
                // Cooperative cancel: skip this chunk's fetch + resolve entirely,
                // ticking each candidate so the bar still reaches `total`.
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    for _ in &chunk {
                        progress.tick();
                    }
                    return Vec::new();
                }
                // Freshness key per mint: the in-memory `trade_count` (0 if the
                // token isn't tracked — then the cache simply never hits for it).
                let counts: Vec<u64> = chunk
                    .iter()
                    .map(|t| {
                        token_cache
                            .get(&t.mint_address)
                            .map(|s| s.trade_count)
                            .unwrap_or(0)
                    })
                    .collect();

                // Reuse fresh cached histories; fetch only the misses, batched.
                let mut histories: Vec<Option<Arc<Vec<Trade>>>> = Vec::with_capacity(chunk.len());
                let mut to_fetch: Vec<String> = Vec::new();
                for (token, &count) in chunk.iter().zip(&counts) {
                    match cache.get(&token.mint_address, count) {
                        Some(h) => histories.push(Some(h)),
                        None => {
                            histories.push(None);
                            to_fetch.push(token.mint_address.clone());
                        }
                    }
                }

                let mut grouped = if to_fetch.is_empty() {
                    std::collections::HashMap::new()
                } else {
                    match trade_repo.find_by_mints_all(&to_fetch).await {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::warn!(
                                "Skipping {} of {} tokens: trade fetch failed: {e}",
                                to_fetch.len(),
                                chunk.len()
                            );
                            for _ in &chunk {
                                progress.tick();
                            }
                            return Vec::new();
                        }
                    }
                };

                let mut out = Vec::with_capacity(chunk.len());
                for (i, token) in chunk.iter().enumerate() {
                    let trades: Arc<Vec<Trade>> = match histories[i].take() {
                        Some(h) => h,
                        None => {
                            let h = Arc::new(grouped.remove(&token.mint_address).unwrap_or_default());
                            cache.insert(token.mint_address.clone(), h.clone(), counts[i]);
                            h
                        }
                    };
                    // Tick once per candidate regardless of outcome (no-entry skip
                    // / resolved) so the count always reaches `total`.
                    let resolved = (|| {
                        // swing1 entry: kill→volume latch + higher-low confirm →
                        // worst-case canonical-spot fill at the trigger index. Byte-
                        // identical to the live paper path (both over `trades`).
                        let (_trigger_idx, fill) = entry::find_phase_entry(&trades, &rule)?;
                        let (entry_price, entry_tx, entry_time) =
                            (fill.price, fill.tx_signature, fill.block_time);

                        // All-time high across the token's full history, priced off the
                        // **canonical GMGN spot** (the same price the leg detector and the
                        // fill use) so ATH is on the same scale as entry/exit.
                        let ath_price = trades
                            .iter()
                            .map(|t| t.chart_spot_price().unwrap_or_else(|| t.execution_price()))
                            .fold(entry_price, f64::max);

                        let exit =
                            exit::find_trade_driven_exit(&trades, entry_time, entry_price, &rule);

                        let (
                            exit_price,
                            exit_tx,
                            exit_time,
                            exit_reason,
                            holding_secs,
                            pnl_percent,
                            pnl_sol,
                        ) = match exit {
                            Some(fill) => {
                                let secs = (fill.block_time - entry_time).num_seconds();
                                let pct = ((fill.price - entry_price) / entry_price) * 100.0;
                                let sol = rule.buy_amount * (pct / 100.0);
                                (
                                    Some(fill.price),
                                    Some(fill.tx_signature),
                                    Some(fill.block_time),
                                    fill.reason.as_str().to_string(),
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
                            entry_token_amount: rule.buy_amount,
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

                        Some((entry_time, exit_time, result))
                    })();
                    progress.tick();
                    if let Some(r) = resolved {
                        out.push(r);
                    }
                }
                out
            }
        })
        .collect();

    let mut candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> =
        stream::iter(per_chunk)
            .buffer_unordered(BACKTEST_FETCH_CONCURRENCY)
            .flat_map(stream::iter)
            .collect()
            .await;

    // If a cancel landed mid-run, discard the partial result so the caller can
    // report a clean cancellation instead of an incomplete simulation.
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        anyhow::bail!("simulation cancelled");
    }

    candidates.sort_by_key(|(entry_time, _, _)| *entry_time);

    let mut results = select_simulated_tokens(candidates, max_concurrent_tokens, max_total_tokens);

    results.sort_by(|a, b| {
        // TakeProfit first, then any other closed exit (incl. NextKill / StopLoss /
        // TrailingStop / LiquidityExit / Stall / TimeStop), then still-Open last.
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
