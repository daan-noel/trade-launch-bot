//! Family-search job (lab only) — grades one fingerprint's sibling family, fitting
//! the ordering broad and taking the level from the held-out target cohort.
//!
//! Sibling of grouped sweep / flow-discovery / metric-discovery / rule-search, not a
//! sweep mode. Rule search is untouched.
//!
//! The cohort **loop lives here**, not in the job module, because loading is async and
//! the RAM budget is a loading decision: the target cohort stays resident (the fit
//! ranking, the level, the capture, the attribution and the narrow re-check all read
//! it) while the fit siblings iterate one at a time. Six concurrent corpora is how a
//! run OOMs.
//!
//! Charter: `hunter/docs/roadmap/family-search.md`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use crate::family_search::dto::StartFamilySearchBody;
use crate::family_search::generator::{parse_standing_all, Candidate, GeneratorConfig, StandingTerm};
use crate::family_search::report::{
    attribution_rows, CandidateRow, FamilyDto, FreshnessDto, LibraryDto, Report, SelectionDto,
    SiblingRow,
};
use crate::family_search::score::{broad_fit, select, wilson_low_pct, CohortScore, SelectionBars};
use crate::family_search::{
    attribution, authority, capture_of, check_cancelled, cut_table, earn_candidates, entry_gates,
    entry_timing, family, gates, narrow_enrich, narrow_recheck, optimistic, score_cohort, spread,
    Authority, CohortRun, RunConfig,
};
use crate::lake::duck::LakeSource;
use crate::models::ingest::SseEvent;
use crate::state::job_progress::ProgressCell;
use crate::state::local_state::{HeavyJob, LocalState};
use crate::sweep::corpus::{sweep_per_mint_cap, CorpusSource, Selection};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;
use crate::sweep::registry::clamp_token_cap;
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::rule_params::RuleParams;
use trading_core::models::Fingerprint;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;

// ── Progress observer ──────────────────────────────────────────────────────

struct FamilyObserver {
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    run_id: Uuid,
    cancel: Arc<AtomicBool>,
    cell: Arc<ProgressCell>,
    total: AtomicUsize,
    done: AtomicUsize,
    phase: std::sync::Mutex<String>,
}

impl FamilyObserver {
    fn set_phase(&self, phase: &str) {
        if let Ok(mut g) = self.phase.lock() {
            *g = phase.to_string();
        }
    }

    fn phase(&self) -> String {
        self.phase.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn emit(&self, processed: usize, total: usize) {
        let _ = self.sse_tx.send(SseEvent::FamilySearchProgress {
            run_id: self.run_id,
            phase: self.phase(),
            processed: processed as u64,
            total: total as u64,
        });
    }
}

impl SweepObserver for FamilyObserver {
    fn set_total(&self, total_tokens: usize, combos_per_token: usize) {
        let total = total_tokens.saturating_mul(combos_per_token.max(1));
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.cell.set_total(total_tokens as u64);
        self.cell.set_processed(0);
        self.emit(0, total);
    }

    fn token_done(&self, combos_folded: usize) {
        let total = self.total.load(Ordering::Relaxed).max(1);
        let prev = self.done.fetch_add(combos_folded, Ordering::Relaxed);
        let n = prev + combos_folded;
        let step = (total / 100).max(1);
        if n >= total || (prev / step) != (n / step) {
            self.cell.set_processed(n as u64);
            self.emit(n, total);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn notice(&self, message: &str) {
        self.set_phase(message);
        self.emit(self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed));
    }
}

// ── Gate ───────────────────────────────────────────────────────────────────

struct Gate {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    progress: Arc<ProgressCell>,
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    run_id: Uuid,
    error: Option<String>,
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.progress.reset();
        let cancelled = self.cancel.load(Ordering::Acquire);
        let _ = self.sse_tx.send(SseEvent::FamilySearchFinished {
            run_id: self.run_id,
            cancelled,
            error: self.error.clone(),
        });
        self.running.store(false, Ordering::Release);
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// `POST /api/strategies/family-search` → `202 { run_id, status }`.
pub async fn start_family_search(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<StartFamilySearchBody>,
) -> impl Responder {
    if let Err(msg) = state.claim_heavy(HeavyJob::FamilySearch) {
        return HttpResponse::Conflict().json(serde_json::json!({ "error": msg }));
    }
    let b = body.into_inner();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_job(state.clone(), b, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "family-search job dropped before admission" }))
    })
}

