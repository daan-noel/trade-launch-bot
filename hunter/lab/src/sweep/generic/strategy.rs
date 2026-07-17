//! `GenericSweepStrategy` — the redesigned engine's [`Strategy`] implementation
//! (plan §5.4). Replaces the three per-strategy wrappers with one precompute-then-
//! scan sweep:
//!
//! * **precompute** ([`prepare_token`](GenericSweepStrategy::prepare_token)) — one
//!   replay pass over a token builds a [`MetricSeries`] carrying every metric
//!   column the axes reference (plus the rule-independent price / reserves / dead
//!   verdict per event). This mirrors, event-for-event, the single-token stream
//!   `lab::strategies::replay` folds through the live `reduce` — same 500 ms tick
//!   grid, same tail, same `TradeLite` mapping — so the scan reads exactly the
//!   values the live engine would see.
//! * **scan** ([`resolve_entry`](Strategy::resolve_entry) /
//!   [`resolve_exit`](Strategy::resolve_exit)) — per combo, a cheap walk over the
//!   precomputed series applying the **same** `hunter_engine` decision logic the
//!   fold uses: armed-side `Dead > Unsatisfiable > Enter`; open-side
//!   `Dead > StopLoss > TakeProfit > Metrics`; caps do not apply (the sweep judges
//!   each token independently, as the legacy sweep always has).
//!
//! Guard test [`super::guard`] asserts this scan ≡ a full `run_replay` on a sample
//! corpus (plan decision 13 / step 5.5) — the drift lock that lets the fast scan
//! stand in for the full engine.

use chrono::{DateTime, Duration, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::deadness::DEAD_QUIET_SECS;
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::eval;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{MetricId, Side, TradeLite, Ts};
use hunter_engine::TICK_MS;

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{round_trip_with_costs, CostModel, ExitCode};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod, TokenOutcome};
use crate::strategies::replay::TAIL_MARGIN_SECS;

use super::axes::AxesModel;

/// One swept combo: its grid index (for refine / entry-key math) and the
/// pre-chewed [`CompiledRule`] the scan reads. The `RuleParams` JSON is derived
/// from the index on demand ([`GenericSweepStrategy::params_json`]).
#[derive(Clone)]
pub struct GenericCombo {
    pub idx: usize,
    pub compiled: CompiledRule,
}

/// The generic engine as a sweep [`Strategy`]. Holds the resolved axes model, the
/// notional every round-trip is priced at, and the deadness "now".
pub struct GenericSweepStrategy {
    model: AxesModel,
    buy_amount_sol: f64,
    as_of: Ts,
    cost: CostModel,
    /// The union of every axis's precompute columns (built once).
    columns: Vec<SeriesColumn>,
}

impl GenericSweepStrategy {
    /// Build the strategy from a resolved axes model. `as_of` is the run-time now
    /// the deadness clock advances toward (matches `run_replay`'s `ReplayConfig`).
    pub fn new(model: AxesModel, buy_amount_sol: f64, as_of: Ts) -> Self {
        let columns = model.columns();
        Self { model, buy_amount_sol, as_of, cost: CostModel::pumpfun_default(), columns }
    }

    /// Compile combo `idx` into a `CompiledRule` (dummy fingerprint + unlimited
    /// caps — the sweep judges entry/exit conditions per token, never concurrency).
    fn compile_combo(&self, idx: usize) -> CompiledRule {
        let params = self.model.combo_params(idx);
        let loaded = LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: FingerprintId(Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: sol_to_lamports(self.buy_amount_sol).max(0) as u64,
            // Unlimited caps: a sweep never models cross-token concurrency.
            max_concurrent_tokens: u32::MAX,
            max_total_tokens: 0,
            params,
        };
        CompiledRule::compile(&loaded)
    }

    fn combo(&self, idx: usize) -> GenericCombo {
        GenericCombo { idx, compiled: self.compile_combo(idx) }
    }

    /// Per-axis value counts, in combo-significance order (index 0 most significant).
    fn axis_lens(&self) -> Vec<usize> {
        self.model.axes.iter().map(axis_value_count).collect()
    }
}

/// Value count of one resolved axis (mirror of `ResolvedAxis::len`, kept local to
/// avoid widening the axis module's surface).
fn axis_value_count(a: &super::axes::ResolvedAxis) -> usize {
    use super::axes::ResolvedAxis::*;
    match a {
        Metric { values, .. } => values.len(),
        TakeProfit { values } | StopLoss { values } => values.len(),
    }
}

