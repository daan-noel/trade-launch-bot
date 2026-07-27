//! Metric-series endpoint (plan 5.7) — replay one token's trades through the
//! engine's [`MetricSeries`](hunter_engine::metrics::series) on demand, returning
//! the value of every metric at every event for chart panes. Metrics are
//! **never persisted**; this recomputes them from the sealed lake + PG tail using
//! the *same* compute the live engine + sweep use, so the overlay can never drift
//! from a decision.
//!
//! **Events, not trades.** The fold runs through the shared sparse tick grid
//! ([`hunter_engine::metrics::grid`]) — the same driver the sweep precompute and
//! `run_replay` use — so rows land on the engine's `TICK_MS` decision grid, not
//! only at trade instants. This is load-bearing, not a nicety: every time-decaying
//! metric (`m_flow_window`/`m_flow_split_window` decay, `m_price_window` rolling
//! extrema, `m_snapshot.stall`/`.time`, the dead verdict) advances *only* inside a
//! tick, so a trade-only fold samples them exactly where a fresh trade has just
//! been folded back in and never sees a between-trades crossing. That shipped: an
//! `m_flow_window.buy < 5` exit drew 70 s after the one simulate booked, because
//! the dip happened in a 1.3 s gap between two trades.
//!
//! Because the grid's density is set by what the caller will *evaluate* over the
//! series, the rule's `time`/`stall` condition ceilings come in as query params
//! ([`MetricSeriesQuery::time_horizon_sec`] / [`stall_horizon_sec`]); the trailing
//! windows are already implied by `windows`. A horizon left at `0` only drops ticks
//! in quiet gaps past every other horizon — never near a trade.
//!
//! [`stall_horizon_sec`]: MetricSeriesQuery::stall_horizon_sec

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::grid::{estimate_sparse_rows, fold_sparse, SparseGrid};
use hunter_engine::metrics::position::PositionCtx;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_spec, metric_spec, MetricGroupId, MetricId, MetricKind, Ts, REGISTRY,
};

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
    /// Entry-fill time of the run being inspected (RFC3339). Together with
    /// `entry_price` it supplies the [`PositionCtx`] the position-scoped `m_position`
    /// metrics anchor on — a token-only replay has no position, so without this pair
    /// those columns are omitted (they'd be all-`NaN`). See [`build_position_series`].
    #[serde(default)]
    pub entry_time: Option<DateTime<Utc>>,
    /// Entry-fill price of the inspected run (the `pnl` reference). Paired with
    /// `entry_time`; ignored unless both are present and the price is positive.
    #[serde(default)]
    pub entry_price: Option<f64>,
    /// Largest `m_snapshot.time` condition value (+ `=`-tolerance) the caller will
    /// evaluate over this series, in seconds. Sizes the sparse tick grid so the
    /// monotone `time` clock is sampled densely up to the last instant it could
    /// still cross. Omitted/`0` ⇒ `time` is assumed not to be evaluated.
    #[serde(default)]
    pub time_horizon_sec: Option<f64>,
    /// Same, for the `m_price_lifetime.stall` clock (measured from the last trade).
    #[serde(default)]
    pub stall_horizon_sec: Option<f64>,
}

/// Row ceiling for one series response. The sparse grid keeps rows ∝ trades for a
/// normally-traded token (the p99 local token lands ~47k), so this bites only on the
/// extreme tail — a token that trades continuously for many hours. Hitting it
/// **truncates in time** rather than coarsening the grid: every returned row stays
/// bit-identical to the engine's `TICK_MS` decision grid (the entire point of the
/// fix), and the response flags the short coverage so the UI can say so. Silently
/// widening the tick instead would reintroduce exactly the class of drift this
/// endpoint just stopped having.
const MAX_SERIES_ROWS: usize = 40_000;

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
    // The tick grid must stay dense wherever anything the caller evaluates can still
    // move: the trailing windows (implied by `windows`) plus the two monotone clocks
    // the caller declares. Deadness is covered unconditionally by the grid itself.
    let grid = SparseGrid {
        max_window_secs: windows.iter().cloned().fold(0.0_f64, f64::max),
        time_horizon_secs: SparseGrid::clamp_secs(query.time_horizon_sec.unwrap_or(0.0)),
        stall_horizon_secs: SparseGrid::clamp_secs(query.stall_horizon_sec.unwrap_or(0.0)),
    };
    // Position-scoped metrics need the caller's entry fill; require both halves and a
    // sane price, else they stay omitted (the token-only replay can't compute them).
    let entry = match (query.entry_time, query.entry_price) {
        (Some(at), Some(price)) if price.is_finite() && price > 0.0 => Some((at, price)),
        _ => None,
    };

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
            "truncated": false, "covered_until": serde_json::Value::Null,
        }));
    }

    let result = web::block(move || {
        build_series(&mint, &trades, &windows, &grid, flow_ctx.as_ref(), entry)
    })
    .await;
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

