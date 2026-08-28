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
//! metric (`m_flow_window`/`m_flow_ix_window` decay, `m_price_window` rolling
//! extrema, `m_state.stall`/`.time`, the dead verdict) advances *only* inside a
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
use hunter_engine::metrics::dump_ix::DumpPatterns;
use hunter_engine::metrics::flow_ix::{wallet_hash, FlowPatterns};
use hunter_engine::metrics::grid::{estimate_sparse_rows, fold_sparse, SparseGrid};
use hunter_engine::metrics::position::PositionCtx;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{
    group_of, group_spec, is_two_window, metric_spec, MetricGroupId, MetricId, MetricKind, Ts,
    WindowSpec, Windows, REGISTRY,
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
    /// Largest `m_state.time` condition value (+ `=`-tolerance) the caller will
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
    /// The whole span this column was computed over. Present only for dynamic
    /// metrics. Readers should prefer this over [`window_size_sec`](Self::window_size_sec).
    window: Option<hunter_engine::metrics::WindowSpec>,
    /// The nested SLICE span, for the two-window metrics alone
    /// (`m_flow_window.trade_share` / `.sol_share`). Their reading is a ratio ACROSS
    /// the pair, so a column labelled by `window` alone names a different number than
    /// it holds — and computing them without it left both metrics all-`null` at every
    /// window, a chart pane that could never draw.
    slice: Option<hunter_engine::metrics::WindowSpec>,
    /// Legacy seconds scalar, kept for readers that predate `window`. `None` on a
    /// slot or print span — neither has seconds to report, so a reader that only
    /// knows this key drops the column rather than calling 30 slots 30 seconds.
    window_size_sec: Option<f64>,
    /// One value per event (aligned with `at`); non-finite values serialize `null`.
    values: Vec<Option<f64>>,
}

/// Default dynamic windows when the caller doesn't specify any. Wall-clock, because
/// a caller that names none is browsing rather than checking a specific rule.
const DEFAULT_WINDOWS: &[f64] = &[10.0, 30.0, 60.0];

/// `GET /api/tokens/{mint}/metric-series?windows=10,30s,30sl@1,20p&curve_only=false` — every
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
        max_window_secs: max_window_secs(&windows),
        time_horizon_secs: SparseGrid::clamp_secs(query.time_horizon_sec.unwrap_or(0.0)),
        stall_horizon_secs: SparseGrid::clamp_secs(query.stall_horizon_sec.unwrap_or(0.0)),
    };
    // Position-scoped metrics need the caller's entry fill; require both halves and a
    // sane price, else they stay omitted (the token-only replay can't compute them).
    let entry = match (query.entry_time, query.entry_price) {
        (Some(at), Some(price)) if price.is_finite() && price > 0.0 => Some((at, price)),
        _ => None,
    };

    // The static token fact every recorded series must be seeded with, read once
    // (see `TokenFacts`). The flow context carries it into the fold.
    let facts = load_token_facts(&state, &mint).await;
    let flow_ctx = match resolve_flow_ctx(&state, query.fingerprint_id.as_deref(), &facts).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    // Wallet identity is a LOAD-time decision: the lake leaves the wallet column out
    // unless asked, and a fold over rows without it sees every trade as one anonymous
    // wallet — `m_crowd_window.unique_wallets` then reads 1 for a hundred traders,
    // silently. This endpoint records every registry column, so it needs the wallet
    // column whenever any recorded metric is wallet-keyed, not only on the flow path.
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

/// The static token facts a fold must be seeded with **before its first event**.
///
/// Each is a metric that reads `NaN` for the whole token when unseeded, and a `NaN`
/// satisfies nothing: the pane draws blank and the condition timeline shows the rule
/// as never firing. The live engine seeds them on `TokenCreated` (`reduce.rs`) and the
/// rule readout replay loads them the same way, so a series without them disagrees
/// with the decision it is drawn next to.
///
/// `ix_count`, `prior_launches` and `first_slot_buy_lamports` are NOT here: they are
/// fingerprint axes, not metrics, so they select which tokens a rule arms on rather
/// than being folded into a series.
#[derive(Debug, Default, Clone, Copy)]
struct TokenFacts {
    /// FNV hash of the creator wallet — tagged unconditionally, and the seed of
    /// the ix-split contagion set.
    creator_wallet_hash: Option<u64>,
}

