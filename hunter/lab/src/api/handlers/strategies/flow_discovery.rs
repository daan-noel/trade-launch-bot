//! Flow-discovery job — score trade ix-structures per fingerprint group so the
//! user can toggle `volume_ix_patterns` on a fingerprint.
//!
//! See `hunter/docs/roadmap/volume-flow-split-plan.md` §7 / V4.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::api::handlers::strategies::grouped_sweep::{
    fingerprint_from_group_key, matches_field_filter,
};
use crate::lake::duck::LakeSource;
use crate::models::ingest::SseEvent;
use crate::state::local_state::LocalState;
use crate::strategies::flow_discovery::{
    score_corpus, Cancelled, DiscoveryConfig, DEFAULT_MIN_TOKENS,
};
use crate::sweep::corpus::{CorpusSource, Selection};
use crate::sweep::grouping::{normalize_label_vec, GroupField, SOL_BUCKET_WIDTH};
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;

// ── Request bodies ───────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct StartFlowDiscoveryBody {
    #[serde(default)]
    pub created_after: Option<DateTime<Utc>>,
    #[serde(default)]
    pub created_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub curve_only: bool,
    #[serde(default)]
    pub group_by: Vec<GroupField>,
    #[serde(default = "default_bucket_width_sol")]
    pub bucket_width_sol: f64,
    #[serde(default)]
    pub ix_labels_filter: Option<Vec<String>>,
    #[serde(default = "default_min_tokens")]
    pub min_tokens: usize,
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
    #[serde(default)]
    pub field_filters: Option<std::collections::HashMap<String, Vec<serde_json::Value>>>,
    /// When set, keep only tokens that **fully match** this saved fingerprint
    /// (same SSOT as live arming — bucket axes included). Skips the looser
    /// `field_filters` / `ix_labels_filter` path so discovery scores the FP's
    /// real token group.
    #[serde(default)]
    pub fingerprint_id: Option<Uuid>,
}

fn default_bucket_width_sol() -> f64 {
    SOL_BUCKET_WIDTH
}
fn default_min_tokens() -> usize {
    DEFAULT_MIN_TOKENS
}
fn default_token_cap() -> usize {
    10_000
}

#[derive(serde::Deserialize)]
pub struct BindFlowDiscoveryBody {
    pub group_key: serde_json::Value,
    #[serde(default = "default_bucket_width_sol")]
    pub bucket_width_sol: f64,
    pub volume_ix_patterns: Vec<Vec<String>>,
    #[serde(default)]
    pub name: Option<String>,
}

// ── Gate ─────────────────────────────────────────────────────────────────────

struct DiscoveryGate {
    running: Arc<std::sync::atomic::AtomicBool>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress: Arc<crate::state::job_progress::ProgressCell>,
    sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
    run_id: Uuid,
    error: Option<String>,
}

