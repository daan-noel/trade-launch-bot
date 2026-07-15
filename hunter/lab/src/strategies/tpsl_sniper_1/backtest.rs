use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use super::util::none_if_zero_u64;
use crate::models::{Token, Tpsl1Rule};
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{quantize_f32, round_trip_with_costs, CostModel};
use crate::state::local_state::LocalState;
use crate::strategies::admission::select_simulated_tokens;
use crate::strategies::sim_progress::SimProgress;
use crate::strategies::token_enrich::{self, TokenEnrichment};
use crate::storage::repositories::token_repo::TokenRepo;
use trading_core::strategies::registry::tpsl1_decision_rule;

/// Per-token simulation result.
#[derive(Clone, serde::Serialize)]
pub struct BacktestTokenResult {
    pub mint_address: String,
    pub symbol: String,
    pub created_at: DateTime<Utc>,
    pub entry_price: f64,
    /// All-time-high price from `tokens_info` — row-owned, filled from the
    /// enrichment batch fetch (same source as Positions/Sweep), not recomputed
    /// from the corpus. `None` until that fetch runs / if the token has no info row.
    pub ath_price: Option<f64>,
    pub entry_token_amount: f64,
    pub entry_tx: String,
    pub entry_time: DateTime<Utc>,
    pub exit_price: Option<f64>,
    pub exit_tx: Option<String>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Seconds from entry to exit (None if still open).
    pub holding_secs: Option<i64>,
    pub pnl_percent: Option<f64>,
    /// PnL in SOL based on the rule's buy_amount_sol.
    pub pnl_sol: Option<f64>,
    /// "LiquidityExit", "TakeProfit", "StopLoss", "TrailingStop", "Stall",
    /// "TimeStop", or "Open"
    pub exit_reason: String,
    /// Token metadata (initial buy, CU price, migrated/dead, market cap, ...),
    /// filled in once after selection via a single bounded batch fetch — see
    /// [`token_enrich`]. Default (empty) until that fetch runs.
    #[serde(flatten)]
    pub token: TokenEnrichment,
}

/// Resolve one candidate token's simulated entry → exit into a
/// [`BacktestTokenResult`], returning `None` when the token has no lake history
/// or no usable fill. Pure and `Send`-safe (reads only the shared in-memory
/// `histories` map + the rule), so the candidate set resolves across cores under
/// `rayon` in [`run_backtest`].
///
/// `find_entry_fill_in_trades` / `find_trade_driven_exit` are generic over
/// `TradeRow`, so they consume the lake's `CorpusTrade` rows unchanged; the
/// `entry_tx`/`exit_tx` come straight from `CorpusTrade::tx_signature()`.
fn resolve_token(
    token: &Token,
    histories: &HashMap<String, Arc<Vec<CorpusTrade>>>,
    rule: &Tpsl1Rule,
) -> Option<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> {
    let trades = histories.get(&token.mint_address)?;
    let entry = entry::find_entry_fill_in_trades(trades, 1)?;
    let (entry_price, entry_tx, entry_time) = (entry.price, entry.tx_signature, entry.block_time);

    let exit = exit::find_trade_driven_exit(trades, entry_time, entry_price, rule);

    let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
        match exit {
            Some(fill) => {
                let secs = (fill.block_time - entry_time).num_seconds();
                // Costed round-trip (fees + slippage + fixed Jito/priority cost),
                // the same `CostModel` every sweep prices its combos with — a
                // frictionless price-to-price % understated the live cost of
                // trading tpsl1 (parity plan A1).
                let (sol, pct) = round_trip_with_costs(
                    entry_price,
                    fill.price,
                    rule.buy_amount_sol,
                    &CostModel::pumpfun_default(),
                );
                (
                    Some(fill.price),
                    Some(fill.tx_signature),
                    Some(fill.block_time),
                    fill.reason.to_string(),
                    Some(secs),
                    Some(quantize_f32(pct)),
                    Some(quantize_f32(sol)),
                )
            }
            None => {
                // Still open at end of history — mark unrealized PnL at the last
                // price, so the simulate drill-in shows the SAME number the grouped
                // sweep does for the same open token (the sweep's `resolve_exit`
                // open arm marks-to-last identically; see `sweep::strategies::tpsl1`).
                // Exit-fill fields stay `None` (no sell fired) and it's still an
                // "Open" outcome — only the PnL is marked. Priced through the same
                // `CostModel` and quantized through `f32` for byte-identical parity.
                let last_price =
                    trades.last().map(|t| t.price_per_token).unwrap_or(entry_price);
                let (sol, pct) = round_trip_with_costs(
                    entry_price,
                    last_price,
                    rule.buy_amount_sol,
                    &CostModel::pumpfun_default(),
                );
                (
                    None,
                    None,
                    None,
                    "Open".to_string(),
                    None,
                    Some(quantize_f32(pct)),
                    Some(quantize_f32(sol)),
                )
            }
        };

    let result = BacktestTokenResult {
        mint_address: token.mint_address.clone(),
        symbol: token.symbol.clone(),
        created_at: token.created_at,
        entry_price,
        // Filled from the enrichment batch fetch below (tokens_info ATH).
        ath_price: None,
        entry_token_amount: rule.buy_amount_sol,
        entry_tx,
        entry_time,
        exit_price,
        exit_tx,
        exit_time,
        holding_secs,
        pnl_percent,
        pnl_sol,
        exit_reason,
        token: TokenEnrichment::default(),
    };
    Some((entry_time, exit_time, result))
}