/// `POST /api/strategies/family-search/cancel`
pub async fn cancel_family_search(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let cancelling = if state.family_search_running.load(Ordering::Acquire) {
        state.family_search_cancel.store(true, Ordering::Release);
        true
    } else {
        false
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

/// `GET /api/strategies/family-search/{run_id}`
pub async fn get_family_search(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let run_id = path.into_inner();
    match state.family_search_result.get(run_id).await {
        Some(result) => {
            HttpResponse::Ok().json(serde_json::json!({ "run_id": run_id, "result": result }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no family-search result for that run_id (still running, superseded, or unknown)"
        })),
    }
}

/// `GET /api/strategies/family-search/last` — cached result for page rehydrate.
pub async fn get_last_family_search(state: web::Data<Arc<LocalState>>) -> impl Responder {
    match state.family_search_result.get_last().await {
        Some((run_id, result)) => {
            HttpResponse::Ok().json(serde_json::json!({ "run_id": run_id, "result": result }))
        }
        None => HttpResponse::NotFound()
            .json(serde_json::json!({ "error": "no cached family-search result" })),
    }
}

// ── Job ────────────────────────────────────────────────────────────────────

async fn run_job(
    state: web::Data<Arc<LocalState>>,
    b: StartFamilySearchBody,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
    let run_id = Uuid::new_v4();
    let mut gate = Gate {
        running: state.family_search_running.clone(),
        cancel: state.family_search_cancel.clone(),
        progress: state.family_search_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        run_id,
        error: None,
    };
    state.family_search_cancel.store(false, Ordering::Release);
    state.family_search_progress.reset();

    // D10: a standing term that does not parse fails the run. Dropping one silently
    // would score a rule the operator did not ask for and report it as theirs.
    if let Err(e) = parse_standing_all(&b.standing_exit) {
        let msg = e.to_string();
        gate.error = Some(msg.clone());
        let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
        return;
    }

    let repo = FingerprintRepo::new(state.db.clone());
    let fam = match family::resolve(&repo, b.fingerprint_id, b.varied_axis.map(Into::into)).await {
        Ok(f) => f,
        Err(e) => {
            let msg = e.to_string();
            gate.error = Some(msg.clone());
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
            return;
        }
    };
    let rows = match repo.list().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("family-search: fingerprint list failed: {e}");
            gate.error = Some(e.to_string());
            let _ = early_tx
                .send(HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" })));
            return;
        }
    };

    // D6: an incumbent is an artifact. It supplies NO buy size, NO cap, NO threshold
    // and NO structure — only one more display column.
    let incumbent_params: Option<RuleParams> = match b.incumbent_rule_id {
        None => None,
        Some(rule_id) => match RuleRepo::new(state.db.clone()).find(rule_id).await {
            Ok(Some(r)) => match RuleParams::parse(&r.params) {
                Ok(p) => Some(p),
                Err(e) => {
                    let msg = format!("incumbent rule params are invalid: {e}");
                    gate.error = Some(msg.clone());
                    let _ = early_tx
                        .send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
                    return;
                }
            },
            Ok(None) => {
                let msg = format!("incumbent rule {rule_id} not found");
                gate.error = Some(msg.clone());
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
                return;
            }
            Err(e) => {
                tracing::error!("family-search: incumbent lookup failed: {e}");
                gate.error = Some(e.to_string());
                let _ = early_tx.send(
                    HttpResponse::InternalServerError().json(serde_json::json!({ "error": "database error" })),
                );
                return;
            }
        },
    };

    // 202 after cheap admission so the HTTP request is not held for six lake loads.
    let _ = early_tx.send(
        HttpResponse::Accepted().json(serde_json::json!({ "run_id": run_id, "status": "started" })),
    );

    if let Err(e) = drive(&state, run_id, &b, &fam, &rows, incumbent_params).await {
        if !state.family_search_cancel.load(Ordering::Acquire) {
            tracing::error!("family-search failed: {e}");
            gate.error = Some(e.to_string());
        }
    }
}

/// One family member's resolved scope: the row, its engine form, the varied axis's
/// value, whether it is the held-out target, its matched mints, and whether the cap
/// truncated them.
type Scope = (Fingerprint, EngineFingerprint, Option<f64>, bool, Vec<String>, bool);

/// Scope resolve is **dimension-only** (`matching_mints` reads the tokens Parquet and
/// never scans trades), so every cohort's scope is resolved up front: an empty cohort
/// then fails before any trade load.
async fn resolve_scopes(
    src: &LakeSource,
    sel: &Selection,
    fam: &family::Family,
    rows: &[Fingerprint],
) -> anyhow::Result<Vec<Scope>> {
    let mut out = Vec::new();
    for m in &fam.members {
        let Some(row) = rows.iter().find(|r| r.id == m.fp_id) else { continue };
        let engine_fp = fp_to_engine(row);
        let (mints, capped) = src.matching_mints(sel, engine_fp.clone()).await?;
        out.push((row.clone(), engine_fp, m.value, m.is_target, mints, capped));
    }
    Ok(out)
}

#[allow(clippy::too_many_lines)]
async fn drive(
    state: &web::Data<Arc<LocalState>>,
    run_id: Uuid,
    b: &StartFamilySearchBody,
    fam: &family::Family,
    rows: &[Fingerprint],
    incumbent: Option<RuleParams>,
) -> anyhow::Result<()> {
    let token_cap = clamp_token_cap(b.token_cap);
    // Freeze deadness "now" at session open so every cohort shares one horizon.
    let as_of = chrono::Utc::now();
    // The bound the OPERATOR named, never a fabricated one: an absent `created_before`
    // asks for whatever the lake holds, and substituting `now` there refuses every
    // open-ended run because a sealed-day export is always hours behind the clock.
    let requested_until = b.created_before;

    let target_row = rows
        .iter()
        .find(|r| r.id == fam.target)
        .ok_or_else(|| anyhow::anyhow!("target fingerprint vanished from the table"))?;
    let flow = FlowPatterns::from_metric_config(&target_row.metric_config);
    let with_flow = flow.is_some();

    let base_sel = Selection {
        mints: None,
        token_cap,
        created_after: b.created_after,
        created_before: b.created_before,
        per_mint_cap: sweep_per_mint_cap(),
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: false,
        with_signatures: false,
        with_flow,
        with_flow_text: false,
        // The oracle denominator (D3) — the one job that reads it.
        with_oracle: true,
    };

    let notice = |message: String| {
        let _ = state.sse_tx.send(SseEvent::FamilySearchNotice { run_id, message });
    };
    let phase = |p: &str| {
        let _ = state.sse_tx.send(SseEvent::FamilySearchProgress {
            run_id,
            phase: p.to_string(),
            processed: 0,
            total: 0,
        });
    };

    phase("scope");
    let src = LakeSource::new(crate::lake::lake_root());
    let scopes = resolve_scopes(&src, &base_sel, fam, rows).await?;
    if scopes.iter().all(|s| s.4.is_empty()) {
        anyhow::bail!(
            "no tokens match fingerprint “{}” or any of its siblings — widen the range or cap",
            target_row.name
        );
    }
    for (row, _, _, _, mints, capped) in &scopes {
        // The cheapest guard against an approximate cohort: `n_matched` against a hand
        // count, every run. An ix-labels-only approximation of one reference cohort
        // takes 3,440 tokens where the engine takes 264.
        notice(format!("{}: {} tokens matched", row.name, mints.len()));
        if *capped {
            notice(format!(
                "{}: hit the {token_cap}-token cap — only the newest {token_cap} were scored.",
                row.name
            ));
        }
    }

    let settings = state.core.settings();
    let dupe_hours = match settings.duplicate_identity_window_hours {
        0 => hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS,
        h => h,
    };
    // Every one of these comes from the REQUEST (D5) — never from the incumbent.
    let cfg = RunConfig {
        pricing: Pricing {
            buy_amount_sol: b.buy_amount_sol,
            fill_model: b.fill_model,
            cost: b.cost_model.model(),
        },
        as_of,
        skip_duplicate_identity: b.skip_duplicate_identity,
        duplicate_identity_window_hours: dupe_hours,
        max_concurrent_tokens: b.max_concurrent_tokens,
        max_total_tokens: b.max_total_tokens,
        generator: GeneratorConfig { slots: b.slots.max(1), ..Default::default() },
    };

    let observer = Arc::new(FamilyObserver {
        sse_tx: state.sse_tx.clone(),
        run_id,
        cancel: state.family_search_cancel.clone(),
        cell: state.family_search_progress.clone(),
        total: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        phase: std::sync::Mutex::new("scope".into()),
    });
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(1);
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("family-search-{i}"))
            .stack_size(8 * 1024 * 1024)
            .build()?,
    );

    // ── Target cohort: load once, keep resident. ────────────────────────────
    let target_scope = scopes
        .iter()
        .find(|s| s.3)
        .ok_or_else(|| anyhow::anyhow!("family has no target member"))?;
    phase("corpus target");
    let mut sel = base_sel.clone();
    sel.mints = Some(target_scope.4.clone());
    let target_corpus = Arc::new(src.load(&sel).await?);
    if target_corpus.tokens.is_empty() {
        anyhow::bail!("no tokens in that date range for the target — widen the selection");
    }

    // D7 is a gate, not a footnote: a window silently two days short answers a
    // different question and nothing downstream can detect it.
    let freshness =
        gates::check_freshness(&target_corpus, requested_until, b.freshness_slack_secs)?;
    // An open-ended range cannot be refused, so it says out loud what it covered —
    // "everything the lake holds" is only an answer once the operator sees where the
    // lake ends.
    if requested_until.is_none() {
        if let Some(last) = freshness.last_trade_at {
            notice(format!("no upper bound set — the run covers through {last} (lake tail)."));
        }
    }

    // ── D8: can this cohort pay for its own execution? ─────────────────────
    //
    // Before the generator, because a search over a cohort whose available moves live
    // inside the round trip is spent on a question with no answer. The instrument is
    // the **ungated control's** oracle — rule-free, and no exit rule beats the best
    // exit. The control pass is not extra work: it is the board's ungated column,
    // moved earlier.
    // Parsed up front in `run_job`; this cannot fail.
    let standing: Vec<StandingTerm> = parse_standing_all(&b.standing_exit)?;
    // The control carries the standing terms too — comparing a gated rule that sells
    // at migration against a control that does not is comparing two different rules.
    let control = crate::family_search::generator::ungated_control(&standing);
    let target_fp = target_scope.1.clone();
    phase("clearance");
    let (control_auth, clearance) = {
        let corpus = target_corpus.clone();
        let cfg2 = cfg.clone();
        let fp2 = target_fp.clone();
        let control2 = control.clone();
        let margin = b.cost_clearance_margin;
        let pool = pool.clone();
        tokio::task::spawn_blocking(move || {
            pool.install(|| {
                let a = authority(&corpus.tokens, &fp2, &control2.params, &cfg2);
                let moves = crate::family_search::oracle::oracle_moves(
                    &corpus.tokens,
                    &a.outcomes,
                    &a.token_idx,
                    &cfg2.pricing,
                );
                let band = crate::family_search::oracle::execution_band_pct(
                    &cfg2.pricing,
                    crate::family_search::oracle::median(&moves.depth_sol),
                );
                let c = gates::cost_clearance(&moves.pct, moves.n_with_upside, band, margin);
                (a, c)
            })
        })
        .await?
    };
    if let Some(why) = clearance.refuse_reason() {
        notice(format!("refused before generating: {why}"));
        // A refusal still boards. The freshness gate can only say "re-sync"; this one
        // has a measurement behind it, and burying that in an error string would hide
        // the one number that decides whether this launch shape is worth revisiting.
        let report = refusal_report(target_row, fam, &scopes, freshness, clearance, why);
        state.family_search_result.store(run_id, report).await;
        return Ok(());
    }
    // The entry side's own score, with no rule at all: what share of everything this
    // cohort offers never had a profitable exit. The draft has to beat it (D11).
    let ungated_capture = {
        let corpus = target_corpus.clone();
        let pricing = cfg.pricing;
        pool.install(|| capture_of(&corpus.tokens, &control_auth, &pricing))
    };

    // ── Earn the candidate menu from the target's own paths (D5). ───────────
    check_cancelled(observer.as_ref())?;
    phase("signatures");
    let cuts = {
        let corpus = target_corpus.clone();
        let flow = flow.clone();
        let fp_id = target_scope.1.id;
        pool.install(move || cut_table(&corpus.tokens, flow.as_ref(), fp_id))
    };
    let library = crate::family_search::generator::generate(&cuts, &cfg.generator, &standing);
    // `earn_candidates` is the same two steps; the table is kept because the enrich
    // stage draws its menu from the very same earning pass, never a second one.
    debug_assert_eq!(
        library.kept.len(),
        {
            let corpus = target_corpus.clone();
            let flow = flow.clone();
            earn_candidates(
                &corpus.tokens,
                flow.as_ref(),
                target_scope.1.id,
                &cfg.generator,
                &standing,
            )
            .kept
            .len()
        },
        "the generator and the cut table must come from one earning pass"
    );
    if library.kept.is_empty() {
        anyhow::bail!("the signature menu earned no candidate on this cohort");
    }
    let candidates: Arc<Vec<Candidate>> = Arc::new(library.kept.clone());

    // ── Score every cohort. Target first (resident), siblings one at a time. ─
    let mut cohorts: Vec<CohortRun> = Vec::new();
    for (row, engine_fp, axis_value, is_target, mints, _) in &scopes {
        // A corpus load cannot be cancelled mid-flight, so this checkpoint — between
        // one sibling's fold and the next one's load — is the whole cancellation story
        // for the longest phase of the run.
        check_cancelled(observer.as_ref())?;
        if mints.is_empty() {
            notice(format!("{}: no matched tokens — excluded from the fit", row.name));
            continue;
        }
        phase(&format!("fit {}", row.name));
        observer.set_phase(&format!("fit {}", row.name));

        let corpus = if *is_target {
            target_corpus.clone()
        } else {
            let mut sel = base_sel.clone();
            sel.mints = Some(mints.clone());
            Arc::new(src.load(&sel).await?)
        };
        if corpus.tokens.is_empty() {
            notice(format!("{}: no trades in range — excluded from the fit", row.name));
            continue;
        }

        let n_matched = corpus.tokens.len() as u64;
        let (scores, enter_pct, ungated) = {
            let corpus = corpus.clone();
            let candidates = candidates.clone();
            let flow = flow.clone();
            let cfg = cfg.clone();
            let observer = observer.clone();
            let fp_id = engine_fp.id;
            let control = control.clone();
            let pool = pool.clone();
            tokio::task::spawn_blocking(move || {
                pool.install(|| {
                    score_cohort(
                        &corpus.tokens,
                        &candidates,
                        Some(&control),
                        flow.as_ref(),
                        fp_id,
                        &cfg,
                        observer.as_ref(),
                    )
                })
            })
            .await??
        };

        cohorts.push(CohortRun {
            fp_id: row.id,
            name: row.name.clone(),
            axis_value: *axis_value,
            is_target: *is_target,
            n_matched,
            scores,
            enter_pct,
            ungated,
        });
        // The sibling's corpus drops here — one at a time, never six.
    }

    let target_run = cohorts
        .iter()
        .find(|c| c.is_target)
        .ok_or_else(|| anyhow::anyhow!("the target cohort produced no score"))?
        .clone();

    // ── Fit broad, validate narrow (D1/D2). ────────────────────────────────
    let n = candidates.len();
    let fit: Vec<Vec<CohortScore>> = (0..n)
        .map(|ci| {
            cohorts
                .iter()
                .filter(|c| !c.is_target)
                .filter_map(|c| c.scores.get(ci).copied())
                .collect()
        })
        .collect();
    let validate: Vec<CohortScore> =
        (0..n).map(|ci| target_run.scores.get(ci).copied().unwrap_or_default()).collect();
    let bf = broad_fit(&fit, &validate);

    // ── Two-sided selection (D11): rank broad, then clear BOTH bars narrow. ─
    //
    // Entry decides safety and exit decides profit, so the ranking alone cannot pick a
    // draft. The win-rate bar is the ungated control's own rate — a gate that does not
    // enter more safely than buying everything is not filtering anything — raised by
    // whatever absolute floor the request set.
    let bars = SelectionBars {
        control_win_pct: target_run.ungated.and_then(|u| u.win_rate_pct()),
        floor_win_pct: b.min_win_rate_pct,
        min_closed: b.min_closed,
    };
    let sel = select(&bf, &validate, bars);
    // With nothing clearing the bars there is still a rule to show and explain; the
    // report carries `none_cleared` so the board never presents it as a draft.
    let winner = sel.chosen.or(sel.top_ranked).unwrap_or(0);
    let skeleton = candidates[winner].clone();

    // ── Authority pass + enrich: the target cohort and the finalist only. ───
    check_cancelled(observer.as_ref())?;
    phase("authority");
    let standing_keys: Vec<_> = standing
        .iter()
        .map(|s| (s.clause.metric, s.clause.window, s.clause.threshold))
        .collect();
    let (finalist, auth, capture, narrow, timing, spread_of_draft, incumbent_auth, enriched, diag) = {
        let corpus = target_corpus.clone();
        let cfg2 = cfg.clone();
        let fp2 = target_fp.clone();
        let skeleton2 = skeleton.clone();
        let incumbent2 = incumbent.clone();
        let standing2 = standing.clone();
        let standing_keys2 = standing_keys.clone();
        let cuts2 = cuts.clone();
        let min_closed = b.min_closed;
        // The regret verdicts read the cohort's own round trip — upside inside the
        // band is not forfeitable.
        let band = clearance.band_pct;
        let pool = pool.clone();
        tokio::task::spawn_blocking(move || {
            pool.install(|| {
                // D12: the only stage that can make a rule DENSER. Offer every earned
                // idea the skeleton lacks, judge each in its own side's currency, keep
                // what pays — then everything below grades the enriched rule.
                let en = narrow_enrich(
                    &corpus.tokens,
                    &fp2,
                    &skeleton2,
                    &cuts2,
                    &standing2,
                    min_closed,
                    &cfg2,
                );
                let mut fin = skeleton2.clone();
                if let Some(combo) = en.combo.clone() {
                    fin.n_entry_quantities += en
                        .trials
                        .iter()
                        .filter(|t| t.accepted && t.is_entry)
                        .count();
                    fin.combo = combo;
                }
                let a = authority(&corpus.tokens, &fp2, &fin.combo.params, &cfg2);
                let cap = capture_of(&corpus.tokens, &a, &cfg2.pricing);
                let nr = narrow_recheck(
                    &corpus.tokens,
                    &fp2,
                    &fin.combo,
                    fin.n_standing,
                    &cfg2,
                    a.score,
                );
                let tm = entry_timing(&corpus.tokens, &fp2, &fin.combo, &cfg2, &a);
                // D8 corollary: the same taken set at the friendliest honest fill.
                let opt = optimistic(&corpus.tokens, &fp2, &fin.combo.params, &cfg2);
                let sp = spread(&corpus.tokens, &a, &opt, cfg2.pricing.buy_amount_sol);
                let inc = incumbent2.map(|p| authority(&corpus.tokens, &fp2, &p, &cfg2));
                // Slice 7: reliability diagnostics on the finalist — ladders, regret,
                // redundancy, per-clause fill. Grades trust, never selection.
                let dg = crate::family_search::diagnose::diagnose(
                    &corpus.tokens,
                    &fp2,
                    &fin,
                    &a,
                    &cfg2,
                    band,
                    &standing_keys2,
                );
                (fin, a, cap, nr, tm, sp, inc, en, dg)
            })
        })
        .await?
    };

    let attribution = attribution::rollup_with_standing(
        &auth.outcomes,
        cfg.pricing.buy_amount_sol,
        &standing_keys,
    );
    let (alarm_rows, other_n, other_pnl) = attribution_rows(&attribution);
    let gate_rows: Vec<_> = entry_gates(&finalist, winner, &cohorts)
        .into_iter()
        .map(Into::into)
        .collect();

    // ── Board. ─────────────────────────────────────────────────────────────
    let row_of = |ci: usize| -> CandidateRow {
        let c = &candidates[ci];
        let s = target_run.scores[ci];
        CandidateRow {
            key: c.key(),
            params: c.combo.params.to_value(),
            families: c.families.iter().map(|f| f.label().to_string()).collect(),
            flags: c.flags.iter().map(|s| s.to_string()).collect(),
            fit_ret_pct: bf.ret_fit[ci],
            target_ret_pct: bf.ret_validate[ci],
            target_pnl_sol: s.pnl_sol,
            target_n_tokens: (s.entry_sol / cfg.pricing.buy_amount_sol).round() as u64,
            target_enter_pct: target_run.enter_pct[ci],
            target_win_pct: s.win_rate_pct(),
            target_n_closed: s.n_closed,
            n_entry_quantities: c.n_entry_quantities,
            n_alarms: c.searched_exit().len(),
        }
    };
    let plain_row = |name: &str, a: &Authority, params: &RuleParams| CandidateRow {
        key: name.to_string(),
        params: params.to_value(),
        families: Vec::new(),
        flags: Vec::new(),
        // A control has no fit ranking of its own — it is a property of the cohort.
        fit_ret_pct: 0.0,
        target_ret_pct: a.score.ret_pct(),
        target_pnl_sol: a.score.pnl_sol,
        target_n_tokens: a.n_tokens,
        target_enter_pct: if target_run.n_matched == 0 {
            0.0
        } else {
            a.n_tokens as f64 / target_run.n_matched as f64
        },
        target_win_pct: a.score.win_rate_pct(),
        target_n_closed: a.score.n_closed,
        n_entry_quantities: 0,
        n_alarms: 0,
    };

    let mut archive: Vec<CandidateRow> = bf.rank_fit.iter().take(24).map(|&ci| row_of(ci)).collect();
    // The draft's level is the HELD-OUT number, replayed under the authority pass —
    // never the fast archive's, and never the fit level. It also describes the
    // ENRICHED rule, which the fast tier never scored.
    let mut draft = row_of(winner);
    draft.key = finalist.key();
    draft.params = finalist.combo.params.to_value();
    draft.families = finalist.families.iter().map(|f| f.label().to_string()).collect();
    draft.n_entry_quantities = finalist.n_entry_quantities;
    draft.n_alarms = finalist.searched_exit().len();
    draft.target_ret_pct = auth.score.ret_pct();
    draft.target_pnl_sol = auth.score.pnl_sol;
    draft.target_n_tokens = auth.n_tokens;
    draft.target_win_pct = auth.score.win_rate_pct();
    draft.target_n_closed = auth.score.n_closed;
    draft.target_enter_pct = if target_run.n_matched == 0 {
        0.0
    } else {
        auth.n_tokens as f64 / target_run.n_matched as f64
    };
    if let Some(first) = archive.first_mut() {
        *first = draft.clone();
    }

    let mut report = Report {
        fingerprint_id: target_row.id,
        fingerprint_name: target_row.name.clone(),
        family: FamilyDto {
            varied_axis: fam.varied.map(|a| a.column().to_string()),
            single_cohort: fam.is_single() || cohorts.len() <= 1,
            members: cohorts
                .iter()
                .map(|c| SiblingRow {
                    fp_id: c.fp_id,
                    name: c.name.clone(),
                    axis_value: c.axis_value,
                    is_target: c.is_target,
                    n_matched: c.n_matched,
                    ungated_ret_pct: c.ungated.map(|u| u.ret_pct()),
                    ungated_win_pct: c.ungated.and_then(|u| u.win_rate_pct()),
                })
                .collect(),
        },
        freshness: FreshnessDto::from(freshness),
        library: LibraryDto {
            n_candidates: candidates.len() as u64,
            dropped_by_quota: library.dropped_by_quota as u64,
            by_family: library
                .by_family
                .iter()
                .map(|(f, n)| (f.label().to_string(), *n as u64))
                .collect(),
        },
        rho: bf.rho,
        fit_broad_holds: bf.holds(),
        // A rule nothing cleared the bars for is not a draft. It still boards, as the
        // archive's head with the refusal spelled out beside it.
        draft: sel.chosen.is_some().then(|| draft.clone()),
        selection: {
            // The honesty layer under the win bar (Slice 7): the bound is computed on
            // the reported draft (the enriched finalist's authority pass) and is a
            // diagnostic only — it never un-selects.
            let draft_win_low_pct = sel
                .chosen
                .is_some()
                .then(|| wilson_low_pct(auth.score.n_wins, auth.score.n_closed))
                .flatten();
            Some(SelectionDto {
                win_bar_pct: bars.win_bar_pct(),
                control_win_pct: bars.control_win_pct,
                floor_win_pct: bars.floor_win_pct,
                min_closed: bars.min_closed,
                n_rejected: sel.n_rejected,
                top_rejected: sel.top_rejected.iter().map(|r| r.label().to_string()).collect(),
                none_cleared: sel.chosen.is_none(),
                win_within_noise: draft_win_low_pct.is_some_and(|l| l < bars.win_bar_pct()),
                draft_win_low_pct,
            })
        },
        ungated_control: Some(plain_row("ungated control", &control_auth, &control.params)),
        capture: capture.into(),
        ungated_capture: Some(ungated_capture.into()),
        standing_terms: standing.iter().map(|s| s.label.clone()).collect(),
        enrich: enriched.trials.into_iter().map(Into::into).collect(),
        cost_clearance: Some(clearance.into()),
        spread: Some(spread_of_draft.into()),
        entry_timing: timing.into_iter().map(Into::into).collect(),
        incumbent: incumbent_auth
            .as_ref()
            .zip(incumbent.as_ref())
            .map(|(a, p)| plain_row("incumbent (display only)", a, p)),
        attribution: alarm_rows,
        attribution_other_n: other_n,
        attribution_other_pnl_sol: other_pnl,
        narrow_recheck: narrow.into_iter().map(Into::into).collect(),
        threshold_ladders: diag.ladders.into_iter().map(Into::into).collect(),
        alarm_regret: diag.regret.into_iter().map(Into::into).collect(),
        entry_redundancy: diag.redundancy.into_iter().map(Into::into).collect(),
        fill_sensitivity: diag.fill_sensitivity.into_iter().map(Into::into).collect(),
        entry_gates: gate_rows,
        archive,
        portrait: Vec::new(),
        diagnostics: Vec::new(),
    };
    if sel.chosen.is_none() {
        report.diagnostics.push(format!(
            "No candidate cleared both bars on the held-out cohort, so there is no draft. \
             {} were tried; the entry side had to win more than {:.0}% of its closes (what \
             buying everything achieves here) and the exit side had to make money. The row \
             below is the ranking's head, shown so you can see how close it came.",
            sel.n_rejected,
            bars.win_bar_pct()
        ));
    }
    if !report.fit_broad_holds && !report.family.single_cohort {
        report.diagnostics.push(
            "Fit-broad does not hold on this family: the pooled ordering did not transfer to \
             the held-out cohort. Treat the ranking as unestablished."
                .into(),
        );
    }
    if clearance.thin() {
        report.diagnostics.push(format!(
            "This cohort clears its execution cost by less than one round trip \
             ({:.1}x). A rule takes a fraction of the best available exit, so the \
             headroom a draft actually has is smaller than it looks.",
            clearance.headroom().unwrap_or(0.0)
        ));
    }
    if !spread_of_draft.clean() {
        report.diagnostics.push(format!(
            "The two pricings did not close the same positions ({} authority-only, {} \
             optimistic-only), so the spread is indicative rather than one taken set \
             measured twice.",
            spread_of_draft.n_authority_only, spread_of_draft.n_optimistic_only
        ));
    }
    report.portrait = crate::family_search::report::portrait(&report);

    state.family_search_result.store(run_id, report).await;
    Ok(())
}

