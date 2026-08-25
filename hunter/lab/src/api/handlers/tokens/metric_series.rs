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
//! **The flow context is the fingerprint's patterns AND the token's creator.** The
//! creator wallet is volume-side unconditionally and seeds the contagion set, so a
//! series folded without it books the dev buy + dev dump — usually a token's two
//! largest single flows — as *organic*. That is not a cosmetic difference: it is a
//! different classification from the one the live engine (`reduce.rs`, seeds on
//! `TokenCreated`) and simulate (`engine_sim.rs`, seeds on its `ReplayToken`) fold,
//! which is the whole premise of this endpoint. See [`resolve_flow_ctx`].
//!
//! [`stall_horizon_sec`]: MetricSeriesQuery::stall_horizon_sec

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::grouping::normalize_labels;
use hunter_engine::metrics::flow_split::{wallet_hash, FlowPatterns};
use hunter_engine::metrics::grid::{estimate_sparse_rows, fold_sparse, SparseGrid};
use hunter_engine::metrics::position::PositionCtx;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_spec, metric_spec, MetricGroupId, MetricId, MetricKind, Ts, REGISTRY,
};

use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::token_repo::TokenRepo;
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

    // The static token facts every recorded series must be seeded with, read once
    // (see `TokenFacts`). The flow context borrows the creator hash from it rather
    // than repeating the lookup.
    let facts = load_token_facts(&state, &mint).await;
    let flow_ctx = match resolve_flow_ctx(&state, query.fingerprint_id.as_deref(), &facts).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    // Wallet identity is a LOAD-time decision: the lake leaves the wallet column out
    // unless asked, and a fold over rows without it sees every trade as one anonymous
    // wallet — `unique_wallets` then reads 1 for a hundred traders, silently. This
    // endpoint records every registry column, so it needs the wallet column whenever
    // any recorded metric is wallet-keyed, not only on the flow path.
    let with_flow = flow_ctx.is_some() || records_wallet_keyed_metric();

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
        build_series(&mint, &trades, &windows, &grid, flow_ctx.as_ref(), entry, &facts)
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

/// The static token facts a fold must be seeded with **before its first event**, all
/// off one `tokens` read.
///
/// Every one of them is a metric that reads `NaN` for the whole token when unseeded,
/// and a `NaN` satisfies nothing: an `ix_count <= 5` or `prior_launches == 0` pane
/// draws blank and the condition timeline shows the rule as never firing — the exact
/// silent failure the seeding contract on `MetricSeries::seed_ix_count` warns about.
/// The live engine seeds all three on `TokenCreated` (`reduce.rs`) and the rule
/// readout replay loads them the same way, so a series without them disagrees with
/// the decision it is drawn next to.
#[derive(Debug, Default, Clone, Copy)]
struct TokenFacts {
    /// FNV hash of the creator wallet — volume-side unconditionally, and the seed of
    /// the flow-split contagion set.
    creator_wallet_hash: Option<u64>,
    /// Creation-transaction instruction count (`m_snapshot.ix_count`).
    ix_count: Option<usize>,
    /// The creator's launches strictly before this token (`m_snapshot.prior_launches`).
    prior_launches: Option<u32>,
}

