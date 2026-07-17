//! Metric-series endpoint (plan 5.7) — replay one token's trades through the
//! engine's [`MetricSeries`](hunter_engine::metrics::series) on demand, returning
//! the value of every metric at every trade event for chart panes. Metrics are
//! **never persisted**; this recomputes them from the sealed lake + PG tail using
//! the *same* compute the live engine + sweep use, so the overlay can never drift
//! from a decision.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{group_spec, metric_spec, MetricKind, Side, TradeLite, REGISTRY};

use crate::state::local_state::LocalState;
use crate::strategies::sim_fetch::fetch_full_history_one;

/// Query for the metric-series read: which trailing windows to compute the dynamic
/// (`m_time_window`) metrics for, and whether to drop AMM legs.
#[derive(Debug, Deserialize)]
pub struct MetricSeriesQuery {
    /// Comma-separated `window_size_sec` list for dynamic metrics (e.g. `10,30,60`).
    /// Omitted ⇒ a sensible default set.
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub curve_only: bool,
    /// Optional fingerprint context. Reserved for flow-metric columns that need a
    /// pattern config (`metric_config`); today the series is pure trade replay and
    /// this id is accepted but unused so the FE can wire it ahead of that work.
    #[serde(default)]
    pub fingerprint_id: Option<String>,
}

/// One computed series in the response.
#[derive(serde::Serialize)]
struct SeriesOut {
    metric: &'static str,
    group: &'static str,
    unit: &'static str,
    /// Present only for dynamic (`m_time_window`) metrics.
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
    let _fingerprint_id = query.fingerprint_id.as_deref(); // reserved — see query struct

    let trades = match fetch_full_history_one(&state.trade_repo(), &mint, query.curve_only).await {
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

    let result = web::block(move || build_series(&mint, &trades, &windows)).await;
    match result {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            tracing::error!("metric-series compute task panicked: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "metric-series compute failed" }))
        }
    }
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
fn build_series(mint: &str, trades: &[crate::sweep::projection::CorpusTrade], windows: &[f64]) -> serde_json::Value {
    // Columns: every static metric once + every dynamic metric per requested window.
    let mut columns: Vec<SeriesColumn> = Vec::new();
    let mut labels: Vec<(SeriesColumn, Option<f64>)> = Vec::new();
    for group in REGISTRY {
        for m in group.metrics {
            match group.kind {
                MetricKind::Static => {
                    columns.push(SeriesColumn::Static(m.id));
                    labels.push((SeriesColumn::Static(m.id), None));
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
    for t in trades {
        series.push_trade(TradeLite {
            side: if t.is_buy { Side::Buy } else { Side::Sell },
            sol: t.amount_sol,
            price: t.price_per_token,
            reserve_sol: t.real_reserve_sol.unwrap_or(f64::NAN),
            at: t.block_time,
        });
    }

    let out: Vec<SeriesOut> = labels
        .iter()
        .filter_map(|(col, window)| {
            let id = match col {
                SeriesColumn::Static(id) => *id,
                SeriesColumn::Window(id, _) => *id,
            };
            let values = series.column_values(*col)?;
            Some(SeriesOut {
                metric: metric_spec(id).name,
                group: group_for(id),
                unit: metric_spec(id).unit.as_str(),
                window_size_sec: *window,
                // Non-finite (NaN pre-first-trade) → JSON null so the chart skips it.
                values: values.into_iter().map(|v| v.is_finite().then_some(v)).collect(),
            })
        })
        .collect();

    // Non-finite prices (pre-first-trade) → null, matching metric `values`.
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
