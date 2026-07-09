use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use super::util::none_if_zero_u64;
use crate::sweep::strategy::{round_trip_with_costs, CostModel};
use crate::state::local_state::LocalState;
use crate::strategies::sim_progress::SimProgress;
use crate::strategies::token_enrich::{self, TokenEnrichment};
use crate::storage::repositories::token_repo::TokenRepo;
use trading_core::strategies::registry::tpsl2_decision_rule;

/// Per-token simulation result.
#[derive(Clone, serde::Serialize)]
pub struct BacktestTokenResult {
    pub mint_address: String,
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
    /// All-time-high price from `tokens_info` — row-owned, filled from the
    /// enrichment batch fetch (same source as Positions/Sweep), not recomputed
    /// from the corpus. `None` until that fetch runs / if the token has no info row.
    pub ath_price: Option<f64>,
    /// Tokens bought at entry (`buy_amount_sol / entry_price`); SOL derived at display.
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
    // `Tpsl2Rule` the decision layer consumes (params JSONB → gates + universal
    // columns). A rule_id that resolves to a different strategy is "not found" for
    // this tpsl2 endpoint.
    let strategy_rule = app_state
        .strategy_repo()
        .find_rule(rule_id)
        .await
        .map_err(|e| anyhow!("DB error fetching rule: {e}"))?
        .filter(|r| r.strategy_id == "tpsl_sniper_2")
        .ok_or_else(|| anyhow!("Rule not found"))?;
    let rule =
        tpsl2_decision_rule(&strategy_rule).map_err(|e| anyhow!("invalid tpsl2 rule params: {e}"))?;

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
    //
    // Token-level pre-filter: every *configured* token criterion must hold. A
    // scalp rule may set none (its gating happens on the trade stream), so this is
    // the vacuous-true variant rather than `token_matches_buy_rule`.
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
                |t| !t.is_mayhem_mode && entry::token_criteria_satisfied(t, &rule_scan),
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
    // absent from the map (absent = no trades = no entry). One `tick()` per
    // candidate keeps the bar reaching `total`.
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

    let mut candidates: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> =
        Vec::with_capacity(tokens.len());
    for token in tokens.iter() {
        // Cooperative cancel: stop resolving but keep ticking so the bar reaches
        // `total` (the partial result is discarded below on cancel anyway).
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            progress.tick();
            continue;
        }
        let resolved = histories.get(&token.mint_address).and_then(|trades| {
            // Fork A: resolve entry by **index**, the exact path the grouped sweep
            // uses (`find_scalp_entry_indexed` → `find_worst_case_paper_entry_at`),
            // so simulate and sweep pick byte-identical entries. The scalp signal is
            // the *target* (trigger trade); the recorded *entry* is the worst-case
            // adverse fill in the trigger's block (and the next) — the same resolver
            // the live/paper poll uses — reproducing the real target↔entry slippage
            // gap. They coincide only when no adverse trade exists.
            let (trigger_idx, target) = entry::find_scalp_entry_indexed(trades, &rule)?;
            let entry_fill = entry::find_worst_case_paper_entry_at(trades, trigger_idx)?;
            // No usable fill priced in the window → drop the token, mirroring paper's
            // cleanup (it deletes the un-filled position rather than trade a 0-priced row).
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

            // Exit ladder is driven from the recorded entry fill (price + time),
            // exactly as the live/paper exit poll resolves it.
            let exit = exit::find_trade_driven_exit(trades, entry_time, entry_price, &rule);

            let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
                match exit {
                    Some(fill) => {
                        let secs = (fill.block_time - entry_time).num_seconds();
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
                            Some(pct),
                            Some(sol),
                        )
                    }
                    None => (None, None, None, "Open".to_string(), None, None, None),
                };

            let result = BacktestTokenResult {
                mint_address: token.mint_address.clone(),
                symbol: token.symbol.clone(),
                target_price: Some(target_price),
                target_token_amount: Some(target_token_amount),
                target_time: Some(target_time),
                target_tx: Some(target_tx),
                entry_price,
                // Filled from the enrichment batch fetch below (tokens_info ATH).
                ath_price: None,
                // Token count `buy_amount_sol / entry_price` (entry_price > 0 here —
                // a 0-priced fill was dropped above). pnl_sol math is unchanged
                // because `buy_amount_sol × pct == (buy_amount_sol/entry)×exit − buy_amount_sol`.
                entry_token_amount: rule.buy_amount_sol / entry_price,
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

            // Admit by the trigger (target) time — when the position arms — so
            // concurrency/total caps match the live admission order.
            Some((target_time, exit_time, result))
        });
        progress.tick();
        if let Some(r) = resolved {
            candidates.push(r);
        }
    }

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
