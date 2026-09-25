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
//! read (windowed flows decay, rolling price extrema age out, `m_price.stall_sec` and
//! `m_state.age_sec` climb, the dead verdict) advances *only* inside a
//! tick, so a trade-only fold samples them exactly where a fresh trade has just
//! been folded back in and never sees a between-trades crossing. That shipped: an
//! `m_flow.buy_sol [10s] < 5` exit drew 70 s after the one simulate booked, because
//! the dip happened in a 1.3 s gap between two trades.
//!
//! Because the grid's density is set by what the caller will *evaluate* over the
//! series, the rule's `age_sec`/`stall_sec` condition ceilings come in as query params
//! ([`MetricSeriesQuery::time_horizon_sec`] / [`stall_horizon_sec`]); the trailing
//! windows are already implied by `windows`. A horizon left at `0` only drops ticks
//! in quiet gaps past every other horizon — never near a trade.
//!
//! **The tag context is the fingerprint's tags AND the token's creator.** A tag with
//! the `creator` matcher (or `sticky`) takes the dev's trades by wallet, so a series
//! folded without the creator books the dev buy and dump — usually a token's two
//! largest single flows — on the other side of the split: a different classification
//! from the one the live engine (`reduce.rs`, seeds on `TokenCreated`) and simulate
//! (`engine_sim.rs`, seeds on its `ReplayToken`) fold. See [`load_creator`].
//!
//! **Every read, from the registry.** The columns are [`chart_reads`] of every metric:
//! each tag and span it accepts. A metric added to the registry is drawn here with no
//! change to this file.
//!
//! [`stall_horizon_sec`]: MetricSeriesQuery::stall_horizon_sec

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::grid::{estimate_sparse_rows, fold_sparse, SparseGrid};
use hunter_engine::metrics::position::{position_value, PositionCtx};
use hunter_engine::metrics::registry::METRICS;
#[cfg(test)]
use hunter_engine::metrics::registry::TagLevel;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::tags::config::{compile_tags, CompiledTag};
use hunter_engine::metrics::trade_keys::wallet_hash;
use hunter_engine::metrics::{chart_reads, family_spec, Family, MetricRef, Ts, WindowSpec};

use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::token_repo::TokenRepo;
use trading_core::strategies::fingerprint_axes::fp_to_engine;

use crate::state::local_state::LocalState;
use crate::strategies::sim_fetch::fetch_full_history_one_opts;
use crate::sweep::generic::strategy::register_tags;
use crate::sweep::projection::to_trade_lite;

/// Query for the metric-series read: which trailing spans to draw the windowed reads
/// over, whether to drop AMM legs, and the fingerprint whose tags the tagged reads use.
#[derive(Debug, Deserialize)]
pub struct MetricSeriesQuery {
    /// Comma-separated spans for windowed reads (`10,30s,30sl@1,20p`; a bare number is
    /// seconds). Omitted ⇒ a default set.
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub curve_only: bool,
    /// Fingerprint whose `tags` the tagged reads use. Absent ⇒ tagged reads are omitted
    /// (a tag means nothing without its definition).
    #[serde(default)]
    pub fingerprint_id: Option<String>,
    /// Entry-fill time of the run being inspected (RFC3339). With `entry_price` it
    /// anchors the `m_position` reads — a coin-only replay has no position, so without
    /// the pair those columns are omitted (they would be all-`NaN`).
    #[serde(default)]
    pub entry_time: Option<DateTime<Utc>>,
    /// Entry-fill price of the inspected run (the `pnl_pct` reference).
    #[serde(default)]
    pub entry_price: Option<f64>,
    /// Largest `m_state.age_sec` threshold (+ `=` tolerance) the caller will evaluate,
    /// in seconds. Sizes the sparse tick grid so the clock is sampled densely up to the
    /// last instant it could still cross. Omitted/`0` ⇒ not evaluated.
    #[serde(default)]
    pub time_horizon_sec: Option<f64>,
    /// Same, for `m_price.stall_sec` (measured from the last trade).
    #[serde(default)]
    pub stall_horizon_sec: Option<f64>,
}