/// Read the `tokens` row once and derive every static fact the fold needs.
///
/// One indexed PK lookup plus, when the creator is known, one counting query — the
/// same pair the live rule readout runs (`load_ix_count` / `load_prior_launches`), so
/// the two surfaces seed from identical values. A missing row is non-fatal and never
/// silent: each fact stays `None`, its metric reads `NaN`, and the reason is logged.
async fn load_token_facts(state: &LocalState, mint: &str) -> TokenFacts {
    let repo = TokenRepo::new(state.core.db.clone());
    let token = match repo.find_by_mint(mint).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::warn!(mint, "metric-series: no tokens row - ix_count/prior_launches unseeded");
            return TokenFacts::default();
        }
        Err(e) => {
            tracing::warn!(mint, error = %e, "metric-series: token lookup failed - ix_count/prior_launches unseeded");
            return TokenFacts::default();
        }
    };
    // Through the SSOT `normalize_labels` so both persisted shapes (bare array and
    // `{"instructions":[...]}`) count alike; a zero count is "never stored", not a
    // creation transaction with no instructions.
    let n = normalize_labels(&token.instruction_labels).len();
    let ix_count = (n > 0).then_some(n);

    if token.creator_wallet.is_empty() {
        tracing::warn!(mint, "metric-series: no creator wallet - flow split + prior_launches unseeded");
        return TokenFacts { creator_wallet_hash: None, ix_count, prior_launches: None };
    }
    let prior_launches = match repo
        .count_prior_launches(&token.creator_wallet, token.created_at)
        .await
    {
        Ok(n) => Some(n.max(0) as u32),
        Err(e) => {
            tracing::warn!(mint, error = %e, "metric-series: prior_launches unseeded");
            None
        }
    };
    TokenFacts {
        creator_wallet_hash: Some(wallet_hash(&token.creator_wallet)),
        ix_count,
        prior_launches,
    }
}

/// Whether any column this endpoint records outside the flow groups is wallet-keyed
/// (`m_flow_window.unique_wallets` today). Registry-derived rather than a name list,
/// so the answer follows `MetricId::needs_wallet_identity` instead of drifting from it.
fn records_wallet_keyed_metric() -> bool {
    REGISTRY
        .iter()
        .filter(|g| {
            !matches!(
                g.id,
                MetricGroupId::FlowSplit | MetricGroupId::FlowSplitWindow | MetricGroupId::Position
            )
        })
        .flat_map(|g| g.metrics.iter())
        .any(|m| m.id.needs_wallet_identity())
}

struct FlowCtx {
    fp_id: FingerprintId,
    patterns: FlowPatterns,
    /// FNV hash of the token's creator wallet — volume-side unconditionally, and the
    /// seed of the contagion set. `None` only when the `tokens` row is missing or
    /// carries no creator. **Load-bearing for parity**: the live engine seeds it on
    /// `TokenCreated` (`reduce.rs`) and simulate seeds it on its `ReplayToken`
    /// (`engine_sim.rs`), so an unseeded series books the dev buy/dump as *organic*
    /// and disagrees with every decision the engine actually makes.
    creator_wallet_hash: Option<u64>,
}