impl ParamSpace for GenericSweepStrategy {
    type Params = GenericCombo;

    fn sample(&self, method: SweepMethod) -> Vec<Self::Params> {
        let total = self.model.combo_count();
        if total == 0 {
            return Vec::new();
        }
        match method {
            SweepMethod::Grid => (0..total).map(|i| self.combo(i)).collect(),
            SweepMethod::Random { n, seed } => {
                if n >= total {
                    return (0..total).map(|i| self.combo(i)).collect();
                }
                let mut rng = StdRng::seed_from_u64(seed);
                let mut seen = std::collections::HashSet::with_capacity(n);
                let mut out = Vec::with_capacity(n);
                while out.len() < n {
                    let i = rng.gen_range(0..total);
                    if seen.insert(i) {
                        out.push(self.combo(i));
                    }
                }
                out
            }
            SweepMethod::LatinHypercube { n, seed } => {
                let lens = self.axis_lens();
                let mut rng = StdRng::seed_from_u64(seed);
                let plan = crate::sweep::strategy::lhs_index_plan(&mut rng, n.min(total), &lens);
                let draws = plan.first().map(|c| c.len()).unwrap_or(0);
                let mut seen = std::collections::HashSet::with_capacity(draws);
                let mut out = Vec::with_capacity(draws);
                // `d` indexes every axis column `plan[a][d]`, so a range loop is the
                // natural shape here (not a single-slice iteration).
                #[allow(clippy::needless_range_loop)]
                for d in 0..draws {
                    // Reassemble the flat combo index from this draw's per-axis picks.
                    let mut idx = 0usize;
                    for (a, &len) in lens.iter().enumerate() {
                        idx = idx * len.max(1) + plan[a][d];
                    }
                    if seen.insert(idx) {
                        out.push(self.combo(idx));
                    }
                }
                out
            }
        }
    }

    fn refine(&self, survivors: &[Self::Params]) -> Vec<Self::Params> {
        let lens = self.axis_lens();
        let mut out = Vec::new();
        for s in survivors {
            // Decode this survivor's per-axis picks, then step each axis to its
            // adjacent candidate values, holding the others fixed.
            let picks = decode_picks(s.idx, &lens);
            for (a, &len) in lens.iter().enumerate() {
                for nb in crate::sweep::strategy::neighbor_indices(picks[a], len) {
                    let mut p = picks.clone();
                    p[a] = nb;
                    let idx = encode_picks(&p, &lens);
                    out.push(self.combo(idx));
                }
            }
        }
        out
    }

    fn order_for_entry_cache(&self, params: &mut [Self::Params]) {
        // Stable-sort by entry-key so same-entry combos are contiguous under any
        // sampler (Random/LHS/refine shuffle the grid order).
        params.sort_by_key(|c| self.model.entry_key(c.idx));
    }
}

impl Strategy for GenericSweepStrategy {
    type Entry = EntryResolution;
    type EntryKey = u64;
    type TokenState = MetricSeries;

    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey {
        self.model.entry_key(params.idx)
    }

    fn prepare_token(&self, token: &CorpusToken) -> Self::TokenState {
        build_series(token, self.columns.clone(), self.as_of)
    }

    fn resolve_entry(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        params: &Self::Params,
    ) -> Self::Entry {
        resolve_entry(series, &params.compiled)
    }

    fn resolve_exit(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        entry: &Self::Entry,
        params: &Self::Params,
    ) -> TokenOutcome {
        resolve_exit(series, &params.compiled, entry, self.buy_amount_sol, &self.cost)
    }

    fn params_json(&self, params: &Self::Params) -> serde_json::Value {
        self.model.combo_params(params.idx).to_value()
    }
}

// ───────────────────────────── decode / encode ─────────────────────────────

fn decode_picks(mut idx: usize, lens: &[usize]) -> Vec<usize> {
    let mut picks = vec![0usize; lens.len()];
    for (a, &len) in lens.iter().enumerate().rev() {
        let radix = len.max(1);
        picks[a] = idx % radix;
        idx /= radix;
    }
    picks
}