/// Row ceiling for one series response. The sparse grid keeps rows in proportion to
/// trades for a normally-traded token, so this bites only on a token that trades for
/// many hours. Hitting it **truncates in time** rather than coarsening the grid: every
/// returned row stays on the engine's `TICK_MS` decision grid, and the response flags
/// the short coverage.
const MAX_SERIES_ROWS: usize = 40_000;

/// One computed series in the response.
#[derive(serde::Serialize)]
struct SeriesOut {
    /// Registry path, `m_flow.buy_sol`.
    metric: String,
    family: &'static str,
    unit: &'static str,
    /// `volume` / `!volume` / a built-in class; absent untagged.
    tag: Option<String>,
    /// The span, `10s` / `30sl@1` / `age0s`; absent for the life.
    span: Option<String>,
    /// The nested slice of a two-window read.
    slice: Option<String>,
    /// The one full spelling, `m_flow.buy_sol @!volume [10s]` — the rule editor's.
    label: String,
    /// One value per event (aligned with `at`); non-finite values serialize `null`.
    values: Vec<Option<f64>>,
}

impl SeriesOut {
    fn of(r: MetricRef, values: Vec<Option<f64>>) -> Self {
        let spec = r.metric.spec();
        Self {
            metric: spec.path(),
            family: family_spec(spec.family).name,
            unit: spec.unit.as_str(),
            tag: r.tag.map(|t| t.text()),
            span: r.span.span_text(),
            slice: r.span.slice_text(),
            label: r.label(),
            values,
        }
    }
}

/// Default spans when the caller names none. Wall-clock, because a caller that names
/// none is browsing rather than checking a specific rule.
const DEFAULT_WINDOWS: &[f64] = &[10.0, 30.0, 60.0];

/// `GET /api/tokens/{mint}/metric-series?windows=10,30s,30sl@1,20p&curve_only=false` —
/// every read of every metric at every event of the token, as parallel arrays (plus the
/// per-event spot `price` for chart markers).
pub async fn token_metric_series(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<String>,
    query: web::Query<MetricSeriesQuery>,
) -> impl Responder {
    let mint = path.into_inner();
    let windows = parse_windows(query.windows.as_deref());
    // The grid stays dense wherever anything the caller evaluates can still move: the
    // trailing spans plus the two clocks the caller declares. Deadness is covered by
    // the grid itself.
    let grid = SparseGrid {
        max_window_secs: max_window_secs(&windows),
        time_horizon_secs: SparseGrid::clamp_secs(query.time_horizon_sec.unwrap_or(0.0)),
        stall_horizon_secs: SparseGrid::clamp_secs(query.stall_horizon_sec.unwrap_or(0.0)),
    };
    let entry = match (query.entry_time, query.entry_price) {
        (Some(at), Some(price)) if price.is_finite() && price > 0.0 => Some((at, price)),
        _ => None,
    };
    let creator = load_creator(&state, &mint).await;
    let tags = match resolve_tag_ctx(&state, query.fingerprint_id.as_deref()).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    // Wallet identity is a LOAD-time decision: without the wallet column every trade is
    // one anonymous wallet and `m_crowd.unique_wallets` reads 1 for a hundred traders.
    // This endpoint records every coin read, so it asks for the column whenever any of
    // them is wallet-keyed.
    let with_flow = tags.is_some() || records_wallet_keyed_metric();

    let trades = match fetch_full_history_one_opts(&state.trade_repo(), &mint, query.curve_only, with_flow).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("metric-series trade fetch failed for {mint}: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "trade fetch failed" }));
        }
    };
    if trades.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!({
            "mint_address": mint, "at": [], "price": [], "series": [],
            "truncated": false, "covered_until": serde_json::Value::Null,
        }));
    }
    let result = web::block(move || build_series(&mint, &trades, &windows, &grid, tags.as_ref(), creator, entry)).await;
    match result {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            tracing::error!("metric-series compute task panicked: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": "metric-series compute failed" }))
        }
    }
}

