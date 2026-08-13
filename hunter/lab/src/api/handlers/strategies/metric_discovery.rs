//! Metric-combo **discovery pipeline** endpoint (lab only) — runs the automated
//! screen → family-grid → validate flow for a token cohort and returns the ranked
//! shortlist, family winners, and out-of-sample verdicts as one JSON tree.
//!
//! Mirrors [`flow_discovery`](super::flow_discovery)'s job shape: single-flight gate,
//! detached job with an early admission reply, cooperative cancel, SSE progress, and
//! a last-result slot the page rehydrates from. Everything downstream of the lake
//! load reuses the discovery library (`crate::discovery`) — this handler is just the
//! corpus-scoping + orchestration edge.
//!
//! Architecture: `hunter/docs/arch/sweep.md` "Metric-combo discovery pipeline".

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::api::handlers::strategies::grouped_sweep::matches_field_filter;
use crate::discovery::baseline::BaselineGrid;
use crate::discovery::dto::PipelineDto;
use crate::discovery::objective::DiscoveryWeights;
use crate::discovery::pipeline::{run_pipeline, PipelineConfig};
use crate::discovery::screen::ScreenBaseline;
use crate::discovery::validate::{SplitPolicy, ValidationThresholds};
use crate::discovery::candidates::ScreenConfig;
use crate::lake::duck::LakeSource;
use crate::models::ingest::SseEvent;
use crate::state::job_progress::ProgressCell;
use crate::state::local_state::LocalState;
use crate::sweep::corpus::{sweep_per_mint_cap, CorpusSource, Selection};
use crate::sweep::grouping::{normalize_label_vec, GroupField};
use crate::sweep::progress::SweepObserver;
use hunter_engine::metrics::flow_split::FlowPatterns;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;

// ── Request body ───────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct StartMetricDiscoveryBody {
    #[serde(default)]
    pub created_after: Option<DateTime<Utc>>,
    #[serde(default)]
    pub created_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub curve_only: bool,
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
    /// Keep only tokens the engine matches against this saved fingerprint (exact +
    /// bucket axes, the live-arming SSOT). Recommended: scope to one regime.
    #[serde(default)]
    pub fingerprint_id: Option<Uuid>,
    /// Exact-set instruction-label filter (used when no `fingerprint_id`).
    #[serde(default)]
    pub ix_labels_filter: Option<Vec<String>>,
    /// Per-field value filters (used when no `fingerprint_id`), same shape as the
    /// grouped sweep's.
    #[serde(default)]
    pub field_filters: Option<std::collections::HashMap<String, Vec<serde_json::Value>>>,
    /// Notional (SOL) every simulated round-trip is priced at.
    #[serde(default = "default_buy_amount_sol")]
    pub buy_amount_sol: f64,
    /// The fixed take-profit % the whole screen runs against (`null` ⇒ no TP guard —
    /// but at least one of TP/SL must be set). Ignored when a bracket menu below
    /// resolves to more than one candidate.
    #[serde(default = "default_take_profit")]
    pub take_profit_pct: Option<f64>,
    /// The fixed stop-loss % the whole screen runs against.
    #[serde(default = "default_stop_loss")]
    pub stop_loss_pct: Option<f64>,
    /// Candidate take-profit rungs for baseline selection. With the stop-loss menu
    /// these cross into the bracket grid Layer 0 measures; a `null` entry means "no
    /// take-profit guard". Absent/single ⇒ no selection pass, `take_profit_pct` stands.
    #[serde(default)]
    pub take_profit_menu: Option<Vec<Option<f64>>>,
    /// Candidate stop-loss rungs — see [`Self::take_profit_menu`].
    #[serde(default)]
    pub stop_loss_menu: Option<Vec<Option<f64>>>,
    /// Minimum closed trades for a combo to be rankable (the anti-overfit gate).
    #[serde(default = "default_min_closed")]
    pub min_closed: u64,
    /// Train fraction for the out-of-sample split (earliest tokens train). `0.7` =
    /// 70/30. Clamped to `[0,1]`.
    #[serde(default = "default_split_fraction")]
    pub split_fraction: f64,
    /// Entry-side window (s) every dynamic metric is screened at.
    #[serde(default = "default_entry_window")]
    pub entry_window_sec: f64,
    /// Exit-side window (s) every dynamic metric is screened at.
    #[serde(default = "default_exit_window")]
    pub exit_window_sec: f64,
    /// The run's `volume_ix_patterns` — enables the flow-split metrics (skipped
    /// without them). Absent ⇒ no flow gating.
    #[serde(default)]
    pub volume_ix_patterns: Option<Vec<Vec<String>>>,
}

