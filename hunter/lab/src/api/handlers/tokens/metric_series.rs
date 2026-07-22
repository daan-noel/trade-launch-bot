//! Metric-series endpoint (plan 5.7) — replay one token's trades through the
//! engine's [`MetricSeries`](hunter_engine::metrics::series) on demand, returning
//! the value of every metric at every trade event for chart panes. Metrics are
//! **never persisted**; this recomputes them from the sealed lake + PG tail using
//! the *same* compute the live engine + sweep use, so the overlay can never drift
//! from a decision.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{group_spec, metric_spec, MetricGroupId, MetricKind, REGISTRY};

use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;

use crate::state::local_state::LocalState;
use crate::strategies::sim_fetch::fetch_full_history_one_opts;
use crate::sweep::projection::to_trade_lite;

/// Query for the metric-series read: which trailing windows to compute the dynamic
/// metrics for, whether to drop AMM legs, and optional fingerprint context for
/// flow columns.
#[derive(Debug, Deserialize)]
pub struct MetricSeriesQuery {
    /// Comma-separated `window_size_sec` list for dynamic metrics (e.g. `10,30,60`).
    /// Omitted ⇒ a sensible default set.
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub curve_only: bool,
    /// Fingerprint whose `metric_config` supplies volume-ix patterns. When set,
    /// `m_flow_*` columns are included; when absent they are omitted (NaN without
    /// a pattern context).
    #[serde(default)]
    pub fingerprint_id: Option<String>,
}

/// One computed series in the response.
#[derive(serde::Serialize)]
struct SeriesOut {
    metric: &'static str,
    group: &'static str,
    unit: &'static str,
    /// Present only for dynamic metrics.
    window_size_sec: Option<f64>,
    /// One value per event (aligned with `at`); non-finite values serialize `null`.
    values: Vec<Option<f64>>,
}

/// Default dynamic windows when the caller doesn't specify any.
const DEFAULT_WINDOWS: &[f64] = &[10.0, 30.0, 60.0];

/// `GET /api/tokens/{mint}/metric-series?windows=10,30,60&curve_only=false` — every
/// metric's value at every trade of the token, as parallel arrays (plus per-event
/// spot `price` for chart entry/exit markers).
pub async fn token_metric_series(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<String>,
    query: web::Query<MetricSeriesQuery>,
) -> impl Responder {
    let mint = path.into_inner();
    let windows = parse_windows(query.windows.as_deref());

    let flow_ctx = match resolve_flow_ctx(&state, query.fingerprint_id.as_deref()).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let with_flow = flow_ctx.is_some();

    let trades =
        match fetch_full_history_one_opts(&state.trade_repo(), &mint, query.curve_only, with_flow)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("metric-series trade fetch failed for {mint}: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": "trade fetch failed" }));
            }
        };

    if trades.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!({
            "mint_address": mint, "at": [], "price": [], "series": [],
        }));
    }

    let result = web::block(move || build_series(&mint, &trades, &windows, flow_ctx.as_ref())).await;
    match result {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            tracing::error!("metric-series compute task panicked: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "metric-series compute failed" }))
        }
    }
}

struct FlowCtx {
    fp_id: FingerprintId,
    patterns: FlowPatterns,
}

async fn resolve_flow_ctx(
    state: &LocalState,
    fingerprint_id: Option<&str>,
) -> Result<Option<FlowCtx>, HttpResponse> {
    let Some(raw) = fingerprint_id.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let id = Uuid::parse_str(raw).map_err(|_| {
        HttpResponse::BadRequest().json(serde_json::json!({ "error": "invalid fingerprint_id" }))
    })?;
    let repo = FingerprintRepo::new(state.core.db.clone());
    let fp = repo.find(id).await.map_err(|e| {
        tracing::error!("metric-series fingerprint lookup failed: {e}");
        HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": "fingerprint lookup failed" }))
    })?;
    let Some(fp) = fp else {
        return Err(HttpResponse::NotFound()
            .json(serde_json::json!({ "error": "fingerprint not found" })));
    };
    let engine_fp = fp_to_engine(&fp);
    let Some(patterns) = FlowPatterns::from_metric_config(&engine_fp.metric_config) else {
        // Fingerprint present but unconfigured — omit flow columns (same as no id).
        return Ok(None);
    };
    Ok(Some(FlowCtx {
        fp_id: engine_fp.id,
        patterns,
    }))
}