/// The token's creator wallet hash — the `creator` matcher and a `sticky` tag's first
/// member. The live engine seeds it on `TokenCreated` and simulate on its
/// `ReplayToken`, so a series folded without it would book the dev buy and dump
/// differently from every decision the engine makes. A missing row is logged, never
/// silent.
async fn load_creator(state: &LocalState, mint: &str) -> Option<u64> {
    match TokenRepo::new(state.core.db.clone()).find_by_mint(mint).await {
        Ok(Some(t)) if !t.creator_wallet.is_empty() => Some(wallet_hash(&t.creator_wallet)),
        Ok(_) => {
            tracing::warn!(mint, "metric-series: no creator wallet - creator and sticky tags unseeded");
            None
        }
        Err(e) => {
            tracing::warn!(mint, error = %e, "metric-series: token lookup failed - creator unseeded");
            None
        }
    }
}

/// Whether any coin read this endpoint records is wallet-keyed — from the registry,
/// so the answer follows `Metric::needs_wallet_identity`.
fn records_wallet_keyed_metric() -> bool {
    METRICS.iter().filter(|m| m.family != Family::Position).any(|m| m.id.needs_wallet_identity(false))
}

/// The fingerprint whose tags the tagged reads use, compiled.
struct TagCtx {
    fp_id: FingerprintId,
    tags: Vec<CompiledTag>,
}

async fn resolve_tag_ctx(state: &LocalState, fingerprint_id: Option<&str>) -> Result<Option<TagCtx>, HttpResponse> {
    let Some(raw) = fingerprint_id.filter(|s| !s.is_empty()) else { return Ok(None) };
    let id = Uuid::parse_str(raw)
        .map_err(|_| HttpResponse::BadRequest().json(serde_json::json!({ "error": "invalid fingerprint_id" })))?;
    let fp = FingerprintRepo::new(state.core.db.clone()).find(id).await.map_err(|e| {
        tracing::error!("metric-series fingerprint lookup failed: {e}");
        HttpResponse::InternalServerError().json(serde_json::json!({ "error": "fingerprint lookup failed" }))
    })?;
    let Some(fp) = fp else {
        return Err(HttpResponse::NotFound().json(serde_json::json!({ "error": "fingerprint not found" })));
    };
    let engine_fp = fp_to_engine(&fp);
    let tags = compile_tags(&engine_fp.tags);
    // A fingerprint with no tags adds no column: same as no id.
    Ok((!tags.is_empty()).then_some(TagCtx { fp_id: engine_fp.id, tags }))
}

/// The grid is a WALL clock, so a slot span converts at the nominal slot time and a
/// print span contributes nothing. Sizes the horizon, never a reading.
fn max_window_secs(windows: &[WindowSpec]) -> f64 {
    windows
        .iter()
        .map(|w| match w.unit {
            hunter_engine::metrics::WindowUnit::Sec => w.size + w.lag,
            hunter_engine::metrics::WindowUnit::Slot => (w.size + w.lag) * hunter_engine::metrics::NOMINAL_SLOT_SECS,
            hunter_engine::metrics::WindowUnit::Print => 0.0,
        })
        .fold(0.0_f64, f64::max)
}

/// Parse the `windows` CSV into a deduped list (a bare number is seconds); the default
/// set when absent or empty.
fn parse_windows(raw: Option<&str>) -> Vec<WindowSpec> {
    let mut ws: Vec<WindowSpec> = raw.unwrap_or("").split(',').filter_map(WindowSpec::parse).collect();
    // Dedup on the WHOLE span: a 30-second and a 30-slot column are two reads.
    ws.sort_by_key(|w| w.key());
    ws.dedup_by_key(|w| w.key());
    if ws.is_empty() {
        DEFAULT_WINDOWS.iter().copied().map(WindowSpec::secs).collect()
    } else {
        ws
    }
}