/// Simulate a TPSL rule over historical trade data read from the Parquet lake
/// (the same corpus the grouped sweep uses); returns token-level results. Shares
/// the exit ladder with the live path via [`exit::find_trade_driven_exit`], so a
/// backtest and a live run resolve identical exits.
pub async fn run_backtest(
    app_state: actix_web::web::Data<Arc<LocalState>>,
    rule_id: Uuid,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress_cell: Arc<crate::state::job_progress::ProgressCell>,
) -> Result<Vec<BacktestTokenResult>> {
    // Bound concurrent backtests so overlapping runs can't drain the `batch` pool
    // (the `candidate token scan failed: pool timed out` contention). Held for the
    // whole run; excess simulations queue here rather than pile onto the pool. The
    // spawned task already owns its cancel/progress, so queueing only delays start.
    let _permit = app_state
        .backtest_sem
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow!("backtest concurrency semaphore closed"))?;

    // Load the rule from the unified `strategy_rules` table and rebuild the
    // `Tpsl1Rule` the decision layer consumes (params JSONB → gates + universal
    // columns). A rule_id that resolves to a different strategy is "not found" for
    // this tpsl1 endpoint.
    let strategy_rule = app_state
        .strategy_repo()
        .find_rule(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .filter(|r| r.strategy_id == "tpsl_sniper_1")
        .ok_or_else(|| anyhow!("Rule not found"))?;
    let rule =
        tpsl1_decision_rule(&strategy_rule).map_err(|e| anyhow!("invalid tpsl1 rule params: {e}"))?;

    let max_concurrent_tokens = none_if_zero_u64(rule.p_max_concurrent_tokens).map(|v| v as usize);
    let max_total_tokens = none_if_zero_u64(rule.p_max_total_tokens).map(|v| v as usize);

    let cache_key = crate::state::analysis_cache::AnalysisCacheKey::new(
        &strategy_rule.strategy_id,
        trading_core::strategies::match_keys::fingerprint_key(&strategy_rule),
        since,
        until,
    );

    // Candidate tokens are scanned from the **whole** `tokens` table (keyset-
    // streamed within the optional `[since, until)` window), decoupled from the
    // live `token_cache` so an old, evicted-but-matching mint is still simulated.
    // Only the sparse matches stay resident — never the table.
    //
    // Never trade Mayhem-Mode tokens: they carry an AI random-walk agent (2B supply,
    // net-sell drift, ±300% noise) for their first 24h — manufactured chaos, not a
    // snipeable edge. Exclude them outright (legacy-only policy, 2026-06 regime).
    // The whole-table token scan runs on the dedicated batch pool so a backtest
    // can't starve dashboard reads; trade histories then come from the lake (no PG).
    let rule_scan = rule.clone();
    let batch_db = app_state.batch_db.clone();
    let tokens = crate::strategies::candidate_cache::get_or_scan_candidates_state(
        &app_state,
        cache_key.clone(),
        Box::pin(async move {
            let repo = TokenRepo::new(batch_db);
            crate::strategies::analysis::collect_matching_tokens(
                &repo,
                since,
                until,
                |t| {
                    !t.is_mayhem_mode
                        && entry::token_matches_buy_rule(t, &rule_scan)
                },
            )
            .await
            .map_err(|e| anyhow!("candidate token scan failed: {e}"))
        }),
    )
    .await?;

    // One shared lake read for every candidate — the **same** Parquet corpus the
    // grouped sweep loads (`fetch_sim_histories`), so a rule prices identically
    // whether swept or drilled into. Replaces the old per-chunk PG
    // `find_by_mints_all` + `backtest_trade_cache`; a mint with no lake rows is
    // absent from the map (absent = no trades = no entry, same as the old empty
    // default). One `tick()` per candidate keeps the bar reaching `total`.
    let progress = Arc::new(SimProgress::new(
        app_state.sse_tx.clone(),
        rule_id,
        tokens.len(),
        progress_cell,
    ));
    progress.start();

    let histories = crate::strategies::candidate_cache::get_or_fetch_histories_state(
        &app_state,
        cache_key,
        &tokens,
    )
    .await
    .map_err(|e| anyhow!("lake trade fetch failed: {e}"))?;

    // Resolve every candidate's entry→exit **in parallel, off the async worker**
    // (see tpsl_sniper_2::backtest for the rationale): pure CPU over the in-memory
    // `histories` map, spread across cores by `rayon` inside `spawn_blocking`. One
    // `tick()` per token keeps the bar reaching `total`; order is irrelevant (sorted
    // by entry time below).
    let mut candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> = {
        let tokens = tokens.clone();
        let rule = rule.clone();
        let progress = progress.clone();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            tokens
                .par_iter()
                .filter_map(|token| {
                    let resolved = if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        None
                    } else {
                        resolve_token(token, &histories, &rule)
                    };
                    progress.tick();
                    resolved
                })
                .collect()
        })
        .await
        .map_err(|e| anyhow!("simulate resolve task panicked: {e}"))?
    };

    // If a cancel landed mid-run, discard the partial result so the caller can
    // report a clean cancellation instead of an incomplete simulation.
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        anyhow::bail!("simulation cancelled");
    }

    candidates.sort_by_key(|(entry_time, _, _)| *entry_time);

    let mut results = select_simulated_tokens(candidates, max_concurrent_tokens, max_total_tokens);

    // One bounded batch fetch — enrich exactly the tokens that made the final
    // result set with token metadata (initial buy, CU price, market cap,
    // migrated/dead, ...), so the Simulated-tokens table can sort/filter/search
    // on it server-side (previously only merged client-side, per visible page).
    let result_mints: Vec<String> = results.iter().map(|r| r.mint_address.clone()).collect();
    let mut enrichment = token_enrich::fetch_enrichment(&app_state.batch_db, &result_mints)
        .await
        .map_err(|e| anyhow!("token enrichment fetch failed: {e}"))?;
    for r in &mut results {
        if let Some(e) = enrichment.remove(&r.mint_address) {
            // `ath_price` is row-owned (excluded from `TokenEnrichment`); set it
            // off the row, then flatten the rest — mirrors Positions/Sweep.
            r.ath_price = e.ath_price;
            r.token = (&e).into();
        }
    }

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
