use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::entry;
use super::exit;
use crate::models::{Swing1Rule, Token};
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{quantize_f32, round_trip_with_costs, CostModel};
use crate::state::local_state::LocalState;
use crate::strategies::admission::select_simulated_tokens;
use crate::strategies::sim_progress::SimProgress;
use crate::strategies::token_enrich::{self, TokenEnrichment};
use crate::storage::repositories::token_repo::TokenRepo;
// swing1 borrows tpsl1's `none_if_zero_u64` (there is no swing1 `util` module).
use serde::Serialize;
use trading_core::strategies::registry::swing1_decision_rule;
use trading_core::strategies::swing_1::swing::{detect_swing_legs_raw, SwingLeg};
use trading_core::strategies::swing_1::swing_params_from_rule;
use trading_core::strategies::tpsl_sniper_1::util::none_if_zero_u64;

// Reuse tpsl1's per-token result shape (swing1 has no trigger snapshot, exactly like
// tpsl1) as the flattened base so the shared frontend card/table renders both
// identically — kept in lockstep by embedding, not duplicating, the struct.
pub use crate::strategies::tpsl_sniper_1::backtest::BacktestTokenResult as BacktestBase;

/// swing1's per-token result: tpsl1's base shape plus the leg ledger the sim resolved
/// this token's entry/exit against. `#[serde(flatten)]` inlines every base field on the
/// wire (unchanged contract), adding only `swing_legs` — so the inspect chart draws
/// exactly the legs the sim used with no separate `swing1-detect` round-trip. `None` for
/// the live-position adapter (its legs come from the exit memo, not a sim).
#[derive(Clone, Serialize)]
pub struct BacktestTokenResult {
    #[serde(flatten)]
    pub base: BacktestBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swing_legs: Option<Vec<SwingLeg>>,
}

/// Resolve one candidate token's simulated entry → exit into a
/// [`BacktestTokenResult`], returning `None` when the token has no lake history
/// or no entry latch. Pure and `Send`-safe (reads only the shared in-memory
/// `histories` map + the rule), so the candidate set resolves across cores under
/// `rayon` in [`run_backtest`].
fn resolve_token(
    token: &Token,
    histories: &HashMap<String, Arc<Vec<CorpusTrade>>>,
    rule: &Swing1Rule,
) -> Option<(DateTime<Utc>, Option<DateTime<Utc>>, BacktestTokenResult)> {
    let trades = histories.get(&token.mint_address)?;
    // swing1 entry: kill→volume latch + higher-low confirm → worst-case
    // canonical-spot fill at the trigger index. Already index-based (no Fork
    // A); byte-identical to the live paper path (both over the trade slice).
    let (_trigger_idx, fill) = entry::find_phase_entry(trades, rule)?;
    let (entry_price, entry_tx, entry_time) = (fill.price, fill.tx_signature, fill.block_time);

    let exit = exit::find_trade_driven_exit(trades, entry_time, entry_price, rule);

    let (exit_price, exit_tx, exit_time, exit_reason, holding_secs, pnl_percent, pnl_sol) =
        match exit {
            Some(fill) => {
                let secs = (fill.block_time - entry_time).num_seconds();
                // Costed round-trip (fees + slippage + fixed Jito/priority cost),
                // the same `CostModel` every sweep prices its combos with — a
                // frictionless price-to-price % understated the live cost of
                // trading swing1 (parity plan A1).
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
                    fill.reason.as_str().to_string(),
                    Some(secs),
                    Some(quantize_f32(pct)),
                    Some(quantize_f32(sol)),
                )
            }
            None => (None, None, None, "Open".to_string(), None, None, None),
        };

    let base = BacktestBase {
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

    // Carry the exact legs the sim priced entry/exit against so the inspect
    // chart draws them without a separate `swing1-detect` round-trip. Same
    // `detect_swing_legs_raw` + params the entry walk used, over the same
    // corpus → identical legs by construction. Only for entry-resolved tokens
    // (this fn returns early otherwise), so the ledger isn't computed for
    // the whole candidate scan.
    let legs = detect_swing_legs_raw(trades, &swing_params_from_rule(rule));
    let result = BacktestTokenResult { base, swing_legs: Some(legs) };

    Some((entry_time, exit_time, result))
}

/// Simulate a swing1 rule over historical trade data read from the Parquet lake
/// (the same corpus the grouped sweep uses); returns token-level results. Shares
/// the entry latch + exit ladder with the live path via
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

    let cache_key = crate::state::analysis_cache::AnalysisCacheKey::new(
        &strategy_rule.strategy_id,
        trading_core::strategies::match_keys::fingerprint_key(&strategy_rule),
        since,
        until,
    );

    // Candidate tokens are scanned from the whole `tokens` table (keyset-streamed
    // within the optional `[since, until)` window). swing1's token-creation gate is
    // the **optional** dev fingerprint (`token_matches_buy_rule`, vacuous-true when
    // no fingerprint is set), so this admits every non-Mayhem token that passes any
    // configured fingerprint; the entry latch then decides from the trade stream.
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
                |t| !t.is_mayhem_mode && entry::token_matches_buy_rule(t, &rule_scan),
            )
            .await
            .map_err(|e| anyhow!("candidate token scan failed: {e}"))
        }),
    )
    .await?;

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
    let result_mints: Vec<String> = results.iter().map(|r| r.base.mint_address.clone()).collect();
    let mut enrichment = token_enrich::fetch_enrichment(&app_state.batch_db, &result_mints)
        .await
        .map_err(|e| anyhow!("token enrichment fetch failed: {e}"))?;
    for r in &mut results {
        if let Some(e) = enrichment.remove(&r.base.mint_address) {
            // `ath_price` is row-owned (excluded from `TokenEnrichment`); set it
            // off the row, then flatten the rest — mirrors Positions/Sweep.
            r.base.ath_price = e.ath_price;
            r.base.token = (&e).into();
        }
    }

    results.sort_by(|a, b| {
        // TakeProfit first, then any other closed exit (incl. NextKill / StopLoss /
        // TrailingStop / LiquidityExit / Stall / TimeStop), then still-Open last.
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "Open" => 2,
            _ => 1,
        };
        rank(&a.base.exit_reason)
            .cmp(&rank(&b.base.exit_reason))
            .then_with(|| {
                b.base
                    .pnl_percent
                    .unwrap_or(0.0)
                    .partial_cmp(&a.base.pnl_percent.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    Ok(results)
}
