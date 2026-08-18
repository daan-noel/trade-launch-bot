//! Generic engine simulate (plan 5.2) — the analysis counterpart of the live
//! engine loop. It resolves a rule (a saved `strategy_rules` row **or** an inline
//! params draft for a frontend dry-run) into the engine's [`LoadedRule`] +
//! [`Fingerprint`], scans the tokens that match the fingerprint, loads their lake
//! histories, and drives them all through the shared [`replay`] engine — so a rule
//! prices identically whether it ran live, was swept, or is simulated here.
//!
//! It replaces the three per-strategy simulate routes (`tpsl1`/`tpsl2`/`swing1`)
//! with one. Saved-rule results land in disk-backed
//! [`SimResults`](crate::state::sim_results) (keyed by rule id); drafts stay
//! RAM-only. The strategy-agnostic `positions::sim_result_page` /
//! `sim_result_summary` endpoints serve them. Caps (`max_concurrent`/`max_total`)
//! are applied **in the fold** by the engine (global time order), not post-hoc,
//! so simulate honors them exactly as live does.

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
use trading_core::strategies::kernel::CostModelKind;
use trading_core::strategies::paper_fill::FillModel;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::token_repo::TokenRepo;
use trading_core::services::veteran_roster;
use trading_core::strategies::fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded};

use crate::state::analysis_cache::AnalysisCacheKey;
use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;
use crate::strategies::replay::{
    self, no_entry_row, outcome_to_row, EngineBacktestResult, ReplayConfig, ReplayToken,
};
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
    /// Which fill model prices the round-trips. Defaults to
    /// [`FillModel::WorstCase`] — the pessimistic bound live paper + the sweep book.
    /// The other models (`first_in_window` / `signal_price`, aliased `first` /
    /// `signal`) exist only for the sim's fill-sensitivity analysis: they reprice the
    /// *same* taken set to separate fill pessimism from a genuine absence of edge (the
    /// flow-scalper honest bottom line was measured under `first`). Applies to both a
    /// saved-rule and a draft run.
    #[serde(default)]
    pub fill_model: FillModel,
    /// Which execution-cost model prices the round-trips. Defaults to
    /// [`CostModelKind::PumpfunDefault`] (fee + slippage), which pairs correctly
    /// with the default `fill_model` (`WorstCase`) but **double-counts** slippage
    /// against an explicit `first_in_window`/`signal_price` fill — those already
    /// price it into the fill itself. Pass `pumpfun_fee_only` alongside a non-default
    /// fill model to avoid that (mirrors the grouped-sweep's `cost_model`).
    #[serde(default)]
    pub cost_model: CostModelKind,
    /// When set (non-empty), restrict the backtest to these mint addresses only.
    /// The fingerprint filter still applies first; this narrows further (e.g. a
    /// top-trade subset used during grouped sweep).
    #[serde(default)]
    pub mints: Option<Vec<String>>,
    /// Copycat guard (`strategy.skip_duplicate_identity`) for this run.
    ///
    /// **Absent ⇒ inherit the box's `app_settings` value**, so simulate measures the
    /// rule under the same policy live is trading it under — flip the toggle in
    /// Settings and the next backtest reflects it, no second knob to remember.
    /// Present ⇒ override for this run only (A/B the guard without touching live).
    ///
    /// Whichever way it resolves is stamped on the run's meta
    /// (`SimMeta::dupe_guard_window_hours`), because the toggle changes the PnL: two
    /// results of the same rule are only comparable if you can see which policy each
    /// was booked under. That is the lesson of the cost-model constants, which were
    /// *not* persisted per run and silently made every pre-2026-07-28 result
    /// incomparable.
    #[serde(default)]
    pub skip_duplicate_identity: Option<bool>,
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
    /// Whether the rule reads `m_bundle`. Decides whether the backtest pays for a
    /// walk-forward veteran roster (see [`run_engine_backtest`]) - every other
    /// metric is folded from the corpus itself and needs no such rebuild.
    reads_bundle: bool,
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
    let fill_model = req.fill_model;
    let cost_model = req.cost_model;
    let mints = req.mints.clone();
    // Resolve the copycat guard ONCE, here, into `Some(window_hours)` = on. The
    // request wins if it said anything; otherwise the box's own `app_settings`
    // decides — the same row the co-hosted live bin reads (one `DATABASE_URL` per
    // host in the compose), so "the toggle" means one thing on a given machine.
    let dupe_guard_window_hours =
        resolve_dupe_guard(req.skip_duplicate_identity, &app_state.core.settings());
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
    // Saved rules persist the last Done result on disk; drafts stay RAM-only.
    let persist = req.rule_id.is_some();
    let fingerprint_id = target.fp.id.0;

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
            &app_state,
            &target,
            since,
            until,
            fill_model,
            cost_model,
            mints,
            dupe_guard_window_hours,
            cancel,
            cell,
        )
        .await
        {
            Ok(rows) => SimOutcome::Done(Arc::new(rows)),
            Err(e) => classify_error(&e),
        };
        app_state.sim_results.insert(
            run_id,
            sim_cache_key(&target),
            fingerprint_id,
            since,
            until,
            fill_model,
            cost_model,
            dupe_guard_window_hours,
            outcome,
            persist,
        );
    });

    HttpResponse::Accepted().json(serde_json::json!({ "started": true, "run_id": run_id }))
}