fn encode_picks(picks: &[usize], lens: &[usize]) -> usize {
    let mut idx = 0usize;
    for (a, &len) in lens.iter().enumerate() {
        idx = idx * len.max(1) + picks[a];
    }
    idx
}

// ───────────────────────────── precompute ──────────────────────────────────

/// Build one token's [`MetricSeries`] over the same event stream a single-token
/// `run_replay` folds: trades interleaved with 500 ms ticks on a grid anchored at
/// `created_at + TICK`, then a tail up to `min(as_of, last_trade + DEAD_QUIET +
/// TAIL_MARGIN)`. Trades map to `TradeLite` exactly as `replay::load_tokens` does
/// (REAL reserves for deadness parity; canonical spot price).
///
/// Trades are folded in the corpus's load order (slot → tx_index → leg), which is
/// block-time-monotonic — the order `run_replay` also folds them in (its global
/// `at` sort is stable and block_time tracks slot), so the two never diverge.
pub(crate) fn build_series(token: &CorpusToken, columns: Vec<SeriesColumn>, as_of: Ts) -> MetricSeries {
    let created = token.created_at;
    let mut series = MetricSeries::new(created, columns);
    let trades = &token.trades;
    if trades.is_empty() {
        return series;
    }
    let tick = Duration::milliseconds(TICK_MS);
    let mut next_tick = created + tick;
    let mut last_trade_at = created;
    for ct in trades.iter() {
        let at = ct.block_time;
        while next_tick < at {
            series.push_tick(next_tick);
            next_tick += tick;
        }
        series.push_trade(trade_lite(ct));
        if at > last_trade_at {
            last_trade_at = at;
        }
    }
    // Tail: keep ticking so a quiet token books its dead verdict, but no further
    // than the window past which every token is provably dead + pruned.
    let cap = last_trade_at + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS);
    let tail_end = as_of.min(cap);
    while next_tick < tail_end {
        series.push_tick(next_tick);
        next_tick += tick;
    }
    series
}

fn trade_lite(ct: &CorpusTrade) -> TradeLite {
    TradeLite {
        side: if ct.is_buy { Side::Buy } else { Side::Sell },
        sol: ct.amount_sol,
        price: ct.price_per_token,
        // Deadness/liquidity read REAL reserves (SSOT parity with live); absent ⇒
        // NaN (no snapshot ⇒ alive) — identical to `replay::load_tokens`.
        reserve_sol: ct.real_reserve_sol.unwrap_or(f64::NAN),
        at: ct.block_time,
    }
}

/// The union of precompute columns a compiled rule reads (both sides). Used by the
/// guard test to build a series for one rule without the axes model.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn columns_for(compiled: &CompiledRule) -> Vec<SeriesColumn> {
    let mut cols = Vec::new();
    for req in compiled.entry_reqs.iter().chain(compiled.exit_reqs.iter()) {
        let c = col_of(req.metric, req.window);
        if !cols.contains(&c) {
            cols.push(c);
        }
    }
    cols
}

fn col_of(metric: MetricId, window: Option<f64>) -> SeriesColumn {
    match window {
        Some(w) => SeriesColumn::Window(metric, w),
        None => SeriesColumn::Static(metric),
    }
}

// ─────────────────────────────── scan ──────────────────────────────────────

/// The resolved entry for one combo on one token.
#[derive(Clone, Copy, Debug)]
pub enum EntryResolution {
    /// Never armed→entered: dead/unsatisfiable before entry, or no fill price.
    NoEntry,
    /// Entered — the fill landed at series row `fill_row`.
    Entered { fill_row: usize, price: f64, at: Ts },
}

/// Read a metric requirement's precomputed value at `row` (`NaN` if the column
/// wasn't recorded — never satisfies a condition).
fn req_value(series: &MetricSeries, metric: MetricId, window: Option<f64>, row: usize) -> f64 {
    let target = col_of(metric, window);
    match series.columns().iter().position(|c| *c == target) {
        Some(idx) => series.rows[row][idx],
        None => f64::NAN,
    }
}

fn entry_satisfied(series: &MetricSeries, c: &CompiledRule, row: usize) -> bool {
    c.entry_reqs
        .iter()
        .all(|r| eval(&r.conds, req_value(series, r.metric, r.window, row), r.tolerance))
}