/// Every coin read the endpoint draws: [`chart_reads`] of every non-position metric,
/// the tagged ones only when a fingerprint supplies the tags.
fn coin_reads(windows: &[WindowSpec], tags: Option<&TagCtx>) -> Vec<SeriesColumn> {
    let trade: Vec<&str> = tags.map(|c| c.tags.iter().map(|t| t.name).collect()).unwrap_or_default();
    let template: Vec<&str> =
        tags.map(|c| c.tags.iter().filter(|t| t.patterns.templates().is_some()).map(|t| t.name).collect()).unwrap_or_default();
    METRICS
        .iter()
        .filter(|m| m.family != Family::Position)
        .flat_map(|m| chart_reads(m, &trade, &template, windows))
        .filter_map(|r| {
            if r.is_fingerprint_scoped() {
                tags.map(|c| SeriesColumn::tagged(r, c.fp_id))
            } else {
                Some(SeriesColumn::of(r))
            }
        })
        .collect()
}

/// Build every read's series over the token's event stream. Creation anchors at the
/// first trade (the dev-buy slot), matching the replay driver's clock, and the fold
/// runs through the shared sparse tick grid so rows land on the engine's decision grid.
fn build_series(
    mint: &str,
    trades: &[crate::sweep::projection::CorpusTrade],
    windows: &[WindowSpec],
    grid: &SparseGrid,
    tags: Option<&TagCtx>,
    creator: Option<u64>,
    entry: Option<(Ts, f64)>,
) -> serde_json::Value {
    let columns = coin_reads(windows, tags);
    let created_at = trades[0].block_time;
    let mut series = MetricSeries::new(created_at, columns.clone());
    // Tags and the creator BEFORE the first fold, in the live `TokenCreated` order.
    if let Some(c) = tags {
        register_tags(&mut series, &c.tags);
    }
    if let Some(h) = creator {
        series.seed_creator(h);
    }
    // `as_of` = now: the deadness clock advances toward the request instant as a
    // `run_replay` over this token would.
    let as_of = Utc::now();
    let estimated = estimate_sparse_rows(created_at, trades.iter().map(|t| t.block_time), grid, as_of);
    let budget = (estimated > MAX_SERIES_ROWS).then_some(MAX_SERIES_ROWS);
    if budget.is_some() {
        tracing::warn!(mint, estimated, cap = MAX_SERIES_ROWS, "metric-series exceeds the row ceiling — truncating coverage in time");
    }
    let fold = fold_sparse(&mut series, created_at, trades.iter().map(|t| (to_trade_lite(t), None)), grid, as_of, budget);

    let mut out: Vec<SeriesOut> = columns
        .iter()
        .filter_map(|&col| {
            let values = series.column_values(col)?;
            Some(SeriesOut::of(col.r, values.into_iter().map(finite).collect()))
        })
        .collect();
    if let Some((entered_at, entry_price)) = entry {
        out.extend(build_position_series(&series, entered_at, entry_price));
    }
    let price: Vec<Option<f64>> = series.price.iter().map(|p| finite(*p)).collect();
    serde_json::json!({
        "mint_address": mint,
        "at": series.at,
        "price": price,
        "series": out,
        // Coverage, never silent: `truncated` says the row ceiling cut the series
        // short, `covered_until` is the last instant it reaches.
        "truncated": fold.truncated,
        "covered_until": fold.covered_until,
    })
}