/// Resolve the copycat guard for one simulate run into `Some(window_hours)` (on) or
/// `None` (off).
///
/// `override_on` is the request's `skip_duplicate_identity`: `None` ⇒ inherit the
/// box's `app_settings`, so a backtest measures the rule under the same policy live
/// is trading it under. A stored window of `0` falls back to the default rather than
/// producing a guard that reads as on while remembering nothing — the same defensive
/// read `DupeGuard::set_policy` does, because a `0` can reach here from an older
/// binary's row even though the settings API now rejects it.
fn resolve_dupe_guard(override_on: Option<bool>, settings: &AppSettings) -> Option<u64> {
    let on = override_on.unwrap_or(settings.skip_duplicate_identity);
    on.then(|| match settings.duplicate_identity_window_hours {
        0 => hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS,
        h => h,
    })
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
        let loaded = backtest_armed(rule_to_loaded(&rule).map_err(|e| {
            HttpResponse::BadRequest().json(err(&format!("invalid rule params: {e}")))
        })?);
        let reads_bundle = hunter_engine::metrics::bundle::params_reference_bundle(&rule.params);
        return Ok(ResolvedTarget {
            reads_bundle,
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
        // Tags are a Rules-board label; a synthetic dry-run row is never listed.
        tags: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let loaded = backtest_armed(rule_to_loaded(&synthetic).map_err(|e| {
        HttpResponse::BadRequest().json(err(&format!("invalid draft params: {e}")))
    })?);
    Ok(ResolvedTarget {
        reads_bundle: hunter_engine::metrics::bundle::params_reference_bundle(&synthetic.params),
        run_id: synthetic.id,
        buy_amount_sol: draft.buy_amount_sol,
        fp: fp_to_engine(&fp),
        loaded,
    })
}

/// Force the engine's arming switch on for a backtest.
///
/// `rule_to_loaded` maps the row's `is_active` onto `LoadedRule::entry_enabled`,
/// which `hunter_engine::reduce` reads as "may this rule arm a token at all". That
/// is a *live lifecycle* flag (start/pause), and a simulate is an offline replay —
/// honoring it made every Idle rule report `n_matched > 0, n_tokens_entered = 0`,
/// which reads like an entry-condition miss rather than a switched-off rule. It
/// also disagreed with the grouped-sweep, which has always compiled its candidates
/// with `entry_enabled: true` — so the same rule scored in the sweep and came back
/// empty in the simulate that is supposed to be the PnL authority for it.
fn backtest_armed(mut loaded: LoadedRule) -> LoadedRule {
    loaded.entry_enabled = true;
    loaded
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
    fill_model: FillModel,
    cost_model: CostModelKind,
    mints: Option<Vec<String>>,
    // Copycat guard, already resolved by the caller: `Some(window_hours)` = on.
    dupe_guard_window_hours: Option<u64>,
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

    let mut fp = target.fp.clone();

    // The matched candidate set — every token whose observed creation axes match the
    // fingerprint's instant axes (see [`scan_matched_candidates`]). Shared, cached,
    // and single-flighted with the matched-tokens endpoint.
    let tokens = {
        let _stage = crate::sweep::obs::Stage::start("sim_scan");
        scan_matched_candidates(app_state, &fp, since, until).await?
    };
    let tokens = if let Some(allow) = mints.filter(|m| !m.is_empty()) {
        let set: std::collections::HashSet<&str> = allow.iter().map(|s| s.as_str()).collect();
        Arc::new(
            tokens
                .iter()
                .filter(|t| set.contains(t.mint_address.as_str()))
                .cloned()
                .collect(),
        )
    } else {
        tokens
    };

    // `m_bundle` is the one metric whose input does not come from the corpus: the
    // veteran roster is a stored snapshot, and the stored one was built from launches
    // that lie in the FUTURE of nearly every token replayed here. Reading it would
    // score a three-week-old token against today's answer. So the driver rebuilds the
    // roster once per day of the run window and hands the engine a timeline instead -
    // each token then sees only launches older than itself.
    if target.reads_bundle {
        let _stage = crate::sweep::obs::Stage::start("sim_roster");
        let span = tokens
            .iter()
            .map(|t| t.created_at)
            .min()
            .zip(tokens.iter().map(|t| t.created_at).max());
        if let Some((from, to)) = span {
            let min_launches = veteran_roster::configured_min_launches(&fp.metric_config);
            let timeline = veteran_roster::walk_forward_timeline(
                &app_state.db,
                &TokenRepo::new(app_state.db.clone()),
                &fp,
                min_launches,
                from,
                to,
                veteran_roster::DEFAULT_LOOKBACK_DAYS,
            )
            .await
            .map_err(|e| anyhow!("veteran roster rebuild failed: {e}"))?;
            veteran_roster::install_timeline(&mut fp.metric_config, min_launches, timeline);
        }
    }

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
                // Straight off the PG `tokens` row through the ONE engine hasher —
                // the same value the live producer stamps on `TokenCreated`.
                identity: hunter_engine::token_identity_hash(&t.name, &t.symbol),
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
                // Defaults to worst-case (what live paper books); the request can select
                // `first`/`signal` for the fill-sensitivity reprice (honest bottom line).
                ReplayConfig {
                    as_of: Utc::now(),
                    fill_model,
                    skip_duplicate_identity: dupe_guard_window_hours.is_some(),
                    duplicate_identity_window_hours: dupe_guard_window_hours
                        .unwrap_or(hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS),
                    ..Default::default()
                },
            );
            outcomes
                .iter()
                .filter_map(|o| {
                    let (symbol, created_at) = meta.get(&o.mint)?;
                    progress2.tick();
                    Some(outcome_to_row(o, symbol, *created_at, buy_amount_sol, cost_model))
                })
                .collect()
        })
        .await
        .map_err(|e| anyhow!("simulate replay task panicked: {e}"))?
    };

    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("simulation cancelled");
    }

    // Pad matched candidates that never entered — no lake history, never armed,
    // entry give-up, or empty fill window. Same full-slice contract as the sweep
    // combo drill-in (`simulate_one_combo` always returns one row per token).
    {
        let entered: std::collections::HashSet<String> =
            rows.iter().map(|r| r.mint_address.clone()).collect();
        for t in tokens.iter() {
            if !entered.contains(&t.mint_address) {
                rows.push(no_entry_row(&t.mint_address, &t.symbol, t.created_at));
            }
        }
    }

    // Enrich every result row (fired + NoEntry), attaching token metadata +
    // row-owned ATH — mirrors the tpsl / sweep drill-in.
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

    // Display order: TakeProfit first, other closed exits, still-Open, NoEntry
    // last; ties by pnl% desc (NoEntry has null pnl → sorts as 0).
    rows.sort_by(|a, b| {
        let rank = |r: &str| match r {
            "TakeProfit" => 0,
            "Open" => 2,
            "NoEntry" => 3,
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

/// True when the rule reads any metric that needs the lake's wallet / label columns.
///
/// Asks each metric itself (`MetricId::needs_wallet_identity`) rather than listing
/// groups here: a group list silently excludes a wallet-keyed metric that lives in an
/// otherwise SOL-only group, and the run then folds every trade as one anonymous
/// wallet — which reads as a gate that never fires, not as a load error.
///
/// **Scale-out stages count.** A stage reuses the full exit grammar, so a ladder can
/// be the only thing in the rule referencing a flow metric.
fn rule_needs_flow(loaded: &LoadedRule) -> bool {
    let stages = loaded.params.scale_out.iter().flatten().map(|s| &s.conditions);
    [loaded.params.entry.as_ref(), loaded.params.exit.as_ref()]
        .into_iter()
        .flatten()
        .chain(stages)
        .flat_map(|side| side.0.values())
        .flat_map(|instances| instances.iter())
        .flat_map(|g| g.metrics.keys())
        .any(|m| m.needs_wallet_identity())
}

/// Scan (or reuse) the fingerprint's **matched** candidate set: every token whose
/// observed creation axes satisfy every configured axis, first-slot included —
/// [`MatchPhase::Full`] against the token's already-settled `first_slot_buy_sol`/
/// `first_slot_sell_sol`. Analysis-only tokens are historical, so the creation slot
/// has always settled by the time this scan runs (unlike the live `TokenCreated`
/// arm, which must defer first-slot axes to a later `FirstSlotSettled`). Matching
/// Full here — not just the live arm's *instant* phase — keeps this the exact set
/// the replay fold goes on to arm/enter from: a fingerprint with a first-slot axis
/// (e.g. a promoted rule scoped to one creation-buy-size bucket) would otherwise
/// list tokens outside that bucket as "matched" even though they can never actually
/// arm. Single-flighted + TTL-cached, so the matched-tokens endpoint and the
/// backtest share one whole-table scan.
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
                        &observed_axes(t, t.first_slot_buy_sol, t.first_slot_sell_sol),
                        MatchPhase::Full,
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
/// over the same window can short-circuit to the cached rows / stay valid on disk.
fn sim_cache_key(t: &ResolvedTarget) -> String {
    sim_cache_key_parts(
        t.fp.id.0,
        t.loaded.buy_amount_lamports,
        t.loaded.max_concurrent_tokens,
        t.loaded.max_total_tokens,
        &t.loaded.params.to_value(),
    )
}

/// Cache key for a saved [`StrategyRule`] after load — used to drop a durable
/// disk result when the rule's trading config changes.
pub fn sim_cache_key_for_rule(rule: &StrategyRule) -> Result<String, String> {
    let loaded = rule_to_loaded(rule)?;
    Ok(sim_cache_key_parts(
        rule.fingerprint_id,
        loaded.buy_amount_lamports,
        loaded.max_concurrent_tokens,
        loaded.max_total_tokens,
        &loaded.params.to_value(),
    ))
}

fn sim_cache_key_parts(
    fingerprint_id: Uuid,
    buy_amount_lamports: u64,
    max_concurrent_tokens: u32,
    max_total_tokens: u32,
    params: &Value,
) -> String {
    format!(
        "engine:{fingerprint_id}:{buy_amount_lamports}:{max_concurrent_tokens}:{max_total_tokens}:{params}"
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

#[cfg(test)]
mod dupe_guard_resolution {
    use super::*;

    fn settings(on: bool, hours: u64) -> AppSettings {
        AppSettings {
            skip_duplicate_identity: on,
            duplicate_identity_window_hours: hours,
            ..AppSettings::default()
        }
    }

    /// The whole point of inheriting: flip the toggle in Settings and the next
    /// backtest measures the rule under the policy live is trading it under, with no
    /// second knob to remember.
    #[test]
    fn an_absent_request_field_inherits_the_app_setting() {
        assert_eq!(resolve_dupe_guard(None, &settings(true, 168)), Some(168));
        assert_eq!(resolve_dupe_guard(None, &settings(false, 168)), None);
    }

    /// ...but a run can still A/B the guard without touching what live is doing.
    #[test]
    fn an_explicit_request_field_overrides_the_app_setting_both_ways() {
        assert_eq!(resolve_dupe_guard(Some(true), &settings(false, 72)), Some(72));
        assert_eq!(resolve_dupe_guard(Some(false), &settings(true, 72)), None);
    }

    /// A stored `0` predates the API's rejection of it. Honoring it literally would
    /// produce a run that reports the guard as ON while remembering nothing — a
    /// backtest that silently measures the wrong policy.
    #[test]
    fn a_zero_window_falls_back_to_the_default_instead_of_remembering_nothing() {
        assert_eq!(
            resolve_dupe_guard(Some(true), &settings(true, 0)),
            Some(hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS)
        );
    }
}

/// Which lake columns a rule's metrics oblige the loader to fetch.
#[cfg(test)]
mod flow_column_needs {
    use super::*;

    fn rule_with(params: serde_json::Value) -> LoadedRule {
        use hunter_engine::event::{RuleId, TradeMode};
        use hunter_engine::fingerprint::FingerprintId;
        LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: FingerprintId(Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: hunter_engine::rule_params::RuleParams::parse(&params).expect("params"),
            entry_enabled: true,
        }
    }

    /// A wallet-keyed metric must pull the lake's flow columns even when its GROUP is
    /// otherwise SOL-only.
    ///
    /// The bug this pins is silent and looks like a strategy result, not a load error:
    /// without the `wallet` column every trade folds as one anonymous wallet, so
    /// `unique_wallets >= N` reads 1 forever and the run reports zero entries — which
    /// is indistinguishable from "the gate is simply strict".
    #[test]
    fn unique_wallets_forces_the_flow_columns() {
        let uw = rule_with(serde_json::json!({
            "entry": { "m_flow_window": {
                "window_size_sec": 60,
                "unique_wallets": [{ "operator": ">=", "value": 20 }]
            } }
        }));
        assert!(rule_needs_flow(&uw), "unique_wallets needs the wallet column");

        let sol_only = rule_with(serde_json::json!({
            "entry": { "m_flow_window": {
                "window_size_sec": 60,
                "gross_flow": [{ "operator": ">=", "value": 45 }]
            } }
        }));
        assert!(!rule_needs_flow(&sol_only), "a SOL-only window must not pay for flow");
    }

    /// A ladder stage reuses the full exit grammar, so it can be the ONLY part of a
    /// rule that reads a flow metric. Checking `entry`/`exit` alone missed it.
    #[test]
    fn a_scale_out_stage_can_be_what_needs_flow() {
        let staged = rule_with(serde_json::json!({
            "scale_out": [{
                "sell_bps": 5000,
                "conditions": { "m_flow_window": {
                    "window_size_sec": 30,
                    "unique_wallets": [{ "operator": "<=", "value": 3 }]
                } }
            }]
        }));
        assert!(rule_needs_flow(&staged), "a stage's flow metric counts too");
    }
}