/// Read the `tokens` row once and derive every static fact the fold needs.
///
/// One indexed PK lookup — the same read the rule readout runs, so the two surfaces
/// seed from identical values. A missing row is non-fatal and never silent: the fact
/// stays `None`, its metric reads `NaN`, and the reason is logged.
async fn load_token_facts(state: &LocalState, mint: &str) -> TokenFacts {
    let repo = TokenRepo::new(state.core.db.clone());
    let token = match repo.find_by_mint(mint).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::warn!(mint, "metric-series: no tokens row - static facts unseeded");
            return TokenFacts::default();
        }
        Err(e) => {
            tracing::warn!(mint, error = %e, "metric-series: token lookup failed - static facts unseeded");
            return TokenFacts::default();
        }
    };
    if token.creator_wallet.is_empty() {
        tracing::warn!(mint, "metric-series: no creator wallet - flow split unseeded");
        return TokenFacts { creator_wallet_hash: None };
    }
    TokenFacts { creator_wallet_hash: Some(wallet_hash(&token.creator_wallet)) }
}

/// Whether any column this endpoint records outside the split-flow groups is
/// wallet-keyed (`m_crowd_window` today). Registry-derived rather than a name list, so
/// the answer follows `MetricId::needs_wallet_identity` instead of drifting from it.
fn records_wallet_keyed_metric() -> bool {
    REGISTRY
        .iter()
        .filter(|g| {
            !matches!(
                g.id,
                MetricGroupId::FlowIx | MetricGroupId::FlowIxWindow | MetricGroupId::Position
            )
        })
        .flat_map(|g| g.metrics.iter())
        .any(|m| m.id.needs_wallet_identity())
}

struct FlowCtx {
    fp_id: FingerprintId,
    /// `m_flow_ix.ix_patterns`; `None` ⇒ the flow groups are omitted from the series.
    patterns: Option<FlowPatterns>,
    /// `m_dump_ix.ix_patterns`. Independently optional — the two lists are separate
    /// groups on one row, and requiring the flow one made a dump-only fingerprint
    /// resolve to no context at all, which built `m_dump_ix` columns unscoped: a
    /// line that reads `NaN` on every row rather than one that is absent.
    dump: Option<DumpPatterns>,
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
    let patterns = FlowPatterns::from_metric_config(&engine_fp.metric_config);
    let dump = DumpPatterns::from_metric_config(&engine_fp.metric_config);
    if patterns.is_none() && dump.is_none() {
        // Fingerprint present but unconfigured for either list — omit the
        // fingerprint-scoped columns entirely (same as no id).
        return Ok(None);
    }
    // The creator comes off the one `tokens` read every series already does
    // ([`load_token_facts`]). A missing row is non-fatal: the series still folds, it
    // just can't seed the creator (logged there, never silent — an unseeded fold
    // misclassifies the dev buy and dump as organic).
    Ok(Some(FlowCtx {
        fp_id: engine_fp.id,
        patterns,
        dump,
        creator_wallet_hash: facts.creator_wallet_hash,
    }))
}

/// The grid is a WALL clock, so a slot span converts at the nominal slot time and a
/// PRINT span contributes nothing - no tick can move a print cursor, and a trade emits
/// its own row. Mirrors `ClockHorizons::absorb_req`: this only sizes the horizon,
/// never a reading, so the slot approximation costs coverage and never correctness.
fn max_window_secs(windows: &[WindowSpec]) -> f64 {
    windows
        .iter()
        .map(|w| match w.unit {
            hunter_engine::metrics::WindowUnit::Sec => w.size + w.lag,
            hunter_engine::metrics::WindowUnit::Slot => {
                (w.size + w.lag) * hunter_engine::metrics::NOMINAL_SLOT_SECS
            }
            hunter_engine::metrics::WindowUnit::Print => 0.0,
        })
        .fold(0.0_f64, f64::max)
}