/// Every `m_position` read over a series, anchored on the inspected run's entry fill,
/// the way the fold keeps a held position: peak and trough seed at the entry price and
/// ratchet on each print from the entry on, `room_taken_pct` reads the depth at the
/// last row at or before the fill, and every read is `null` before the entry. The
/// stage clock reads from the entry (a chart has no stage moves).
fn build_position_series(series: &MetricSeries, entered_at: Ts, entry_price: f64) -> Vec<SeriesOut> {
    let n = series.n_rows();
    let reads: Vec<MetricRef> =
        METRICS.iter().filter(|m| m.family == Family::Position).flat_map(|m| chart_reads(m, &[], &[], &[])).collect();
    let mut cols: Vec<Vec<Option<f64>>> = vec![Vec::with_capacity(n); reads.len()];
    let mut ctx = PositionCtx::at_fill(entry_price, entered_at);
    for i in 0..n {
        let at = series.at[i];
        if at <= entered_at {
            ctx.entry_priced_reserve = series.priced_reserve_sol[i];
        }
        if at < entered_at {
            cols.iter_mut().for_each(|c| c.push(None));
            continue;
        }
        let price = series.price[i];
        ctx.fold_price(price);
        for (c, r) in cols.iter_mut().zip(&reads) {
            c.push(finite(position_value(r.metric, &ctx, price, at)));
        }
    }
    reads.into_iter().zip(cols).map(|(r, values)| SeriesOut::of(r, values)).collect()
}

