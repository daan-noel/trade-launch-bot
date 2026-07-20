//! Generic engine simulate (plan 5.2) — the analysis counterpart of the live
//! engine loop. It resolves a rule (a saved `strategy_rules` row **or** an inline
//! params draft for a frontend dry-run) into the engine's [`LoadedRule`] +
//! [`Fingerprint`], scans the tokens that match the fingerprint, loads their lake
//! histories, and drives them all through the shared [`replay`] engine — so a rule
//! prices identically whether it ran live, was swept, or is simulated here.
//!
//! It replaces the three per-strategy simulate routes (`tpsl1`/`tpsl2`/`swing1`)
//! with one, and reuses the existing result plumbing verbatim: rows land in
//! [`SimResults`](crate::state::sim_results) keyed by a run id, and the
//! strategy-agnostic `positions::sim_result_page`/`sim_result_summary` endpoints
//! serve them. Caps (`max_concurrent`/`max_total`) are applied **in the fold** by
//! the engine (global time order), not post-hoc, so simulate honors them exactly as
//! live does.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use hunter_engine::event::LoadedRule;
use hunter_engine::fingerprint::{match_all, Fingerprint as EngineFingerprint, MatchPhase};

use trading_core::models::ingest::SseEvent;
use trading_core::models::{Fingerprint as ModelFingerprint, StrategyRule};
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::storage::repositories::token_repo::TokenRepo;
use trading_core::strategies::fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded};

use crate::state::analysis_cache::AnalysisCacheKey;
use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;
use crate::strategies::replay::{self, outcome_to_row, EngineBacktestResult, ReplayConfig, ReplayToken};
use crate::strategies::sim_progress::SimProgress;

/// Serialize a backtest row vector to JSON values for storage.
fn rows_to_json<T: serde::Serialize>(rows: Vec<T>) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    match serde_json::to_value(&rows) {
        Ok(serde_json::Value::Array(vals)) => Ok(vals),
        Ok(_) => anyhow::bail!("unexpected sim result shape (not an array)"),
        Err(e) => Err(anyhow::Error::from(e)),
    }
}

/// A generic-engine simulate request: a saved rule id **or** an inline draft, plus
/// the optional creation-time window.
#[derive(Debug, Deserialize)]
pub struct EngineSimRequest {
    /// Simulate a saved `strategy_rules` row.
    pub rule_id: Option<Uuid>,
    /// Simulate an unsaved params draft (frontend dry-run). Ignored if `rule_id` is
    /// set.
    pub draft: Option<EngineRuleDraft>,
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// An inline, unsaved rule for a dry-run simulate — the "how it trades" columns
/// plus the raw `params` (validated by [`rule_to_loaded`]).
#[derive(Debug, Deserialize)]
pub struct EngineRuleDraft {
    pub fingerprint_id: Uuid,
    pub params: Value,
    pub buy_amount_sol: f64,
    #[serde(default)]
    pub max_concurrent_tokens: i64,
    #[serde(default)]
    pub max_total_tokens: i64,
    #[serde(default = "default_trade_mode")]
    pub trade_mode: String,
}

fn default_trade_mode() -> String {
    "paper".to_string()
}

/// A resolved simulate target: the run id its results are keyed by, the engine rule
/// + its fingerprint, and the SOL notional to price round-trips at.
struct ResolvedTarget {
    run_id: Uuid,
    loaded: LoadedRule,
    fp: EngineFingerprint,
    buy_amount_sol: f64,
}

/// Start a generic engine simulation. Resolves the target, then spawns a detached
/// backtest whose terminal rows land in [`SimResults`] under the run id; the client
/// collects them via the shared `/simulate/result` endpoints once the
/// `simulation_finished` SSE fires. Returns `202 { run_id, started }`.
pub async fn spawn_engine_simulation(
    app_state: web::Data<Arc<LocalState>>,
    req: EngineSimRequest,
) -> HttpResponse {
    let since = req.since;
    let until = req.until;
    let target = match resolve_target(&app_state, &req).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let run_id = target.run_id;

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cell = Arc::new(crate::state::job_progress::ProgressCell::default());
    app_state.sim_cancels.insert(run_id, cancel.clone());
    app_state.sim_progress.insert(run_id, cell.clone());
    app_state.sim_results.clear(&run_id);

    actix_web::rt::spawn(async move {
        // Fire `simulation_finished` + drop the run's cancel/progress no matter how
        // the backtest ends — the client's SSE-then-collect flow relies on it.
        struct Guard {
            state: web::Data<Arc<LocalState>>,
            run_id: Uuid,
            cancel: Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                self.state.sim_cancels.remove(&self.run_id);
                self.state.sim_progress.remove(&self.run_id);
                let _ = self.state.sse_tx.send(SseEvent::SimulationFinished {
                    rule_id: self.run_id,
                    cancelled: self.cancel.load(Ordering::Acquire),
                });
            }
        }
        let _guard = Guard { state: app_state.clone(), run_id, cancel: cancel.clone() };

        let outcome = match run_engine_backtest(
            &app_state, &target, since, until, cancel, cell,
        )
        .await
        {
            Ok(rows) => SimOutcome::Done(Arc::new(rows)),
            Err(e) => classify_error(&e),
        };
        app_state.sim_results.insert(run_id, sim_key(&target), since, until, outcome);
    });