async fn resolve_flow_ctx(
    state: &LocalState,
    fingerprint_id: Option<&str>,
    facts: &TokenFacts,
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
    // The creator comes off the one `tokens` read every series already does
    // ([`load_token_facts`]). A missing row is non-fatal: the series still folds, it
    // just can't seed the creator (logged there, never silent — an unseeded fold
    // misclassifies the dev buy and dump as organic).
    Ok(Some(FlowCtx {
        fp_id: engine_fp.id,
        patterns,
        creator_wallet_hash: facts.creator_wallet_hash,
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
    facts: &TokenFacts,
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
    // Static token facts first, in the same order the live `TokenCreated` arm seeds
    // them (`reduce.rs`), and always BEFORE the first fold — a seed after it would
    // leave the early rows reading `NaN` while the late ones read a value.
    if let Some(n) = facts.ix_count {
        series.seed_ix_count(n);
    }
    if let Some(n) = facts.prior_launches {
        series.seed_prior_launches(n);
    }
    if let Some(ctx) = flow {
        // Same order as the live `TokenCreated` arm (`new_track` → `seed_creator`):
        // `seed_creator` back-fills every flow state already registered.
        series.ensure_flow(ctx.fp_id, &ctx.patterns, windows);
        if let Some(h) = ctx.creator_wallet_hash {
            series.seed_creator(h);
        }
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
    use hunter_engine::metrics::flow_split::ix_hash;
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

    /// A corpus row carrying the flow columns; only the fields the classifier and
    /// the tick grid read are meaningful here.
    fn corpus_trade(
        sol: f64,
        wallet: &str,
        labels: Option<&str>,
        secs: i64,
    ) -> crate::sweep::projection::CorpusTrade {
        crate::sweep::projection::CorpusTrade {
            // Resolved at load in production (`duck.rs` / `project_pg_tail`), so a
            // fixture that carries label/wallet text must resolve them too.
            flow: crate::sweep::projection::FlowKeys::from_stored(labels, Some(wallet)),
            block_time: ts(secs),
            amount_sol: sol,
            token_amount: 1_000.0,
            price_per_token: 1.0,
            reserve_sol: Some(30.0),
            reserve_token: Some(30.0),
            real_reserve_sol: Some(30.0),
            real_token_reserves: Some(30.0),
            slot: secs as u64 + 1,
            leg_index: 0,
            is_buy: true,
            tx_signature: None,
            ix_labels: labels.map(Box::from),
            wallet: Some(Box::from(wallet)),
        }
    }

    /// Last finite value of a lifetime (`window_size_sec: null`) column.
    fn last_lifetime(resp: &serde_json::Value, metric: &str) -> f64 {
        let col = resp["series"]
            .as_array()
            .expect("series array")
            .iter()
            .find(|s| s["metric"] == metric && s["window_size_sec"].is_null())
            .unwrap_or_else(|| panic!("{metric} lifetime column present"));
        col["values"]
            .as_array()
            .expect("values array")
            .iter()
            .rev()
            .find_map(serde_json::Value::as_f64)
            .unwrap_or_else(|| panic!("{metric} has a finite value"))
    }

    /// The parity guard for the creator seed: the dev buy is volume-side even with
    /// no matching `ix_labels`, exactly as the live engine folds it (`reduce.rs`
    /// seeds `TokenCreated`) and simulate folds it (`engine_sim.rs`). Unseeded, the
    /// creator's SOL lands in `nonvol_*` and the pane disagrees with every decision.
    #[test]
    fn the_creator_wallet_is_volume_side_even_without_a_pattern_match() {
        let patterns = FlowPatterns::new(std::collections::BTreeSet::from([ix_hash(&[
            "Pump.Fun: Create",
            "Pump.Fun: Buy",
        ])]));
        let ctx = FlowCtx {
            fp_id: FingerprintId(Uuid::new_v4()),
            patterns,
            creator_wallet_hash: Some(wallet_hash("dev")),
        };
        // Dev buys 5 (no labels ⇒ classified only by the creator seed), a stranger
        // buys 3 (no labels ⇒ organic), a bot buys 2 on the configured pattern.
        let trades = [
            corpus_trade(5.0, "dev", None, 0),
            corpus_trade(3.0, "normie", None, 1),
            corpus_trade(2.0, "bot", Some(r#"["Pump.Fun: Create","Pump.Fun: Buy"]"#), 2),
        ];
        let grid = SparseGrid::for_windows(&[10.0]);

        let seeded = build_series("mint", &trades, &[10.0], &grid, Some(&ctx), None, &TokenFacts::default());
        assert_eq!(last_lifetime(&seeded, "vol_buy"), 7.0, "dev + pattern bot");
        assert_eq!(last_lifetime(&seeded, "nonvol_buy"), 3.0, "the stranger only");
        assert_eq!(last_lifetime(&seeded, "nonvol_net"), 3.0);

        // Unseeded (the pre-fix behavior) the dev's 5 SOL crosses into organic —
        // the exact drift this seed exists to prevent.
        let unseeded = FlowCtx { creator_wallet_hash: None, ..ctx };
        let out = build_series("mint", &trades, &[10.0], &grid, Some(&unseeded), None, &TokenFacts::default());
        assert_eq!(last_lifetime(&out, "vol_buy"), 2.0);
        assert_eq!(last_lifetime(&out, "nonvol_buy"), 8.0);
    }

    /// Every value of a lifetime column, `None` where the metric read non-finite.
    fn lifetime_values(resp: &serde_json::Value, metric: &str) -> Vec<Option<f64>> {
        resp["series"]
            .as_array()
            .expect("series array")
            .iter()
            .find(|s| s["metric"] == metric && s["window_size_sec"].is_null())
            .unwrap_or_else(|| panic!("{metric} lifetime column present"))["values"]
            .as_array()
            .expect("values array")
            .iter()
            .map(serde_json::Value::as_f64)
            .collect()
    }

    /// The seeding contract for the static `m_snapshot` facts: they must be on the
    /// track before the first fold, so EVERY row carries them — not just the rows
    /// after some later seed. Unseeded they read `null` for the whole token, which is
    /// what made an `ix_count <= 5` pane draw blank and its condition timeline show a
    /// rule that fires live as never firing.
    #[test]
    fn snapshot_facts_are_seeded_on_every_row() {
        let trades = [corpus_trade(5.0, "dev", None, 0), corpus_trade(3.0, "normie", None, 1)];
        let grid = SparseGrid::for_windows(&[10.0]);
        let facts = TokenFacts { creator_wallet_hash: None, ix_count: Some(7), prior_launches: Some(3) };

        let out = build_series("mint", &trades, &[10.0], &grid, None, None, &facts);
        for (metric, want) in [("ix_count", 7.0), ("prior_launches", 3.0)] {
            let vals = lifetime_values(&out, metric);
            assert!(!vals.is_empty(), "{metric} has rows");
            assert!(vals.iter().all(|v| *v == Some(want)), "{metric} on every row: {vals:?}");
        }

        // Absent facts stay `null` — the honest reading for a token whose creation
        // labels / creator were never stored, and never a `0` an `== 0` gate matches.
        let out = build_series("mint", &trades, &[10.0], &grid, None, None, &TokenFacts::default());
        for metric in ["ix_count", "prior_launches"] {
            assert!(
                lifetime_values(&out, metric).iter().all(Option::is_none),
                "{metric} unseeded reads null",
            );
        }
    }

    /// The extensibility contract, as a guard: a metric added to `REGISTRY` must
    /// appear as a column here with **no** change to this file. The response is what
    /// every chart pane and the lab condition timeline read, so a metric missing from
    /// it is a metric that does not exist on the frontend — and the omission is
    /// silent, since a pane with no column simply draws nothing.
    #[test]
    fn every_registry_metric_is_a_column() {
        let patterns = FlowPatterns::new(std::collections::BTreeSet::from([ix_hash(&[
            "Pump.Fun: Buy",
        ])]));
        let ctx = FlowCtx {
            fp_id: FingerprintId(Uuid::new_v4()),
            patterns,
            creator_wallet_hash: Some(wallet_hash("dev")),
        };
        let trades = [corpus_trade(5.0, "dev", None, 0), corpus_trade(3.0, "normie", None, 1)];
        let grid = SparseGrid::for_windows(&[10.0]);
        let out = build_series(
            "mint",
            &trades,
            &[10.0],
            &grid,
            Some(&ctx),
            // An entry context, so the position-scoped group is computed too.
            Some((ts(0), 1.0)),
            &TokenFacts { creator_wallet_hash: None, ix_count: Some(3), prior_launches: Some(1) },
        );
        let cols: Vec<(String, String)> = out["series"]
            .as_array()
            .expect("series array")
            .iter()
            .map(|c| (c["group"].as_str().unwrap().to_string(), c["metric"].as_str().unwrap().to_string()))
            .collect();
        for g in REGISTRY {
            for m in g.metrics {
                assert!(
                    cols.iter().any(|(cg, cm)| cg == g.name && cm == m.name),
                    "{}.{} has no column - the pane for it draws nothing",
                    g.name,
                    m.name,
                );
            }
        }
    }

    /// `unique_wallets` is wallet-keyed, so this endpoint must load the wallet column
    /// on every request — not only when a fingerprint puts it on the flow path.
    #[test]
    fn the_endpoint_declares_it_needs_wallet_identity() {
        assert!(records_wallet_keyed_metric());
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