/// A finite value as `Some` (non-finite → `null`, the pane's "no value").
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

    fn col<'a>(out: &'a [SeriesOut], metric: &str) -> &'a [Option<f64>] {
        &out.iter().find(|s| s.metric == metric).expect("metric present").values
    }

    #[test]
    fn position_series_blanks_before_entry_then_tracks_the_held_window() {
        // t=0 pre-entry, t=10 entry at 1.0, t=20 run-up to 1.5, t=30 pullback to 1.2.
        let mut s = MetricSeries::new(ts(0), Vec::new());
        for (p, secs) in [(1.0, 0), (1.0, 10), (1.5, 20), (1.2, 30)] {
            s.push_trade(trade(p, secs));
        }
        let out = build_position_series(&s, ts(10), 1.0);
        let (pnl, retrace, bounce, held) = (
            col(&out, "m_position.pnl_pct"),
            col(&out, "m_position.retrace_pct"),
            col(&out, "m_position.bounce_pct"),
            col(&out, "m_position.held_sec"),
        );
        assert_eq!((pnl[0], retrace[0], bounce[0], held[0]), (None, None, None, None));
        assert_eq!((pnl[1], retrace[1], bounce[1], held[1]), (Some(0.0), Some(0.0), Some(0.0), Some(0.0)));
        assert_eq!((pnl[2], retrace[2], bounce[2], held[2]), (Some(50.0), Some(0.0), Some(50.0), Some(10.0)));
        assert!((pnl[3].unwrap() - 20.0).abs() < 1e-9);
        assert!((retrace[3].unwrap() - 20.0).abs() < 1e-9);
        assert!((bounce[3].unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(held[3], Some(20.0));
    }

    #[test]
    fn position_series_tracks_bounce_off_the_since_entry_trough() {
        let mut s = MetricSeries::new(ts(0), Vec::new());
        for (p, secs) in [(1.0, 0), (0.8, 10), (1.0, 20)] {
            s.push_trade(trade(p, secs));
        }
        let out = build_position_series(&s, ts(0), 1.0);
        let bounce = col(&out, "m_position.bounce_pct");
        assert_eq!((bounce[0], bounce[1]), (Some(0.0), Some(0.0)));
        assert!((bounce[2].unwrap() - 25.0).abs() < 1e-9);
    }

    fn corpus_trade(sol: f64, wallet: &str, labels: Option<&str>, secs: i64) -> crate::sweep::projection::CorpusTrade {
        crate::sweep::projection::CorpusTrade {
            // Resolved at load in production (`duck.rs` / `project_pg_tail`).
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
            tx_index: 0,
            leg_index: 0,
            is_buy: true,
            on_curve: true,
            venue_fee_bps: None,
            tx_signature: None,
            ix_labels: labels.map(Box::from),
            wallet: Some(Box::from(wallet)),
        }
    }

    /// Last finite value of the column labelled `label`.
    fn last(resp: &serde_json::Value, label: &str) -> f64 {
        let col = resp["series"]
            .as_array()
            .expect("series array")
            .iter()
            .find(|s| s["label"] == label)
            .unwrap_or_else(|| panic!("{label} present"));
        col["values"].as_array().unwrap().iter().rev().find_map(serde_json::Value::as_f64).unwrap_or_else(|| panic!("{label} has a value"))
    }

    fn volume_ctx() -> TagCtx {
        let doc = serde_json::json!({ "volume": { "match": { "ix_shape": [["Pump.Fun: Create", "Pump.Fun: Buy"]], "creator": true } } });
        TagCtx { fp_id: FingerprintId(Uuid::new_v4()), tags: compile_tags(&doc) }
    }

    /// The creator seed: with `creator: true` the dev's buy carries the tag with no
    /// matching ix labels, as the live engine folds it. Unseeded, the dev's SOL lands on
    /// the other side and the pane disagrees with every decision.
    #[test]
    fn the_creator_is_tagged_only_when_seeded() {
        let trades = [
            corpus_trade(5.0, "dev", None, 0),
            corpus_trade(3.0, "normie", None, 1),
            corpus_trade(2.0, "bot", Some(r#"["Pump.Fun: Create","Pump.Fun: Buy"]"#), 2),
        ];
        let grid = SparseGrid::for_windows(&[10.0]);
        let ctx = volume_ctx();
        let w = [WindowSpec::secs(10.0)];
        let seeded = build_series("mint", &trades, &w, &grid, Some(&ctx), Some(wallet_hash("dev")), None);
        assert_eq!(last(&seeded, "m_flow.buy_sol @volume"), 7.0, "dev + pattern bot");
        assert_eq!(last(&seeded, "m_flow.buy_sol @!volume"), 3.0, "the stranger only");
        let unseeded = build_series("mint", &trades, &w, &grid, Some(&ctx), None, None);
        assert_eq!(last(&unseeded, "m_flow.buy_sol @volume"), 2.0);
        assert_eq!(last(&unseeded, "m_flow.buy_sol @!volume"), 8.0);
    }

    /// The extensibility contract: a metric added to the registry appears here with no
    /// change to this file — a metric missing from the response does not exist on the
    /// chart, silently.
    #[test]
    fn every_registry_metric_is_a_column() {
        let trades = [corpus_trade(5.0, "dev", None, 0), corpus_trade(3.0, "normie", None, 1)];
        let grid = SparseGrid::for_windows(&[10.0, 30.0]);
        let mut ctx = volume_ctx();
        ctx.tags.extend(compile_tags(&serde_json::json!({ "working": { "match": { "ix_template": ["Pump.Fun|200000|0|1|0|0"] } } })));
        // Two spans, so the two-window reads have a nested pair.
        let w = [WindowSpec::secs(30.0), WindowSpec::secs(10.0)];
        let out = build_series("mint", &trades, &w, &grid, Some(&ctx), Some(wallet_hash("dev")), Some((ts(0), 1.0)));
        let paths: Vec<&str> = out["series"].as_array().unwrap().iter().map(|c| c["metric"].as_str().unwrap()).collect();
        for m in METRICS {
            assert!(paths.contains(&m.path().as_str()), "{} has no column", m.path());
        }
        let tagged_template = METRICS.iter().any(|m| m.tag_level == TagLevel::Template);
        assert!(tagged_template, "the template tag fixture has something to cover");
    }

    #[test]
    fn the_endpoint_declares_it_needs_wallet_identity() {
        assert!(records_wallet_keyed_metric());
    }
}