    HttpResponse::Accepted().json(serde_json::json!({ "started": true, "run_id": run_id }))
}

/// Resolve `req` into a concrete [`ResolvedTarget`] (loading the rule + its
/// fingerprint from PG), or an HTTP error response.
async fn resolve_target(
    state: &LocalState,
    req: &EngineSimRequest,
) -> Result<ResolvedTarget, HttpResponse> {
    let fp_repo = FingerprintRepo::new(state.db.clone());

    if let Some(rule_id) = req.rule_id {
        let rule = RuleRepo::new(state.db.clone())
            .find(rule_id)
            .await
            .map_err(|e| HttpResponse::InternalServerError().json(err(&format!("DB error: {e}"))))?
            .ok_or_else(|| HttpResponse::NotFound().json(err("Rule not found")))?;
        let fp = load_fp(&fp_repo, rule.fingerprint_id).await?;
        let loaded = rule_to_loaded(&rule)
            .map_err(|e| HttpResponse::BadRequest().json(err(&format!("invalid rule params: {e}"))))?;
        return Ok(ResolvedTarget {
            run_id: rule_id,
            buy_amount_sol: rule.buy_amount_sol(),
            fp: fp_to_engine(&fp),
            loaded,
        });
    }

    let Some(draft) = &req.draft else {
        return Err(HttpResponse::BadRequest().json(err("either rule_id or draft is required")));
    };
    let fp = load_fp(&fp_repo, draft.fingerprint_id).await?;
    // A dry-run rule gets a fresh id (its results are keyed by it); the row is never
    // persisted, only fed to the engine.
    let synthetic = StrategyRule {
        id: Uuid::new_v4(),
        rule_name: "draft".to_string(),
        fingerprint_id: draft.fingerprint_id,
        trade_mode: draft.trade_mode.clone(),
        is_active: false,
        is_enabled: true,
        buy_amount_lamports: trading_core::config::constants::sol_to_lamports(draft.buy_amount_sol),
        max_concurrent_tokens: draft.max_concurrent_tokens,
        max_total_tokens: draft.max_total_tokens,
        params: draft.params.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let loaded = rule_to_loaded(&synthetic)
        .map_err(|e| HttpResponse::BadRequest().json(err(&format!("invalid draft params: {e}"))))?;
    Ok(ResolvedTarget {
        run_id: synthetic.id,
        buy_amount_sol: draft.buy_amount_sol,
        fp: fp_to_engine(&fp),
        loaded,
    })
}

async fn load_fp(
    repo: &FingerprintRepo,
    id: Uuid,
) -> Result<ModelFingerprint, HttpResponse> {
    repo.find(id)
        .await
        .map_err(|e| HttpResponse::InternalServerError().json(err(&format!("DB error: {e}"))))?
        .ok_or_else(|| HttpResponse::NotFound().json(err("Fingerprint not found")))
}

/// The core backtest: scan → load → replay → rows. CPU-bound work (the replay fold)
/// runs on a blocking thread so the async worker stays free.
///
/// Each phase is wrapped in an [`obs::Stage`](crate::sweep::obs::Stage) timer
/// (`sim_scan` → candidate scan, `sim_load` → lake histories, `sim_replay` → the
/// single-threaded engine fold, `sim_enrich` → the fired-token DB enrichment) under a
/// `sim_backtest_total` wrapper, so the lab log shows *where the seconds go* — the
/// measurement that decides what's worth optimizing (load vs fold), the simulate
/// counterpart of the sweep's `sweep_pass` P0 gate. The scan/load phases are cache +
/// single-flight backed, so a warm re-run shifts the weight onto `sim_replay` and a
/// cold run onto `sim_load`. Log-only — this is the analysis path, not the live hot
/// path, so a handful of RSS reads per run are free.
async fn run_engine_backtest(
    app_state: &Arc<LocalState>,
    target: &ResolvedTarget,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress_cell: Arc<crate::state::job_progress::ProgressCell>,
) -> Result<Vec<Value>> {
    let _permit = app_state
        .backtest_sem
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow!("backtest concurrency semaphore closed"))?;

    // Time the whole backtest (started after the permit, so queue-wait for a busy
    // semaphore is excluded and this measures real work). Drop-based, so it reports
    // even on a `?`-error or a cancellation `bail!` — the sum of the four inner
    // stages vs this total also exposes the un-timed remainder (build set, sort, JSON).
    let _total = crate::sweep::obs::Stage::start("sim_backtest_total");

    let fp = target.fp.clone();

    // The matched candidate set — every token whose observed creation axes match the
    // fingerprint's instant axes (see [`scan_matched_candidates`]). Shared, cached,
    // and single-flighted with the matched-tokens endpoint.
    let tokens = {
        let _stage = crate::sweep::obs::Stage::start("sim_scan");
        scan_matched_candidates(app_state, &fp, since, until).await?
    };

    let progress = Arc::new(SimProgress::new(
        app_state.sse_tx.clone(),
        target.run_id,
        tokens.len(),
        progress_cell,
    ));
    progress.start();

    let with_flow = rule_needs_flow(&target.loaded);
    let histories = {
        let _stage = crate::sweep::obs::Stage::start("sim_load");
        crate::strategies::candidate_cache::get_or_fetch_histories_state(
            app_state,
            history_cache_key(&fp, since, until, with_flow),
            &tokens,
            with_flow,
        )
        .await
        .map_err(|e| anyhow!("lake trade fetch failed: {e}"))?
    };

    // Build the replay set (skip tokens with no lake history — absent = no trades =
    // no entry). The whole fold is CPU-bound but single-threaded (one shared
    // `EngineState`), so it runs on one blocking thread.
    let replay_tokens: Vec<ReplayToken> = tokens
        .iter()
        .filter_map(|t| {
            let trades = histories.get(&t.mint_address)?.clone();
            let creator_wallet_hash = (!t.creator_wallet.is_empty())
                .then(|| hunter_engine::metrics::flow_split::wallet_hash(&t.creator_wallet));
            Some(ReplayToken {
                mint: t.mint_address.clone(),
                symbol: t.symbol.clone(),
                created_at: t.created_at,
                tf: observed_axes(t, None, None),
                trades,
                creator_wallet_hash,
            })
        })
        .collect();
    // Token → (symbol, created_at) for building result rows off the outcomes.
    let meta: std::collections::HashMap<String, (String, DateTime<Utc>)> = tokens
        .iter()
        .map(|t| (t.mint_address.clone(), (t.symbol.clone(), t.created_at)))
        .collect();

    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("simulation cancelled");
    }

    let loaded = target.loaded.clone();
    let buy_amount_sol = target.buy_amount_sol;
    let progress2 = progress.clone();
    // The single-threaded engine fold (one shared `EngineState` — this is the phase
    // AVX-512 does *not* apply to; its win, if any, is precompute-per-token). Row
    // building runs inside the same blocking closure, so `sim_replay` covers it.
    let mut rows: Vec<EngineBacktestResult> = {
        let _stage = crate::sweep::obs::Stage::start("sim_replay");
        tokio::task::spawn_blocking(move || {
            let outcomes = replay::run_replay(
                std::slice::from_ref(&loaded),
                std::slice::from_ref(&fp),
                replay_tokens,
                ReplayConfig { as_of: Utc::now() },
            );
            outcomes
                .iter()
                .filter_map(|o| {
                    let (symbol, created_at) = meta.get(&o.mint)?;
                    progress2.tick();
                    Some(outcome_to_row(o, symbol, *created_at, buy_amount_sol))
                })
                .collect()
        })
        .await
        .map_err(|e| anyhow!("simulate replay task panicked: {e}"))?
    };

    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("simulation cancelled");
    }

    // Enrich exactly the fired tokens (bounded batch), attaching token metadata +
    // row-owned ATH — mirrors the tpsl backtest.
    let result_mints: Vec<String> = rows.iter().map(|r| r.mint_address.clone()).collect();
    let mut enrichment = {
        let _stage = crate::sweep::obs::Stage::start("sim_enrich");
        crate::strategies::token_enrich::fetch_enrichment(&app_state.batch_db, &result_mints)
            .await
            .map_err(|e| anyhow!("token enrichment fetch failed: {e}"))?
    };
    for r in &mut rows {
        if let Some(e) = enrichment.remove(&r.mint_address) {
            r.ath_price = e.ath_price;
            r.token = (&e).into();
        }
    }

    // Same display order as the tpsl backtest: TakeProfit first, other closed exits
    // next, still-Open last; ties by pnl% desc.
    rows.sort_by(|a, b| {
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

    rows_to_json(rows)
}

/// The analysis-cache key for a fingerprint's candidate scan over `[since, until)`.
/// The backtest and the matched-tokens endpoint build it the same way so both hit
/// the one cached scan for a given `(fingerprint, window)`.
fn candidate_cache_key(
    fp: &EngineFingerprint,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> AnalysisCacheKey {
    AnalysisCacheKey::new("engine", fp.id.0.to_string(), since, until)
}

/// History-cache key — distinct from the candidate key when `with_flow` so a
/// slim (no ix_labels/wallet) hit never poisons a flow simulate.
fn history_cache_key(
    fp: &EngineFingerprint,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    with_flow: bool,
) -> AnalysisCacheKey {
    let strategy = if with_flow { "engine+flow" } else { "engine" };
    AnalysisCacheKey::new(strategy, fp.id.0.to_string(), since, until)
}

/// True when the rule's params reference a flow metric group (needs lake flow cols).
fn rule_needs_flow(loaded: &LoadedRule) -> bool {
    use hunter_engine::metrics::MetricGroupId;
    for side in [loaded.params.entry.as_ref(), loaded.params.exit.as_ref()]
        .into_iter()
        .flatten()
    {
        if side.0.contains_key(&MetricGroupId::FlowSplit)
            || side.0.contains_key(&MetricGroupId::FlowWindow)
        {
            return true;
        }
    }
    false
}

/// Scan (or reuse) the fingerprint's **matched** candidate set: every token whose
/// observed creation axes satisfy the fingerprint's *instant* axes. This is the
/// superset the replay fold then arms/enters from — the two-phase matcher's
/// first-slot axes resolve inside the fold, so a token that matches here but fails
/// first-slot simply never arms. Single-flighted + TTL-cached, so the matched-tokens
/// endpoint and the backtest share one whole-table scan.
pub(crate) async fn scan_matched_candidates(
    app_state: &Arc<LocalState>,
    fp: &EngineFingerprint,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> Result<Arc<Vec<crate::models::Token>>> {
    let batch_db = app_state.batch_db.clone();
    let fp_scan = fp.clone();
    crate::strategies::candidate_cache::get_or_scan_candidates_state(
        app_state,
        candidate_cache_key(fp, since, until),
        Box::pin(async move {
            let repo = TokenRepo::new(batch_db);
            crate::strategies::analysis::collect_matching_tokens(&repo, since, until, |t| {
                !t.is_mayhem_mode
                    && !match_all(
                        std::slice::from_ref(&fp_scan),
                        &observed_axes(t, None, None),
                        MatchPhase::Instant,
                    )
                    .is_empty()
            })
            .await
            .map_err(|e| anyhow!("candidate token scan failed: {e}"))
        }),
    )
    .await
}

/// Resolve a saved rule's fingerprint (engine form) for the matched-tokens scan.
/// Matched depends only on the fingerprint, so this deliberately skips exit-param
/// validation — a rule with a valid fingerprint still has a well-defined matched set
/// even if its params wouldn't parse into a [`LoadedRule`].
pub(crate) async fn load_rule_fingerprint(
    state: &LocalState,
    rule_id: Uuid,
) -> Result<EngineFingerprint, HttpResponse> {
    let rule = RuleRepo::new(state.db.clone())
        .find(rule_id)
        .await
        .map_err(|e| HttpResponse::InternalServerError().json(err(&format!("DB error: {e}"))))?
        .ok_or_else(|| HttpResponse::NotFound().json(err("Rule not found")))?;
    let fp = load_fp(&FingerprintRepo::new(state.db.clone()), rule.fingerprint_id).await?;
    Ok(fp_to_engine(&fp))
}

/// A stable cache key for a resolved target — its config fingerprint, so a re-run
/// over the same window can short-circuit to the cached rows.
fn sim_key(t: &ResolvedTarget) -> String {
    format!(
        "engine:{}:{}:{}:{}:{}",
        t.fp.id.0,
        t.loaded.buy_amount_lamports,
        t.loaded.max_concurrent_tokens,
        t.loaded.max_total_tokens,
        t.loaded.params.to_value(),
    )
}

fn classify_error(e: &anyhow::Error) -> SimOutcome {
    let msg = e.to_string();
    if msg.contains("cancelled") {
        SimOutcome::Cancelled
    } else {
        tracing::error!("engine simulation failed: {e}");
        SimOutcome::Failed { status: 500, message: msg }
    }
}

fn err(message: &str) -> serde_json::Value {
    serde_json::json!({ "error": message })
}
