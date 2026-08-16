//! Rule-search job (lab only) — finds one champion `RuleParams` for a fingerprint
//! and a datetime range. Sibling of grouped sweep / flow-discovery / metric-discovery,
//! not a sweep mode.
//!
//! Architecture: `hunter/docs/arch/sweep.md` "Rule search".
//! Method: `hunter/docs/plans/strategies/rule-search-method.md`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use crate::lake::duck::LakeSource;
use crate::models::ingest::SseEvent;
use crate::rule_search::dto::StartRuleSearchBody;
use crate::rule_search::{run_search, SearchInput};
use crate::state::job_progress::ProgressCell;
use crate::state::local_state::{HeavyJob, LocalState};
use crate::sweep::corpus::{sweep_per_mint_cap, CorpusSource, Selection};
use crate::sweep::generic::Pricing;
use crate::sweep::progress::SweepObserver;
use crate::sweep::registry::clamp_token_cap;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::rule_params::RuleParams;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;
use trading_core::strategies::paper_fill::FillModel;

// ── Progress observer ──────────────────────────────────────────────────────

struct SearchObserver {
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    run_id: Uuid,
    cancel: Arc<AtomicBool>,
    cell: Arc<ProgressCell>,
    total: AtomicUsize,
    done: AtomicUsize,
    phase: std::sync::Mutex<String>,
}

impl SearchObserver {
    fn set_phase(&self, phase: &str) {
        if let Ok(mut g) = self.phase.lock() {
            *g = phase.to_string();
        }
    }

    fn phase(&self) -> String {
        self.phase.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn emit(&self, processed: usize, total: usize) {
        let _ = self.sse_tx.send(SseEvent::RuleSearchProgress {
            run_id: self.run_id,
            phase: self.phase(),
            processed: processed as u64,
            total: total as u64,
        });
    }
}

impl SweepObserver for SearchObserver {
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
        // `run_search` uses notice as phase labels (`cuts` / `generate` / …).
        self.set_phase(message);
        self.emit(
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        );
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
        let _ = self.sse_tx.send(SseEvent::RuleSearchFinished {
            run_id: self.run_id,
            cancelled,
            error: self.error.clone(),
        });
        self.running.store(false, Ordering::Release);
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// `POST /api/strategies/rule-search` → `202 { run_id, status }`.
pub async fn start_rule_search(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<StartRuleSearchBody>,
) -> impl Responder {
    if let Err(msg) = state.claim_heavy(HeavyJob::RuleSearch) {
        return HttpResponse::Conflict().json(serde_json::json!({ "error": msg }));
    }

    let b = body.into_inner();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_job(state.clone(), b, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "rule-search job dropped before admission" }))
    })
}

/// `POST /api/strategies/rule-search/cancel`
pub async fn cancel_rule_search(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let cancelling = if state.rule_search_running.load(Ordering::Acquire) {
        state.rule_search_cancel.store(true, Ordering::Release);
        true
    } else {
        false
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

/// `GET /api/strategies/rule-search/{run_id}`
pub async fn get_rule_search(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let run_id = path.into_inner();
    match state.rule_search_result.get(run_id).await {
        Some(result) => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "result": result,
        })),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no rule-search result for that run_id (still running, superseded, or unknown)"
        })),
    }
}

/// `GET /api/strategies/rule-search/last` — cached result for page rehydrate.
pub async fn get_last_rule_search(state: web::Data<Arc<LocalState>>) -> impl Responder {
    match state.rule_search_result.get_last().await {
        Some((run_id, result)) => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "result": result,
        })),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no cached rule-search result"
        })),
    }
}

// ── Job ────────────────────────────────────────────────────────────────────