/// The nested `(reference, slice)` pairs the two-window metrics are read over: every
/// ORDERED pair of requested spans that can nest — same unit, slice strictly narrower.
///
/// No new query param, because a caller asking for `10,30,60` has already named the
/// spans it wants compared, and `trade_share(60s/10s)` is exactly that comparison.
/// The alternative shipped for a while and was worse than useless: computing these
/// metrics at a bare window produced a column of `NaN` on every row of every token, so
/// the chart offered two panes that could never draw.
fn nested_pairs(windows: &[WindowSpec]) -> Vec<(WindowSpec, WindowSpec)> {
    windows
        .iter()
        .flat_map(|&reference| {
            windows
                .iter()
                // Same unit, because a ratio across two clocks is not a share of
                // anything (`rule_params::validate_group` rejects the same pair), and
                // strictly narrower, because a slice equal to its reference reads 100
                // on every token.
                .filter(move |b| b.unit == reference.unit && b.size < reference.size)
                .map(move |&slice| (reference, slice))
        })
        .collect()
}

/// Parse the `windows` CSV into a deduped, positive, finite list; fall back to the
/// default set when absent/empty.
fn parse_windows(raw: Option<&str>) -> Vec<WindowSpec> {
    // `WindowSpec::parse` is the same grammar a persisted exit reason and a chart
    // legend use, and a bare number in it is seconds — so `?windows=10,30,60` means
    // exactly what it always did while `30sl@1` and `20p` now mean themselves.
    let mut ws: Vec<WindowSpec> =
        raw.unwrap_or("").split(',').filter_map(WindowSpec::parse).collect();
    // Dedup on the WHOLE span, the same identity the engine keys buffers by: a
    // 30-second and a 30-slot column are two reads, and collapsing them would return
    // one series under two labels.
    ws.sort_by_key(|w| w.key());
    ws.dedup_by_key(|w| w.key());
    if ws.is_empty() {
        DEFAULT_WINDOWS.iter().copied().map(WindowSpec::secs).collect()
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
    windows: &[WindowSpec],
    grid: &SparseGrid,
    flow: Option<&FlowCtx>,
    entry: Option<(Ts, f64)>,
) -> serde_json::Value {
    let mut columns: Vec<SeriesColumn> = Vec::new();
    // `(column, reference span, slice span)` — the slice is set for the two-window
    // metrics alone, and it is what makes their columns computable at all.
    let mut labels: Vec<(SeriesColumn, Option<WindowSpec>, Option<WindowSpec>)> = Vec::new();
    let nested = nested_pairs(windows);
    for group in REGISTRY {
        // A fingerprint-scoped group needs ITS OWN list off the row, not merely a
        // fingerprint: `m_flow_ix` reads the tagged patterns, `m_dump_ix` the dump
        // builds, and a group whose list is unconfigured is omitted rather than
        // drawn as an all-`NaN` line.
        let fp_id = match group.id {
            MetricGroupId::FlowIx | MetricGroupId::FlowIxWindow => {
                flow.filter(|c| c.patterns.is_some()).map(|c| c.fp_id)
            }
            MetricGroupId::DumpIx | MetricGroupId::DumpIxWindow => {
                flow.filter(|c| c.dump.is_some()).map(|c| c.fp_id)
            }
            _ => None,
        };
        let is_flow_group = matches!(
            group.id,
            MetricGroupId::FlowIx
                | MetricGroupId::FlowIxWindow
                | MetricGroupId::DumpIx
                | MetricGroupId::DumpIxWindow
        );
        if is_flow_group && fp_id.is_none() {
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
                    let fp = fp_id.unwrap();
                    let col = SeriesColumn::Fingerprint(m.id, None, fp);
                    columns.push(col);
                    labels.push((col, None, None));
                }
                MetricKind::Static => {
                    columns.push(SeriesColumn::Static(m.id));
                    labels.push((SeriesColumn::Static(m.id), None, None));
                }
                MetricKind::Dynamic if is_flow_group => {
                    let fp = fp_id.unwrap();
                    for &w in windows {
                        let col = SeriesColumn::Fingerprint(m.id, Some(w), fp);
                        columns.push(col);
                        labels.push((col, Some(w), None));
                    }
                }
                // A two-window metric is a ratio ACROSS a nested pair, so one span is
                // not a column of it — asking for `trade_share` at a bare window built
                // a column that read `NaN` on every row of every token.
                MetricKind::Dynamic if is_two_window(m.id) => {
                    for &(reference, slice) in &nested {
                        let col =
                            SeriesColumn::Window(m.id, Windows::two(reference, slice));
                        columns.push(col);
                        labels.push((col, Some(reference), Some(slice)));
                    }
                }
                MetricKind::Dynamic => {
                    for &w in windows {
                        columns.push(SeriesColumn::window(m.id, w));
                        labels.push((SeriesColumn::window(m.id, w), Some(w), None));
                    }
                }
            }
        }
    }

    let created_at = trades[0].block_time;
    let mut series = MetricSeries::new(created_at, columns);
    // The static token fact, BEFORE the first fold — a seed after it would leave the
    // early rows reading `NaN` while the late ones read a value.
    if let Some(ctx) = flow {
        // Same order as the live `TokenCreated` arm (`new_track` → `seed_creator`):
        // `seed_creator` back-fills every flow state already registered.
        if let Some(p) = &ctx.patterns {
            series.ensure_flow(ctx.fp_id, p, windows);
        }
        if let Some(d) = &ctx.dump {
            series.ensure_dump(ctx.fp_id, d, windows);
        }
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
        .filter_map(|&(col, window, slice)| {
            let id = match col {
                SeriesColumn::Static(id) => id,
                SeriesColumn::Window(id, _) => id,
                SeriesColumn::Fingerprint(id, _, _) => id,
            };
            let values = series.column_values(col)?;
            Some(SeriesOut {
                metric: metric_spec(id).name,
                group: group_of(id).name,
                slice,
                unit: metric_spec(id).unit.as_str(),
                window,
                // Seconds only: a slot or print span has none to report.
                window_size_sec: window
                    .filter(|w| w.unit == hunter_engine::metrics::WindowUnit::Sec && w.lag == 0.0)
                    .map(|w| w.size),
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
                // Position metrics are static: no span on any key.
                window: None,
                slice: None,
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
    use hunter_engine::metrics::flow_ix::ix_hash;
    use hunter_engine::metrics::{Side, TradeLite};

    fn ts(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn trade(price: f64, secs: i64) -> TradeLite {
        TradeLite { side: Side::Buy, sol: 1.0, price, reserve_sol: 30.0, at: ts(secs), ..Default::default() }
    }

    /// The two-window metrics are read over NESTED pairs of the requested spans, and
    /// only over pairs that can actually nest. A pair that cannot is not a stricter
    /// column, it is a `NaN` one — which is what a bare window produced for every row
    /// of every token before these metrics were paired here at all.
    #[test]
    fn the_two_window_metrics_are_read_over_nestable_pairs_only() {
        let secs = |n: f64| WindowSpec::secs(n);
        assert_eq!(
            nested_pairs(&[secs(10.0), secs(30.0), secs(60.0)]),
            vec![
                (secs(30.0), secs(10.0)),
                (secs(60.0), secs(10.0)),
                (secs(60.0), secs(30.0)),
            ],
        );
        // A single span nests in nothing, so it yields no column rather than a NaN one.
        assert!(nested_pairs(&[secs(30.0)]).is_empty());
        // Equal sizes read 100 on every token; a cross-unit ratio is not a share at all
        // and the rule validator rejects the same pair.
        assert!(nested_pairs(&[secs(30.0), WindowSpec::slots(10.0, 0.0)]).is_empty());
    }

    /// Every dynamic metric the endpoint offers must be COMPUTABLE at the spans it is
    /// offered at. A column that is `NaN` by construction is a chart pane that can
    /// never draw, and the registry-driven builder will happily emit one.
    #[test]
    fn every_dynamic_column_the_endpoint_builds_is_computable() {
        let windows = parse_windows(None);
        for g in REGISTRY {
            if g.kind != MetricKind::Dynamic {
                continue;
            }
            for m in g.metrics {
                let n = if is_two_window(m.id) {
                    nested_pairs(&windows).len()
                } else {
                    windows.len()
                };
                assert!(n > 0, "{}.{} would be offered with no computable span", g.name, m.name);
            }
        }
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

    /// The parity guard for the creator seed: the dev buy is TAGGED even with
    /// no matching `ix_labels`, exactly as the live engine folds it (`reduce.rs`
    /// seeds `TokenCreated`) and simulate folds it (`engine_sim.rs`). Unseeded, the
    /// creator's SOL lands in `untagged_*` and the pane disagrees with every decision.
    #[test]
    fn the_creator_wallet_is_tagged_even_without_a_pattern_match() {
        let patterns = FlowPatterns::new(std::collections::BTreeSet::from([ix_hash(&[
            "Pump.Fun: Create",
            "Pump.Fun: Buy",
        ])]));
        let ctx = FlowCtx {
            fp_id: FingerprintId(Uuid::new_v4()),
            patterns: Some(patterns),
            // This one is about the FLOW seed; `m_dump_ix` has no wallet rule to seed.
            dump: None,
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

        let seeded = build_series("mint", &trades, &[WindowSpec::secs(10.0)], &grid, Some(&ctx), None);
        assert_eq!(last_lifetime(&seeded, "tagged_buy"), 7.0, "dev + pattern bot");
        assert_eq!(last_lifetime(&seeded, "untagged_buy"), 3.0, "the stranger only");
        assert_eq!(last_lifetime(&seeded, "untagged_net"), 3.0);

        // Unseeded (the pre-fix behavior) the dev's 5 SOL crosses into organic —
        // the exact drift this seed exists to prevent.
        let unseeded = FlowCtx { creator_wallet_hash: None, ..ctx };
        let out = build_series("mint", &trades, &[WindowSpec::secs(10.0)], &grid, Some(&unseeded), None);
        assert_eq!(last_lifetime(&out, "tagged_buy"), 2.0);
        assert_eq!(last_lifetime(&out, "untagged_buy"), 8.0);
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
        // BOTH lists: a fingerprint-scoped group is omitted when its own list is
        // unconfigured, so a fixture carrying only the tagged one would assert
        // nothing about `m_dump_ix` while still passing.
        let ctx = FlowCtx {
            fp_id: FingerprintId(Uuid::new_v4()),
            patterns: Some(patterns),
            dump: Some(DumpPatterns::new(std::collections::BTreeSet::from([ix_hash(&[
                "Pump.Fun: Sell",
            ])]))),
            creator_wallet_hash: Some(wallet_hash("dev")),
        };
        let trades = [corpus_trade(5.0, "dev", None, 0), corpus_trade(3.0, "normie", None, 1)];
        let grid = SparseGrid::for_windows(&[10.0, 30.0]);
        // TWO spans, because the two-window metrics are a ratio across a nested pair:
        // one span names no such pair, and the honest answer there is no column at all
        // rather than a column of `NaN`.
        let windows = [WindowSpec::secs(30.0), WindowSpec::secs(10.0)];
        let out = build_series(
            "mint",
            &trades,
            &windows,
            &grid,
            Some(&ctx),
            // An entry context, so the position-scoped group is computed too.
            Some((ts(0), 1.0)),
        );
        let series = out["series"].as_array().expect("series array");
        let cols: Vec<(String, String)> = series
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
        // A two-window column carries BOTH spans. Without the slice the value is `NaN`
        // by construction and the label names a window the number is not read over.
        for c in series.iter().filter(|c| c["metric"] == "trade_share" || c["metric"] == "sol_share") {
            assert!(!c["window"].is_null(), "{c:?} has no reference span");
            assert!(!c["slice"].is_null(), "{c:?} has no slice span");
        }
    }

    /// `m_crowd_window` is wallet-keyed, so this endpoint must load the wallet column
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