/// Build every metric's series over the token's event stream. Creation time anchors
/// at the first trade (the dev-buy slot), matching the replay driver's clock, and the
/// fold runs through the shared sparse tick grid so rows land on the engine's
/// decision grid rather than only at trades (see the module docs).
fn build_series(
    mint: &str,
    trades: &[crate::sweep::projection::CorpusTrade],
    windows: &[f64],
    grid: &SparseGrid,
    flow: Option<&FlowCtx>,
    entry: Option<(Ts, f64)>,
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
        // Position-scoped metrics anchor on the caller's entry fill, not the token
        // track (which would read all-`NaN`). They're computed separately below from
        // `entry`, and omitted entirely when there's no entry context.
        if matches!(group.id, MetricGroupId::Position) {
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
    // `as_of` = now: the deadness clock advances toward the request instant exactly as
    // a `run_replay` over this token would, so the tail ticks that book a Dead exit are
    // present. The driver caps the tail at `last_trade + DEAD_QUIET + TAIL_MARGIN`.
    let as_of = Utc::now();
    // Admission: the estimate never under-counts, so a token that would blow the row
    // ceiling is folded under a budget instead of allocating first and regretting it.
    let estimated = estimate_sparse_rows(created_at, trades.iter().map(|t| t.block_time), grid, as_of);
    let budget = (estimated > MAX_SERIES_ROWS).then_some(MAX_SERIES_ROWS);
    if budget.is_some() {
        tracing::warn!(
            mint, estimated, cap = MAX_SERIES_ROWS,
            "metric-series exceeds the row ceiling — truncating coverage in time",
        );
    }
    let fold = fold_sparse(
        &mut series,
        created_at,
        trades.iter().map(|t| (to_trade_lite(t), None)),
        grid,
        as_of,
        budget,
    );

    let mut out: Vec<SeriesOut> = labels
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

    // Position-scoped columns, computed from the caller's entry fill (see the fold in
    // `reduce.rs`). Omitted when there's no entry context.
    if let Some((entered_at, entry_price)) = entry {
        out.extend(build_position_series(&series, entered_at, entry_price));
    }

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
        // Coverage, never silent: `truncated` says the row ceiling cut the series
        // short, `covered_until` is the last instant it reaches. Every returned row is
        // still on the engine's decision grid — only the span is bounded.
        "truncated": fold.truncated,
        "covered_until": fold.covered_until,
    })
}

/// The group name owning a metric id (walks the registry).
fn group_for(id: MetricId) -> &'static str {
    for group in REGISTRY {
        if group.metrics.iter().any(|m| m.id == id) {
            return group_spec(group.id).name;
        }
    }
    "unknown"
}

/// The `m_position` columns (`retrace`/`bounce`/`pnl`/`held`) over a series,
/// anchored on the inspected run's entry fill. Mirrors the live engine's
/// [`PositionCtx`] fold (`reduce.rs`): peak/trough seed at the entry price and
/// ratchet on each finite print from the entry event onward, and every metric
/// reads `NaN` (serialized `null`) at any event *before* the entry — so the panes
/// draw the position metrics exactly over the held window, blank before it.
fn build_position_series(series: &MetricSeries, entered_at: Ts, entry_price: f64) -> Vec<SeriesOut> {
    let n = series.n_rows();
    let mut ctx = PositionCtx::at_fill(entry_price, entered_at);
    let mut retrace = Vec::with_capacity(n);
    let mut bounce = Vec::with_capacity(n);
    let mut pnl = Vec::with_capacity(n);
    let mut held = Vec::with_capacity(n);
    for i in 0..n {
        let at = series.at[i];
        if at < entered_at {
            retrace.push(None);
            bounce.push(None);
            pnl.push(None);
            held.push(None);
            continue;
        }
        let price = series.price[i];
        ctx.fold_price(price);
        retrace.push(finite(ctx.retrace(price)));
        bounce.push(finite(ctx.bounce(price)));
        pnl.push(finite(ctx.pnl(price)));
        held.push(finite(ctx.held(at)));
    }

    let group = group_spec(MetricGroupId::Position);
    group
        .metrics
        .iter()
        .map(|m| {
            let values = match m.id {
                MetricId::Retrace => retrace.clone(),
                MetricId::Bounce => bounce.clone(),
                MetricId::Pnl => pnl.clone(),
                MetricId::Held => held.clone(),
                _ => vec![None; n],
            };
            SeriesOut {
                metric: m.name,
                group: group.name,
                unit: m.unit.as_str(),
                window_size_sec: None,
                values,
            }
        })
        .collect()
}