/// Parse the `windows` CSV into a deduped, positive, finite list; fall back to the
/// default set when absent/empty.
fn parse_windows(raw: Option<&str>) -> Vec<f64> {
    let mut ws: Vec<f64> = raw
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .filter(|w| w.is_finite() && *w > 0.0)
        .collect();
    ws.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ws.dedup();
    if ws.is_empty() {
        DEFAULT_WINDOWS.to_vec()
    } else {
        ws
    }
}

/// Build every metric's series over the token's trades. Creation time anchors at
/// the first trade (the dev-buy slot), matching the replay driver's clock.
fn build_series(
    mint: &str,
    trades: &[crate::sweep::projection::CorpusTrade],
    windows: &[f64],
    flow: Option<&FlowCtx>,
) -> serde_json::Value {
    let mut columns: Vec<SeriesColumn> = Vec::new();
    let mut labels: Vec<(SeriesColumn, Option<f64>)> = Vec::new();
    for group in REGISTRY {
        // Flow groups need a fingerprint pattern context — skip when absent.
        let is_flow_group =
            matches!(group.id, MetricGroupId::FlowSplit | MetricGroupId::FlowSplitWindow);
        if is_flow_group && flow.is_none() {
            continue;
        }
        for m in group.metrics {
            match group.kind {
                MetricKind::Static if is_flow_group => {
                    let fp = flow.unwrap().fp_id;
                    let col = SeriesColumn::Flow(m.id, None, fp);
                    columns.push(col);
                    labels.push((col, None));
                }
                MetricKind::Static => {
                    columns.push(SeriesColumn::Static(m.id));
                    labels.push((SeriesColumn::Static(m.id), None));
                }
                MetricKind::Dynamic if is_flow_group => {
                    let fp = flow.unwrap().fp_id;
                    for &w in windows {
                        let col = SeriesColumn::Flow(m.id, Some(w), fp);
                        columns.push(col);
                        labels.push((col, Some(w)));
                    }
                }
                MetricKind::Dynamic => {
                    for &w in windows {
                        columns.push(SeriesColumn::Window(m.id, w));
                        labels.push((SeriesColumn::Window(m.id, w), Some(w)));
                    }
                }
            }
        }
    }

    let created_at = trades[0].block_time;
    let mut series = MetricSeries::new(created_at, columns);
    if let Some(ctx) = flow {
        series.ensure_flow(ctx.fp_id, &ctx.patterns, windows);
    }
    for t in trades {
        series.push_trade(to_trade_lite(t));
    }

    let out: Vec<SeriesOut> = labels
        .iter()
        .filter_map(|(col, window)| {
            let id = match col {
                SeriesColumn::Static(id) => *id,
                SeriesColumn::Window(id, _) => *id,
                SeriesColumn::Flow(id, _, _) => *id,
            };
            let values = series.column_values(*col)?;
            Some(SeriesOut {
                metric: metric_spec(id).name,
                group: group_for(id),
                unit: metric_spec(id).unit.as_str(),
                window_size_sec: *window,
                values: values.into_iter().map(|v| v.is_finite().then_some(v)).collect(),
            })
        })
        .collect();

    let price: Vec<Option<f64>> = series
        .price
        .iter()
        .map(|p| p.is_finite().then_some(*p))
        .collect();

    serde_json::json!({
        "mint_address": mint,
        "at": series.at,
        "price": price,
        "series": out,
    })
}

/// The group name owning a metric id (walks the registry).
fn group_for(id: hunter_engine::metrics::MetricId) -> &'static str {
    for group in REGISTRY {
        if group.metrics.iter().any(|m| m.id == id) {
            return group_spec(group.id).name;
        }
    }
    "unknown"
}