async fn run_job(
    state: web::Data<Arc<LocalState>>,
    b: StartRuleSearchBody,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
    let run_id = Uuid::new_v4();
    let mut gate = Gate {
        running: state.rule_search_running.clone(),
        cancel: state.rule_search_cancel.clone(),
        progress: state.rule_search_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        run_id,
        error: None,
    };
    state.rule_search_cancel.store(false, Ordering::Release);
    state.rule_search_progress.reset();

    let token_cap = clamp_token_cap(b.token_cap);
    let fp_id = b.fingerprint_id;

    let repo = FingerprintRepo::new(state.db.clone());
    let fp_row = match repo.find(fp_id).await {
        Ok(Some(f)) => f,
        Ok(None) => {
            let msg = format!("fingerprint {fp_id} not found");
            gate.error = Some(msg.clone());
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
            return;
        }
        Err(e) => {
            tracing::error!("rule-search: fingerprint lookup failed: {e}");
            gate.error = Some(e.to_string());
            let _ = early_tx.send(HttpResponse::InternalServerError().json(
                serde_json::json!({ "error": "database error" }),
            ));
            return;
        }
    };
    let engine_fp = fp_to_engine(&fp_row);
    let flow = FlowPatterns::from_metric_config(&fp_row.metric_config);
    let with_flow = flow.is_some();

    let mut buy_amount_sol = b.buy_amount_sol;
    let mut max_concurrent: u32 = 0;
    let mut max_total: u32 = 0;
    let mut incumbent_params: Option<RuleParams> = None;

    if let Some(rule_id) = b.incumbent_rule_id {
        let rule = match RuleRepo::new(state.db.clone()).find(rule_id).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                let msg = format!("incumbent rule {rule_id} not found");
                gate.error = Some(msg.clone());
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
                return;
            }
            Err(e) => {
                tracing::error!("rule-search: incumbent lookup failed: {e}");
                gate.error = Some(e.to_string());
                let _ = early_tx.send(HttpResponse::InternalServerError().json(
                    serde_json::json!({ "error": "database error" }),
                ));
                return;
            }
        };
        if rule.fingerprint_id != fp_id {
            let msg = "incumbent rule belongs to a different fingerprint";
            gate.error = Some(msg.into());
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
            return;
        }
        buy_amount_sol = rule.buy_amount_sol();
        max_concurrent = rule.max_concurrent_tokens.max(0) as u32;
        max_total = rule.max_total_tokens.max(0) as u32;
        match RuleParams::parse(&rule.params) {
            Ok(p) => incumbent_params = Some(p),
            Err(e) => {
                let msg = format!("incumbent rule params are invalid: {e}");
                gate.error = Some(msg.clone());
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
                return;
            }
        }
    }

    // 202 after cheap admission so the HTTP request is not held for the lake
    // load. Empty-corpus / load failures land on `rule_search_finished`.
    let _ = early_tx.send(HttpResponse::Accepted().json(serde_json::json!({
        "run_id": run_id,
        "status": "started",
    })));
    // Freeze deadness "now" at session open so every combo shares one horizon.
    let as_of = chrono::Utc::now();

    let mut sel = Selection {
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
        // Only family search reads the oracle curve; every other run pays zero.
        with_oracle: false,
    };

    let _ = state.sse_tx.send(SseEvent::RuleSearchProgress {
        run_id,
        phase: "corpus".into(),
        processed: 0,
        total: 0,
    });

    let src = LakeSource::new(crate::lake::lake_root());
    let (mints, scope_capped) = match src.matching_mints(&sel, engine_fp.clone()).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("rule-search: fingerprint mint scan failed: {e}");
            gate.error = Some(e.to_string());
            return;
        }
    };
    if mints.is_empty() {
        gate.error = Some(format!(
            "no tokens match fingerprint “{}” — widen the range or cap",
            fp_row.name
        ));
        return;
    }
    sel.mints = Some(mints);

    let corpus = match src.load(&sel).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("rule-search: lake load failed: {e}");
            gate.error = Some(e.to_string());
            return;
        }
    };

    if corpus.tokens.is_empty() {
        gate.error = Some("no tokens in that date range — widen the selection".into());
        return;
    }
    if corpus.candidates_capped || scope_capped {
        let _ = state.sse_tx.send(SseEvent::RuleSearchNotice {
            run_id,
            message: format!(
                "Corpus hit the {token_cap}-token cap — only the newest {token_cap} tokens were scored."
            ),
        });
    }

    let settings = state.core.settings();
    let dupe_hours = match settings.duplicate_identity_window_hours {
        0 => hunter_engine::dupe_guard::DEFAULT_WINDOW_HOURS,
        h => h,
    };
    let fill_model = b.fill_model;
    let cost = b.cost_model.model();
    let skip_dupe = b.skip_duplicate_identity;

    let observer = Arc::new(SearchObserver {
        sse_tx: state.sse_tx.clone(),
        run_id,
        cancel: state.rule_search_cancel.clone(),
        cell: state.rule_search_progress.clone(),
        total: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        phase: std::sync::Mutex::new("corpus".into()),
    });

    let threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(1);
    let corpus = Arc::new(corpus);
    let result = tokio::task::spawn_blocking({
        let observer = observer.clone();
        let corpus = corpus.clone();
        let flow = flow.clone();
        move || -> anyhow::Result<crate::rule_search::report::Report> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("rule-search-{i}"))
                .stack_size(8 * 1024 * 1024)
                .build()?;
            pool.install(|| {
                let pricing = Pricing {
                    buy_amount_sol,
                    fill_model,
                    cost,
                };
                run_search(
                    SearchInput {
                        corpus: &corpus,
                        fp: &engine_fp,
                        pricing,
                        as_of,
                        flow: flow.as_ref(),
                        skip_duplicate_identity: skip_dupe,
                        duplicate_identity_window_hours: dupe_hours,
                        max_concurrent_tokens: max_concurrent,
                        max_total_tokens: max_total,
                        incumbent: incumbent_params,
                        fill_authority: fill_model,
                        fill_optimistic: FillModel::FirstInWindow,
                        cost,
                    },
                    observer.as_ref(),
                )
            })
        }
    })
    .await;

    match result {
        Ok(Ok(report)) => {
            state.rule_search_result.store(run_id, report).await;
        }
        Ok(Err(e)) => {
            if !state.rule_search_cancel.load(Ordering::Acquire) {
                gate.error = Some(e.to_string());
                tracing::error!("rule-search failed: {e}");
            }
        }
        Err(e) => {
            gate.error = Some(format!("rule-search task join failed: {e}"));
        }
    }
}