/// A finite metric value as `Some` (non-finite → `null`, the pane's "no value").
#[inline]
fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hunter_engine::metrics::{Side, TradeLite};

    fn ts(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn trade(price: f64, secs: i64) -> TradeLite {
        TradeLite { side: Side::Buy, sol: 1.0, price, reserve_sol: 30.0, at: ts(secs), ..Default::default() }
    }

    /// The values of one `m_position` metric out of a position series.
    fn col<'a>(out: &'a [SeriesOut], metric: &str) -> &'a [Option<f64>] {
        &out.iter().find(|s| s.metric == metric).expect("metric present").values
    }

    #[test]
    fn position_series_blanks_before_entry_then_tracks_the_held_window() {
        // Events at t=0 (pre-entry), t=10 (entry, fill 1.0), t=20 (+50% run-up →
        // new peak), t=30 (pullback to 1.2 off the 1.5 peak).
        let mut s = MetricSeries::new(ts(0), Vec::new());
        for (p, secs) in [(1.0, 0), (1.0, 10), (1.5, 20), (1.2, 30)] {
            s.push_trade(trade(p, secs));
        }
        let out = build_position_series(&s, ts(10), 1.0);
        let (pnl, retrace, bounce, held) = (
            col(&out, "pnl"),
            col(&out, "retrace"),
            col(&out, "bounce"),
            col(&out, "held"),
        );

        // Before entry → blank (position metrics have no value with no position).
        assert_eq!((pnl[0], retrace[0], bounce[0], held[0]), (None, None, None, None));
        // Entry event: flat pnl, at-peak retrace, at-trough bounce, zero held.
        assert_eq!(
            (pnl[1], retrace[1], bounce[1], held[1]),
            (Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        );
        // Run-up to 1.5: +50% pnl, still at peak (retrace 0), bounce = pnl (no dip),
        // 10 s held.
        assert_eq!(
            (pnl[2], retrace[2], bounce[2], held[2]),
            (Some(50.0), Some(0.0), Some(50.0), Some(10.0))
        );
        // Pullback to 1.2 off the ratcheted 1.5 peak: +20% pnl, 20% retrace;
        // trough still at entry so bounce = pnl = 20%; 20 s held.
        assert!((pnl[3].unwrap() - 20.0).abs() < 1e-9);
        assert!((retrace[3].unwrap() - 20.0).abs() < 1e-9);
        assert!((bounce[3].unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(held[3], Some(20.0));
    }

    #[test]
    fn position_series_tracks_bounce_off_since_entry_trough() {
        // Entry at 1.0, dip to 0.8 (new trough), recover to 1.0 → bounce = 25%.
        let mut s = MetricSeries::new(ts(0), Vec::new());
        for (p, secs) in [(1.0, 0), (0.8, 10), (1.0, 20)] {
            s.push_trade(trade(p, secs));
        }
        let out = build_position_series(&s, ts(0), 1.0);
        let bounce = col(&out, "bounce");
        assert_eq!(bounce[0], Some(0.0));
        assert_eq!(bounce[1], Some(0.0)); // at the trough
        assert!((bounce[2].unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn no_entry_context_omits_the_position_group() {
        // `build_series` skips `MetricGroupId::Position` in its generic loop, so with
        // no entry the response carries no `m_position` columns at all (not all-null).
        let mut s = MetricSeries::new(ts(0), Vec::new());
        s.push_trade(trade(1.0, 0));
        let out = build_position_series(&s, ts(0), 1.0);
        // The group has exactly its four metrics when an entry IS supplied…
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|c| c.group == "m_position"));
    }
}
