use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use super::util::none_if_zero_u64;
use crate::models::trade::Trade;
use crate::models::Token;
use crate::state::app_state::AppState;
use crate::strategies::sim_progress::SimProgress;
use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;
use crate::storage::repositories::token_repo::TokenRepo;
use crate::storage::repositories::trade_repo::TradeRepo;

/// Candidate mints fetched per batched `trades` query. One round-trip pulls a
/// whole chunk (`find_by_mints_all`) instead of one query per token.
const BACKTEST_FETCH_CHUNK: usize = 16;

/// Concurrent chunk queries. Deliberately small: the PgPool (default 20) is
/// shared with the live ingest pipeline, so a backtest must leave it headroom.
/// `CHUNK * CONCURRENCY` also bounds how many token histories are resident at
/// once. Old code held 16 connections at concurrency 16; this holds ~3.
const BACKTEST_FETCH_CONCURRENCY: usize = 3;

/// Per-token simulation result.
#[derive(Clone, serde::Serialize)]
pub struct BacktestTokenResult {
    pub mint: String,
    pub symbol: String,
    /// Trigger-trade (scalp signal) snapshot — the trade that *armed* the
    /// position, distinct from the worst-case `entry_*` fill below. The gap
    /// between the two is the modeled adverse slippage (mirrors live/paper).
    /// `None` only for legacy paper rows that never recorded a target.
    pub target_price: Option<f64>,
    /// Trigger trade's **token** count (SOL derived at display as `price × tokens`).
    pub target_token_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    pub entry_price: f64,
    /// All-time-high price across every one of the token's trades.
    pub ath_price: f64,
    /// Tokens bought at entry (`buy_amount / entry_price`); SOL derived at display.
    pub entry_token_amount: f64,
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
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress_cell: Arc<crate::state::job_progress::ProgressCell>,
) -> Result<Vec<BacktestTokenResult>> {
    let rule_repo = Tpsl2StrategyRuleRepo::new(app_state.db.clone());

    let rule = rule_repo
        .find_by_id(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .ok_or_else(|| anyhow!("Rule not found"))?;

    let max_concurrent_tokens = none_if_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
    let max_total_tokens = none_if_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

    // Scalp trade-window gates are the only entry path (the legacy first-slot fill
    // was removed): a tpsl2 rule MUST configure at least one scalp gate, else it can
    // never resolve an entry. Token-level criteria (p_token_*) remain an optional
    // pre-filter for *which* tokens to weigh, not a fill resolver. The API maps this
    // exact message to a 400.
    if !entry::rule_configures_any_scalp_gate(&rule) {
        return Err(anyhow!("Rule configures no scalp entry gate"));
    }

    // Candidate tokens are scanned from the **whole** `tokens` table (keyset-
    // streamed within the optional `[since, until)` window), decoupled from the
    // live `token_cache` so an old, evicted-but-matching mint is still simulated.
    // Only the sparse matches stay resident — never the table.
    //
    // Never trade Mayhem-Mode tokens: they carry an AI random-walk agent (2B supply,
    // net-sell drift, ±300% noise) for their first 24h — manufactured chaos, not a
    // snipeable edge. Exclude them outright (legacy-only policy, 2026-06 regime).
    //
    // Token-level pre-filter: every *configured* token criterion must hold. A
    // scalp rule may set none (its gating happens on the trade stream), so this is
    // the vacuous-true variant rather than `token_matches_buy_rule`.
    // Heavy DB work (the whole-table token scan + the batched trade fetches below)
    // runs on the dedicated batch pool so a backtest can't starve dashboard reads.
    let repo = TokenRepo::new(app_state.batch_db.clone());
    let tokens: Vec<Token> = crate::strategies::analysis::collect_matching_tokens(
        &repo,
        since,
        until,
        |t| !t.is_mayhem_mode && entry::token_criteria_satisfied(t, &rule),
    )
    .await
    .map_err(|e| anyhow!("candidate token scan failed: {e}"))?;

    // Resolve each candidate's entry/exit from its trade history. The history is
    // fetched in batched chunks — one query per `BACKTEST_FETCH_CHUNK` mints
    // (`find_by_mints_all`) instead of one query per token — and only a few chunk
    // queries run concurrently. Combined with the dedicated `batch_db` pool this
    // keeps the backtest off the dashboard's and ingest's connections entirely. The
    // slow part is the trade fetch, so one `tick()` per resolved candidate tracks real
    // progress; chunks tick every candidate (even on fetch error) so the bar
    // always reaches `total`.
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
                    // Cache hit, or a freshly fetched history we memoize for the
                    // next run (an absent mint = no trades = no entry).
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
                // Buy on the first trade where every configured scalp gate holds; the
                // exit then runs through `exit::find_trade_driven_exit`.
                // The scalp signal is the *target* (trigger trade). The recorded
                // *entry* is the worst-case adverse fill in the trigger's block (and
                // the next) — the same resolver the live/paper poll uses — so a
                // backtest reproduces the real target↔entry slippage gap. They
                // coincide only when no adverse trade exists.
                let target = entry::find_scalp_entry(&trades, &rule)?;
                let entry_fill = entry::find_worst_case_paper_entry(&trades, &target.tx_signature);
                // No usable fill priced in the window → drop the token, mirroring
                // paper's cleanup (it deletes the un-filled position rather than
                // trade a 0-priced row).
                if entry_fill.price <= 0.0 {
                    return None;
                }
                let target_price = target.price;
                let target_token_amount = target.amount_tokens;
                let target_time = target.block_time;
                let target_tx = target.tx_signature;
                let entry_price = entry_fill.price;
                let entry_tx = entry_fill.tx_signature;
                let entry_time = entry_fill.block_time;

                // All-time high across the token's full trade history.
                let ath_price = trades
                    .iter()
                    .map(|t| t.price_per_token)
                    .fold(entry_price, f64::max);

                // Exit ladder is driven from the recorded entry fill (price + time),
                // exactly as the live/paper exit poll resolves it.
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
                    target_price: Some(target_price),
                    target_token_amount: Some(target_token_amount),
                    target_time: Some(target_time),
                    target_tx: Some(target_tx),
                    entry_price,
                    ath_price,
                    // Token count `buy_amount / entry_price` (entry_price > 0 here —
                    // a 0-priced fill was dropped above). pnl_sol math is unchanged
                    // because `buy_amount × pct == (buy_amount/entry)×exit − buy_amount`.
                    entry_token_amount: rule.buy_amount / entry_price,
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

                // Admit by the trigger (target) time — when the position arms — so
                // concurrency/total caps match the live admission order.
                Some((target_time, exit_time, result))
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