fn exit_metrics_satisfied(series: &MetricSeries, c: &CompiledRule, row: usize) -> bool {
    c.exit_reqs
        .iter()
        .all(|r| eval(&r.conds, req_value(series, r.metric, r.window, row), r.tolerance))
}

fn entry_unsatisfiable(series: &MetricSeries, c: &CompiledRule, row: usize) -> bool {
    c.mono_bounds
        .iter()
        .any(|mb| mb.crossed(req_value(series, mb.metric, mb.window, row)))
}

/// Walk the series to the entry decision, mirroring the armed-side `decide_arm`:
/// `Dead > Unsatisfiable > (enter-on-arm | entry conditions)`. The fill lands at
/// the first row with a finite price at or after the decision (an enter-on-arm or
/// tick-timed decision before the first print defers to that print — exactly the
/// engine's `pending_buys` wait-for-price).
pub(crate) fn resolve_entry(series: &MetricSeries, c: &CompiledRule) -> EntryResolution {
    let n = series.rows.len();
    for i in 0..n {
        if series.dead[i] {
            return EntryResolution::NoEntry;
        }
        if entry_unsatisfiable(series, c, i) {
            return EntryResolution::NoEntry;
        }
        if c.enter_on_arm() || entry_satisfied(series, c, i) {
            // Fill at the first finite, positive price at or after the decision.
            for j in i..n {
                let p = series.price[j];
                if p.is_finite() && p > 0.0 {
                    return EntryResolution::Entered { fill_row: j, price: p, at: series.at[j] };
                }
            }
            return EntryResolution::NoEntry; // never priced ⇒ never filled
        }
    }
    EntryResolution::NoEntry
}

/// Walk from the entry fill to the exit decision, mirroring the open-side
/// `decide_arm`: `Dead > StopLoss > TakeProfit > Metrics`. No exit by the tail ⇒
/// `Open`, marked to the last known price.
pub(crate) fn resolve_exit(
    series: &MetricSeries,
    c: &CompiledRule,
    entry: &EntryResolution,
    buy_amount_sol: f64,
    cost: &CostModel,
) -> TokenOutcome {
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    let n = series.rows.len();
    for j in (fill_row + 1)..n {
        if series.dead[j] {
            return closed(ExitCode::Dead, entry_price, entry_at, series.price[j], series.at[j], buy_amount_sol, cost);
        }
        let p = series.price[j];
        if p.is_finite() {
            if let Some(sl) = c.stop_loss {
                if p <= entry_price * (1.0 - sl / 100.0) {
                    return closed(ExitCode::StopLoss, entry_price, entry_at, p, series.at[j], buy_amount_sol, cost);
                }
            }
            if let Some(tp) = c.take_profit {
                if p >= entry_price * (1.0 + tp / 100.0) {
                    return closed(ExitCode::TakeProfit, entry_price, entry_at, p, series.at[j], buy_amount_sol, cost);
                }
            }
        }
        if c.has_exit_metrics() && exit_metrics_satisfied(series, c, j) {
            return closed(ExitCode::Metrics, entry_price, entry_at, p, series.at[j], buy_amount_sol, cost);
        }
    }
    // Open: mark to the last finite price (unrealized — excluded from the realized
    // stats by `RunAgg`, but priced for the drill-in / row view).
    let last_price = (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
    let (pnl_sol, pnl_pct) = round_trip_with_costs(entry_price, last_price, buy_amount_sol, cost);
    TokenOutcome {
        fired: true,
        holding_secs: 0,
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::Open,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: None,
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

/// The scan as one call (entry then exit) — the guard test's per-token driver.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn scan(
    series: &MetricSeries,
    c: &CompiledRule,
    buy_amount_sol: f64,
    cost: &CostModel,
) -> TokenOutcome {
    let entry = resolve_entry(series, c);
    resolve_exit(series, c, &entry, buy_amount_sol, cost)
}

#[allow(clippy::too_many_arguments)]
fn closed(
    exit: ExitCode,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    exit_price: f64,
    exit_at: DateTime<Utc>,
    buy_amount_sol: f64,
    cost: &CostModel,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = round_trip_with_costs(entry_price, exit_price, buy_amount_sol, cost);
    TokenOutcome {
        fired: true,
        holding_secs: (exit_at - entry_at).num_seconds(),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: None,
        exit_time: Some(exit_at),
        exit_price: Some(exit_price),
        exit_slot: None,
    }
}