impl Drop for DiscoveryGate {
    fn drop(&mut self) {
        self.progress.reset();
        let cancelled = self.cancel.load(Ordering::Acquire);
        let _ = self.sse_tx.send(SseEvent::FlowDiscoveryFinished {
            run_id: self.run_id,
            cancelled,
            error: self.error.clone(),
        });
        self.running.store(false, Ordering::Release);
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /api/strategies/flow-discovery` → `202 { run_id, status }`.
pub async fn start_flow_discovery(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<StartFlowDiscoveryBody>,
) -> impl Responder {
    if state.sweep_running.load(Ordering::Acquire) {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "a sweep is already running — wait or cancel it before discovery"
        }));
    }
    if state
        .discovery_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "a flow-discovery job is already running"
        }));
    }

    let b = body.into_inner();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_flow_discovery_job(state.clone(), b, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "discovery job dropped before admission" }))
    })
}

/// `POST /api/strategies/flow-discovery/cancel`
pub async fn cancel_flow_discovery(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let cancelling = if state.discovery_running.load(Ordering::Acquire) {
        state.discovery_cancel.store(true, Ordering::Release);
        true
    } else {
        false
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

/// `GET /api/strategies/flow-discovery/{run_id}`
pub async fn get_flow_discovery(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let run_id = path.into_inner();
    let guard = state.discovery_result.read().await;
    match guard.as_ref() {
        Some((id, result)) if *id == run_id => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "groups": result.groups,
        })),
        _ => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no discovery result for that run_id (still running, expired, or unknown)"
        })),
    }
}

/// `POST /api/strategies/flow-discovery/bind` — find-or-create fingerprint from a
/// group key and patch `metric_config` (promote-style; plan §7.0.4).
pub async fn bind_flow_discovery(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<BindFlowDiscoveryBody>,
) -> impl Responder {
    let b = body.into_inner();
    let name = b.name.unwrap_or_else(|| "flow-discovery bind".to_string());
    let metric_config = serde_json::json!({
        "m_flow_split": { "volume_ix_patterns": b.volume_ix_patterns }
    });
    if let Err(e) =
        hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(&metric_config)
    {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }

    let mut draft = fingerprint_from_group_key(&b.group_key, b.bucket_width_sol, name);
    draft.metric_config = metric_config.clone();
    if !draft.has_any_criterion() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "group_key has no fingerprint identity axes — pick cu_limit/ix_labels/… or update an existing fingerprint via PUT"
        }));
    }

    let repo = FingerprintRepo::new(state.db.clone());
    let mut fp = match repo.find_or_create(&draft).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("flow-discovery bind: find_or_create failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }));
        }
    };
    // Identity ignores metric_config — patch when the stored row differs.
    if fp.metric_config != metric_config {
        fp.metric_config = metric_config;
        fp.updated_at = Utc::now();
        if let Err(e) = repo.update(&fp).await {
            tracing::error!("flow-discovery bind: update metric_config failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "database error" }));
        }
    }
    HttpResponse::Ok().json(fp)
}

// ── Job ──────────────────────────────────────────────────────────────────────

async fn run_flow_discovery_job(
    state: web::Data<Arc<LocalState>>,
    b: StartFlowDiscoveryBody,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
    let run_id = Uuid::new_v4();
    let mut gate = DiscoveryGate {
        running: state.discovery_running.clone(),
        cancel: state.discovery_cancel.clone(),
        progress: state.discovery_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        run_id,
        error: None,
    };
    state.discovery_cancel.store(false, Ordering::Release);
    state.discovery_progress.reset();

    let token_cap = b.token_cap.max(1);
    let sel = Selection {
        mints: None,
        token_cap,
        created_after: b.created_after,
        created_before: b.created_before,
        per_mint_cap: crate::sweep::corpus::sweep_per_mint_cap(),
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: b.curve_only,
        with_signatures: false,
        with_flow: true,
    };

    let _ = state.sse_tx.send(SseEvent::FlowDiscoveryProgress {
        run_id,
        phase: "corpus".into(),
        processed: 0,
        total: 0,
    });

    let root = crate::lake::lake_root();
    let src = LakeSource::new(root.clone());
    let mut corpus = match src.load(&sel).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("flow-discovery: lake load failed: {e}");
            gate.error = Some(e.to_string());
            let _ = early_tx.send(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            })));
            return;
        }
    };

    if state.discovery_cancel.load(Ordering::Acquire) {
        let _ = early_tx.send(HttpResponse::Ok().json(serde_json::json!({
            "run_id": null,
            "status": "cancelled"
        })));
        return;
    }

    if corpus.tokens.is_empty() {
        let msg = "no tokens in that date range — widen the selection";
        gate.error = Some(msg.into());
        let _ = early_tx.send(
            HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
        );
        return;
    }

    if corpus.tokens.len() >= token_cap {
        let msg = format!(
            "Corpus hit the {token_cap}-token cap — only the newest {token_cap} tokens were scored."
        );
        let _ = state.sse_tx.send(SseEvent::FlowDiscoveryNotice {
            run_id,
            message: msg,
        });
    }

    if let Some(fp_id) = b.fingerprint_id {
        // Saved-fingerprint path: engine match SSOT (exact + bucket axes).
        let repo = FingerprintRepo::new(state.db.clone());
        let fp = match repo.find(fp_id).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                let msg = format!("fingerprint {fp_id} not found");
                gate.error = Some(msg.clone());
                let _ = early_tx.send(
                    HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
                );
                return;
            }
            Err(e) => {
                tracing::error!("flow-discovery: fingerprint lookup failed: {e}");
                gate.error = Some(e.to_string());
                let _ = early_tx.send(HttpResponse::InternalServerError().json(
                    serde_json::json!({ "error": "database error" }),
                ));
                return;
            }
        };
        let engine_fp = fp_to_engine(&fp);
        corpus
            .tokens
            .retain(|t| hunter_engine::fingerprint::matches(&engine_fp, &t.fp));
        if corpus.tokens.is_empty() {
            let msg = format!(
                "no tokens in that window match fingerprint “{}” — widen the date range or token cap",
                fp.name
            );
            gate.error = Some(msg.clone());
            let _ = early_tx.send(
                HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
            );
            return;
        }
    } else {
        // Exact-set ix_labels filter (token-level fingerprint labels).
        if let Some(want) = b
            .ix_labels_filter
            .as_ref()
            .filter(|f| !f.is_empty())
            .map(|f| normalize_label_vec(f.clone()))
        {
            corpus.tokens.retain(|t| t.fp.ix_labels == want);
            if corpus.tokens.is_empty() {
                let msg = "no tokens match that instruction-label filter";
                gate.error = Some(msg.into());
                let _ = early_tx.send(
                    HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
                );
                return;
            }
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
                corpus
                    .tokens
                    .retain(|t| matches_field_filter(&t.fp, field, allowed));
                if corpus.tokens.is_empty() {
                    let msg = format!("no tokens match the '{field_str}' field filter");
                    gate.error = Some(msg.clone());
                    let _ = early_tx.send(
                        HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
                    );
                    return;
                }
            }
        }
    }

    // Admit — POST returns here.
    let _ = early_tx.send(HttpResponse::Accepted().json(serde_json::json!({
        "run_id": run_id,
        "status": "started",
    })));

    let n = corpus.tokens.len() as u64;
    state.discovery_progress.set_total(n);
    let _ = state.sse_tx.send(SseEvent::FlowDiscoveryProgress {
        run_id,
        phase: "score".into(),
        processed: 0,
        total: n,
    });

    let cfg = DiscoveryConfig {
        group_by: b.group_by,
        bucket_width_sol: b.bucket_width_sol,
        min_tokens: b.min_tokens.max(1),
        ..DiscoveryConfig::default()
    };

    // Score on a blocking thread — CPU-bound fold.
    let cancel = state.discovery_cancel.clone();
    let progress = state.discovery_progress.clone();
    let sse_tx = state.sse_tx.clone();
    let result = tokio::task::spawn_blocking(move || {
        // Lightweight progress: poll cancel inside score_corpus; emit a few frames
        // by wrapping isn't available — score_corpus polls cancel. We set processed
        // to total on completion.
        let out = score_corpus(&corpus, &cfg, Some(&cancel));
        if out.is_ok() {
            progress.set_processed(n);
            let _ = sse_tx.send(SseEvent::FlowDiscoveryProgress {
                run_id,
                phase: "score".into(),
                processed: n,
                total: n,
            });
        }
        out
    })
    .await;

    match result {
        Ok(Ok(discovery)) => {
            store_result(&state, run_id, discovery).await;
        }
        Ok(Err(Cancelled)) => {
            // Gate Drop emits cancelled Finished.
        }
        Err(e) => {
            gate.error = Some(format!("discovery task join failed: {e}"));
        }
    }
}

async fn store_result(state: &LocalState, run_id: Uuid, result: crate::strategies::flow_discovery::DiscoveryResult) {
    let mut guard = state.discovery_result.write().await;
    *guard = Some((run_id, result));
}