fn default_token_cap() -> usize {
    crate::sweep::registry::DEFAULT_TOKEN_CAP
}
fn default_buy_amount_sol() -> f64 {
    crate::sweep::registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL
}
fn default_take_profit() -> Option<f64> {
    Some(30.0)
}
fn default_stop_loss() -> Option<f64> {
    Some(15.0)
}
fn default_min_closed() -> u64 {
    DiscoveryWeights::default().min_closed
}
fn default_split_fraction() -> f64 {
    0.7
}
fn default_entry_window() -> f64 {
    ScreenConfig::default().entry_window_sec
}
fn default_exit_window() -> f64 {
    ScreenConfig::default().exit_window_sec
}

// ── Progress observer ──────────────────────────────────────────────────────

/// A [`SweepObserver`] that broadcasts [`SseEvent::MetricDiscoveryProgress`] and
/// carries the pipeline's shared cancel flag into the engine. The pipeline
/// re-declares the total at each layer (screen / family / validate), so the bar
/// restarts per layer — the phase label rides the `phase` field so the client can
/// say which layer is running.
struct PipelineObserver {
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    run_id: Uuid,
    cancel: Arc<AtomicBool>,
    cell: Arc<ProgressCell>,
    total: AtomicUsize,
    done: AtomicUsize,
    phase: std::sync::Mutex<String>,
}

impl PipelineObserver {
    fn set_phase(&self, phase: &str) {
        if let Ok(mut g) = self.phase.lock() {
            *g = phase.to_string();
        }
    }

    fn phase(&self) -> String {
        self.phase.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn emit(&self, processed: usize, total: usize) {
        let _ = self.sse_tx.send(SseEvent::MetricDiscoveryProgress {
            run_id: self.run_id,
            phase: self.phase(),
            processed: processed as u64,
            total: total as u64,
        });
    }
}

impl SweepObserver for PipelineObserver {
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
        // Throttle to ~100 frames per layer, always emitting the final one.
        let step = (total / 100).max(1);
        if n >= total || (prev / step) != (n / step) {
            self.cell.set_processed((n / total.max(1)) as u64);
            self.emit(n, total);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn notice(&self, message: &str) {
        let _ = self.sse_tx.send(SseEvent::MetricDiscoveryNotice {
            run_id: self.run_id,
            message: message.to_string(),
        });
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
        let _ = self.sse_tx.send(SseEvent::MetricDiscoveryFinished {
            run_id: self.run_id,
            cancelled,
            error: self.error.clone(),
        });
        self.running.store(false, Ordering::Release);
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// `POST /api/strategies/metric-discovery` → `202 { run_id, status }`.
pub async fn start_metric_discovery(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<StartMetricDiscoveryBody>,
) -> impl Responder {
    if let Err(msg) = state.claim_heavy(crate::state::local_state::HeavyJob::MetricDiscovery) {
        return HttpResponse::Conflict().json(serde_json::json!({ "error": msg }));
    }

    let b = body.into_inner();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_job(state.clone(), b, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "pipeline job dropped before admission" }))
    })
}

/// `POST /api/strategies/metric-discovery/cancel`
pub async fn cancel_metric_discovery(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let cancelling = if state.metric_discovery_running.load(Ordering::Acquire) {
        state.metric_discovery_cancel.store(true, Ordering::Release);
        true
    } else {
        false
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

/// `GET /api/strategies/metric-discovery/{run_id}`
pub async fn get_metric_discovery(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let run_id = path.into_inner();
    match &*state.metric_discovery_result.read().await {
        Some((id, result)) if *id == run_id => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "result": result,
        })),
        _ => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no pipeline result for that run_id (still running, superseded, or unknown)"
        })),
    }
}

/// `GET /api/strategies/metric-discovery/last` — whatever result is cached, for
/// rehydrating the page after a reload.
pub async fn get_last_metric_discovery(state: web::Data<Arc<LocalState>>) -> impl Responder {
    match &*state.metric_discovery_result.read().await {
        Some((run_id, result)) => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "result": result,
        })),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no cached pipeline result"
        })),
    }
}

// ── Job ────────────────────────────────────────────────────────────────────