/// The board for a cohort refused on execution cost (D8).
///
/// It carries the family, the freshness it passed, and the measurement that refused
/// it — everything except a draft, because no search ran. `library` is deliberately
/// empty rather than absent: zero candidates generated is the finding.
fn refusal_report(
    target_row: &Fingerprint,
    fam: &family::Family,
    scopes: &[Scope],
    freshness: gates::Freshness,
    clearance: gates::CostClearance,
    why: String,
) -> Report {
    let mut report = Report {
        fingerprint_id: target_row.id,
        fingerprint_name: target_row.name.clone(),
        family: FamilyDto {
            varied_axis: fam.varied.map(|a| a.column().to_string()),
            single_cohort: fam.is_single(),
            members: scopes
                .iter()
                .map(|(row, _, axis_value, is_target, mints, _)| SiblingRow {
                    fp_id: row.id,
                    name: row.name.clone(),
                    axis_value: *axis_value,
                    is_target: *is_target,
                    n_matched: mints.len() as u64,
                    // Nothing was scored, so there is no per-cohort number to give.
                    ungated_ret_pct: None,
                    ungated_win_pct: None,
                })
                .collect(),
        },
        freshness: FreshnessDto::from(freshness),
        library: LibraryDto::default(),
        rho: None,
        fit_broad_holds: false,
        draft: None,
        selection: None,
        ungated_control: None,
        capture: Default::default(),
        ungated_capture: None,
        standing_terms: Vec::new(),
        enrich: Vec::new(),
        cost_clearance: Some(clearance.into()),
        spread: None,
        entry_timing: Vec::new(),
        incumbent: None,
        attribution: Vec::new(),
        attribution_other_n: 0,
        attribution_other_pnl_sol: 0.0,
        narrow_recheck: Vec::new(),
        threshold_ladders: Vec::new(),
        alarm_regret: Vec::new(),
        entry_redundancy: Vec::new(),
        fill_sensitivity: Vec::new(),
        entry_gates: Vec::new(),
        archive: Vec::new(),
        portrait: Vec::new(),
        diagnostics: vec![format!("Search refused before generating: {why}")],
    };
    report.portrait = crate::family_search::report::portrait(&report);
    report
}