async fn run_job(
    state: web::Data<Arc<LocalState>>,
    b: StartMetricDiscoveryBody,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
    let run_id = Uuid::new_v4();
    let mut gate = Gate {
        running: state.metric_discovery_running.clone(),
        cancel: state.metric_discovery_cancel.clone(),
        progress: state.metric_discovery_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        run_id,
        error: None,
    };
    state.metric_discovery_cancel.store(false, Ordering::Release);
    state.metric_discovery_progress.reset();

    // Validate the baseline up front so a bad TP/SL fails the request, not the job.
    // A bracket menu (Layer 0) supersedes the single baseline; either way the run must
    // end up with at least one bracket carrying a guard.
    let baseline = ScreenBaseline { take_profit_pct: b.take_profit_pct, stop_loss_pct: b.stop_loss_pct };
    let baseline_grid = match (&b.take_profit_menu, &b.stop_loss_menu) {
        (None, None) => None,
        (tp, sl) => Some(BaselineGrid {
            take_profit_pct: tp.clone().unwrap_or_else(|| vec![baseline.take_profit_pct]),
            stop_loss_pct: sl.clone().unwrap_or_else(|| vec![baseline.stop_loss_pct]),
        }),
    };
    let brackets = baseline_grid.as_ref().map_or(1, |g| g.brackets().len());
    if brackets == 0 || (baseline_grid.is_none() && baseline.take_profit_pct.is_none() && baseline.stop_loss_pct.is_none())
    {
        let msg = "at least one of take_profit_pct / stop_loss_pct is required (or a bracket menu carrying one)";
        gate.error = Some(msg.into());
        let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
        return;
    }
    // A one-bracket menu is just a named baseline — use it and skip the selection pass.
    let baseline = baseline_grid
        .as_ref()
        .filter(|g| g.brackets().len() == 1)
        .map_or(baseline, |g| g.brackets()[0]);

    let flow_patterns = b
        .volume_ix_patterns
        .as_ref()
        .map(|p| FlowPatterns::from_label_sequences(p));

    let token_cap = crate::sweep::registry::clamp_token_cap(b.token_cap);
    let with_flow = flow_patterns.is_some();
    let mut sel = Selection {
        mints: None,
        token_cap,
        created_after: b.created_after,
        created_before: b.created_before,
        per_mint_cap: sweep_per_mint_cap(),
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: b.curve_only,
        with_signatures: false,
        with_flow,
        // Hash-resolved flow keys only — no consumer here reads label text.
        with_flow_text: false,
    };

    let _ = state.sse_tx.send(SseEvent::MetricDiscoveryProgress {
        run_id,
        phase: "corpus".into(),
        processed: 0,
        total: 0,
    });

    let root = crate::lake::lake_root();
    let src = LakeSource::new(root);

    // Cohort scoping resolved BEFORE the trade scan — same fix, and the same reason,
    // as flow-discovery: a post-load `retain` pays for the full trade history of every
    // token in the window and then throws away all but the matched ones. See
    // `LakeSource::matching_mints`. `token_cap` consequently bounds matched tokens.
    let mut scope_capped = false;
    if let Some(fp_id) = b.fingerprint_id {
        let repo = FingerprintRepo::new(state.db.clone());
        let fp = match repo.find(fp_id).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                let msg = format!("fingerprint {fp_id} not found");
                gate.error = Some(msg.clone());
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
                return;
            }
            Err(e) => {
                tracing::error!("metric-discovery: fingerprint lookup failed: {e}");
                gate.error = Some(e.to_string());
                let _ = early_tx.send(HttpResponse::InternalServerError().json(
                    serde_json::json!({ "error": "database error" }),
                ));
                return;
            }
        };
        let (mints, capped) = match src.matching_mints(&sel, fp_to_engine(&fp)).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("metric-discovery: fingerprint mint scan failed: {e}");
                gate.error = Some(e.to_string());
                let _ = early_tx.send(HttpResponse::InternalServerError().json(
                    serde_json::json!({ "error": e.to_string() }),
                ));
                return;
            }
        };
        if mints.is_empty() {
            let msg = format!("no tokens match fingerprint “{}” — widen the range or cap", fp.name);
            gate.error = Some(msg.clone());
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
            return;
        }
        scope_capped = capped;
        sel.mints = Some(mints);
    }
    let sel = sel;

    let mut corpus = match src.load(&sel).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("metric-discovery: lake load failed: {e}");
            gate.error = Some(e.to_string());
            let _ = early_tx.send(
                HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() })),
            );
            return;
        }
    };

    if corpus.tokens.is_empty() {
        let msg = "no tokens in that date range — widen the selection";
        gate.error = Some(msg.into());
        let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
        return;
    }
    if corpus.candidates_capped || scope_capped {
        let _ = state.sse_tx.send(SseEvent::MetricDiscoveryNotice {
            run_id,
            message: format!(
                "Corpus hit the {token_cap}-token cap — only the newest {token_cap} tokens were scored."
            ),
        });
    }

    if b.fingerprint_id.is_none() {
        if let Some(want) = b
            .ix_labels_filter
            .as_ref()
            .filter(|f| !f.is_empty())
            .map(|f| normalize_label_vec(f.clone()))
        {
            corpus.tokens.retain(|t| t.fp.ix_labels == want);
        }
        if let Some(filters) = &b.field_filters {
            for (field_str, allowed) in filters {
                if allowed.is_empty() {
                    continue;
                }
                let field: GroupField = match serde_json::from_str(&format!("\"{field_str}\"")) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                if field == GroupField::IxLabels {
                    continue;
                }
                corpus.tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
            }
        }
        if corpus.tokens.is_empty() {
            let msg = "no tokens match that cohort filter";
            gate.error = Some(msg.into());
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
            return;
        }
    }

    // Admit — POST returns here; the fold runs on a blocking thread.
    let _ = early_tx.send(HttpResponse::Accepted().json(serde_json::json!({
        "run_id": run_id,
        "status": "started",
    })));

    let pricing = crate::sweep::generic::Pricing {
        buy_amount_sol: b.buy_amount_sol,
        fill_model: trading_core::strategies::paper_fill::FillModel::default(),
        cost: trading_core::strategies::kernel::CostModel::pumpfun_fee_only(),
    };
    // Deadness "now" = the newest token's creation instant, the same anchor the sweep
    // uses for a run: judging against wall-clock would mark this historical cohort dead.
    let as_of = corpus
        .tokens
        .iter()
        .map(|t| t.created_at)
        .max()
        .unwrap_or_else(Utc::now);

    let cfg = PipelineConfig {
        screen: ScreenConfig {
            entry_window_sec: b.entry_window_sec,
            exit_window_sec: b.exit_window_sec,
            flow_patterns,
            ..ScreenConfig::default()
        },
        baseline,
        baseline_grid: baseline_grid.filter(|g| g.brackets().len() > 1),
        weights: DiscoveryWeights { min_closed: b.min_closed, ..DiscoveryWeights::default() },
        split: SplitPolicy::AgeFraction(b.split_fraction.clamp(0.0, 1.0)),
        validation_thresholds: ValidationThresholds::default(),
        flow_label_sequences: b.volume_ix_patterns.clone(),
        ..PipelineConfig::default()
    };

    let observer = Arc::new(PipelineObserver {
        sse_tx: state.sse_tx.clone(),
        run_id,
        cancel: state.metric_discovery_cancel.clone(),
        cell: state.metric_discovery_progress.clone(),
        total: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        phase: std::sync::Mutex::new("pipeline".into()),
    });

    let threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(1);
    // The handler is the only layer that knows the cap, so it is the only one that can
    // report a truncated cohort. It rides the result rather than the SSE notice alone:
    // a notice is gone by the time someone reads the shortlist, and "the newest N
    // tokens" is not the cohort the run was asked for.
    let cohort_capped = corpus.candidates_capped || scope_capped;
    let corpus = Arc::new(corpus);
    let result = tokio::task::spawn_blocking({
        let observer = observer.clone();
        let corpus = corpus.clone();
        move || -> anyhow::Result<serde_json::Value> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("metric-discovery-{i}"))
                .stack_size(8 * 1024 * 1024)
                .build()?;
            pool.install(|| {
                observer.set_phase("pipeline");
                let report = run_pipeline(&corpus, &cfg, pricing, as_of, observer.as_ref())?;
                let mut dto = PipelineDto::from_report(&report);
                dto.cohort_capped = cohort_capped;
                dto.token_cap = Some(token_cap);
                if cohort_capped {
                    dto.diagnostics.insert(
                        0,
                        format!(
                            "Cohort hit the {token_cap}-token cap — only the newest {token_cap} \
                             matching token(s) were scored, not the whole selected range."
                        ),
                    );
                }
                Ok(serde_json::to_value(&dto)?)
            })
        }
    })
    .await;

    match result {
        Ok(Ok(json)) => {
            *state.metric_discovery_result.write().await = Some((run_id, json));
        }
        Ok(Err(e)) => {
            if !state.metric_discovery_cancel.load(Ordering::Acquire) {
                gate.error = Some(e.to_string());
                tracing::error!("metric-discovery pipeline failed: {e}");
            }
        }
        Err(e) => {
            gate.error = Some(format!("pipeline task join failed: {e}"));
        }
    }
}
