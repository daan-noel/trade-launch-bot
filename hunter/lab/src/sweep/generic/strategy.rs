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

/// One swept combo: **index only** into the axes grid. The [`CompiledRule`] is
/// bound once per combo batch via [`Strategy::bind_param`] — never resident × N
/// up front (that was the multi-GB cliff at ~1M combos).
#[derive(Clone, Copy, Debug)]
pub struct GenericCombo {
    pub idx: usize,
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
    /// The sparse-grid horizons derived from the swept axes — sizes the precompute
    /// so a long-lived token records rows only where a decision could change
    /// (plan §P2). Built once; the same grid serves every token.
    grid: SparseGrid,
}

impl GenericSweepStrategy {
    /// Build the strategy from a resolved axes model. `as_of` is the run-time now
    /// the deadness clock advances toward (matches `run_replay`'s `ReplayConfig`).
    pub fn new(model: AxesModel, buy_amount_sol: f64, as_of: Ts) -> Self {
        let columns = model.columns();
        let grid = SparseGrid {
            max_window_secs: model.max_window_secs(),
            time_horizon_secs: model.metric_value_ceiling(MetricId::Time),
            stall_horizon_secs: model.metric_value_ceiling(MetricId::Stall),
        };
        Self { model, buy_amount_sol, as_of, cost: CostModel::pumpfun_default(), columns, grid }
    }

    /// Estimate the worst-case resident bytes of one token's precomputed series —
    /// the admission guard's per-token unit (plan §P4). A row costs `n_cols` f64s
    /// in the flat buffer plus the `at`/`price`/`reserve_sol`/`dead` parallel vecs.
    pub fn series_bytes_estimate(&self, token: &CorpusToken) -> usize {
        let rows = estimate_sparse_rows(token, &self.grid, self.as_of);
        let per_row = self.columns.len() * std::mem::size_of::<f64>()
            + std::mem::size_of::<Ts>()   // at
            + 2 * std::mem::size_of::<f64>() // price + reserve_sol
            + std::mem::size_of::<bool>(); // dead
        rows.saturating_mul(per_row)
    }

    /// Compile combo `idx` into a `CompiledRule` (dummy fingerprint + unlimited
    /// caps — the sweep judges entry/exit conditions per token, never concurrency).
    ///
    /// **Deliberately cap-free, and this is a semantic difference from a
    /// single-rule simulate — not an oversight** (parity plan B5). A sweep scores
    /// each token *independently*, which is what lets the fold run as a parallel
    /// per-token scan over a sparse precomputed series; honoring
    /// `max_concurrent_tokens` would require one globally time-ordered fold across
    /// the whole corpus (what `replay::run_replay` does) and would serialize the
    /// very thing the sweep's performance design is built on.
    ///
    /// So the two answer different questions and the UI must not present them as
    /// interchangeable: a sweep reports a combo's **raw per-token edge** (every
    /// qualifying token taken), while a simulate reports **what the rule would
    /// actually have captured** through its concurrency/total slots. A capped rule
    /// therefore fires on strictly more tokens here, and its `n_fired` /
    /// `total_pnl_sol` are upper bounds on the simulated figures. The sweep view
    /// labels this; see `GenericSweepView`'s summary note.
    ///
    /// Same caveat for the notional: `buy_amount_sol` defaults to
    /// [`SWEEP_DEFAULT_BUY_AMOUNT_SOL`](crate::sweep::registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL)
    /// because a sweep explores many candidate combos rather than replaying one
    /// saved rule — there is frequently no rule to inherit it from. Since the cost
    /// model charges a *fixed* per-leg cost, PnL% is not notional-invariant, so
    /// compare a sweep against a simulate only when both were sized the same.
    fn compile_combo(&self, idx: usize) -> CompiledRule {
        let params = self.model.combo_params(idx);
        let loaded = LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: FingerprintId(Uuid::nil()),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: sol_to_lamports(self.buy_amount_sol).max(0) as u64,
            // Unlimited caps — see the doc above; changing this changes the meaning
            // of every sweep number, it does not "fix" a parity bug.
            max_concurrent_tokens: u32::MAX,
            max_total_tokens: 0,
            params,
        };
        CompiledRule::compile(&loaded)
    }

    fn combo(&self, idx: usize) -> GenericCombo {
        GenericCombo { idx }
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
        if total == usize::MAX {
            tracing::error!("combo_count overflowed — refusing to sample");
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
    type BoundParams = CompiledRule;

    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey {
        self.model.entry_key(params.idx)
    }

    fn bind_param(&self, params: &Self::Params) -> Self::BoundParams {
        self.compile_combo(params.idx)
    }

    fn prepare_token(&self, token: &CorpusToken) -> Self::TokenState {
        build_series(token, self.columns.clone(), &self.grid, self.as_of)
    }

    fn resolve_entry(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
    ) -> Self::Entry {
        resolve_entry(series, bound)
    }

    fn resolve_exit(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        _params: &Self::Params,
    ) -> TokenOutcome {
        resolve_exit(series, bound, entry, self.buy_amount_sol, &self.cost)
    }

    fn params_json(&self, params: &Self::Params) -> serde_json::Value {
        self.model.combo_params(params.idx).to_value()
    }

    fn token_state_bytes_estimate(&self, token: &CorpusToken) -> usize {
        self.series_bytes_estimate(token)
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

/// The horizons that size a token's **sparse** tick grid (plan §P2). Between two
/// trades the token state is almost entirely static — price, reserves, liquidity
/// and trail are constant; only window flows (until a trade ages out of the
/// largest window), the monotone `time`/`stall` clocks, and the one-shot dead
/// verdict can change. So within a gap we only need dense 500 ms ticks up to the
/// last instant any of those could still flip a swept condition; past it every
/// tick is provably identical to its predecessor and is omitted. Every emitted
/// tick lands on the same `created + k·TICK` grid the dense series used, so its
/// values are bit-identical — the scan can never disagree with a full replay.
#[derive(Clone, Copy, Debug, Default)]
pub struct SparseGrid {
    /// Largest registered flow window (secs); `0` if the rule reads no flows. A
    /// trade keeps changing flows until `trade + this`, after which all flows are 0.
    pub max_window_secs: f64,
    /// Max swept `time` condition value + its `=`-tolerance (secs since creation);
    /// `0` if `time` isn't swept. `time` is monotone, so past this every `time`
    /// condition is settled forever.
    pub time_horizon_secs: f64,
    /// Max swept `stall` condition value + its `=`-tolerance (secs). `stall` grows
    /// monotonically within a gap, so past `last_trade + this` every `stall`
    /// condition is settled for that gap.
    pub stall_horizon_secs: f64,
}

impl SparseGrid {
    /// Hard ceiling on sparse-grid horizons (secs). A fat-fingered `time`/`stall`/
    /// window axis (e.g. 1e12) must not turn `gap_horizon` into DateTime::MAX and
    /// emit centuries of 500 ms ticks (the alloc that printed ~419 TB).
    const MAX_HORIZON_SECS: f64 = 7.0 * 24.0 * 3600.0;

    fn clamp_secs(s: f64) -> f64 {
        if !s.is_finite() || s <= 0.0 {
            0.0
        } else {
            s.min(Self::MAX_HORIZON_SECS)
        }
    }

    /// The last grid instant in a gap at which any swept condition could still
    /// change, given the trade state entering the gap: `last_trade_at` (the newest
    /// folded trade, ≥ every metric clock's origin) and `last_meaningful_at` (the
    /// deadness clock). Dense ticks are emitted up to here; ticks past it are all
    /// provably static and skipped.
    fn gap_horizon(&self, created: Ts, last_trade_at: Ts, last_meaningful_at: Ts) -> Ts {
        let secs = |s: f64| {
            let ms = (Self::clamp_secs(s) * 1000.0).ceil();
            // chrono::Duration::milliseconds panics / overflows on absurd inputs —
            // clamp to i64::MAX/2 ms (~3e8 years still, but finite adds).
            let ms_i = if ms.is_finite() {
                ms.min((i64::MAX / 2) as f64) as i64
            } else {
                0
            };
            Duration::milliseconds(ms_i.max(0))
        };
        let mut h = last_trade_at;
        // Flow decay: a trade influences the largest window until `trade + window`.
        h = h.max(last_trade_at + secs(self.max_window_secs));
        // `time` is measured from creation.
        h = h.max(created + secs(self.time_horizon_secs));
        // `stall` is measured from the last all-time high (≤ last_trade_at); using
        // last_trade_at is a safe upper bound (it only over-emits, never drops).
        h = h.max(last_trade_at + secs(self.stall_horizon_secs));
        // Dead flips once, at the first tick past the quiet window.
        h = h.max(last_meaningful_at + Duration::seconds(DEAD_QUIET_SECS));
        h
    }
}

/// Build one token's [`MetricSeries`] over the same event stream a single-token
/// `run_replay` folds: trades interleaved with 500 ms ticks on a grid anchored at
/// `created_at + TICK`, then a tail up to `min(as_of, last_trade + DEAD_QUIET +
/// TAIL_MARGIN)`. Trades map to `TradeLite` exactly as `replay::load_tokens` does
/// (REAL reserves for deadness parity; canonical spot price).
///
/// Ticks are emitted **sparsely** ([`SparseGrid`]): every trade, plus grid ticks
/// only up to each gap's [`gap_horizon`](SparseGrid::gap_horizon). Omitted ticks
/// are provably identical (w.r.t. the swept conditions) to the last emitted row,
/// so the scan reads the same decisions a dense series would — but a week-long
/// token records rows ∝ its trades, not ∝ its wall-clock lifespan.
///
/// Trades are folded in the corpus's load order (slot → tx_index → leg), which is
/// block-time-monotonic — the order `run_replay` also folds them in (its global
/// `at` sort is stable and block_time tracks slot), so the two never diverge.
pub(crate) fn build_series(
    token: &CorpusToken,
    columns: Vec<SeriesColumn>,
    grid: &SparseGrid,
    as_of: Ts,
) -> MetricSeries {
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
        // Gap before this trade: dense ticks only up to the gap horizon, then jump
        // straight to the trade (skipping the provably-static remainder).
        let horizon = grid.gap_horizon(created, last_trade_at, series.last_meaningful_at());
        emit_gap_ticks(&mut series, &mut next_tick, at, horizon, tick, created);
        series.push_trade_at(trade_lite(ct), Some(ct.slot));
        if at > last_trade_at {
            last_trade_at = at;
        }
    }
    // Tail: keep ticking so a quiet token books its dead verdict, but no further
    // than the window past which every token is provably dead + pruned.
    //
    // **Known bounded-tail approximation (parity plan B8).** The justification —
    // "past this the token is provably dead" — holds only for a token that lost its
    // liquidity. A token that goes quiet while still liquid never books `Dead`, yet
    // its monotone `time`/`stall` clocks keep running in reality: live would tick it
    // indefinitely and fire, say, an `exit on time > 2h` long after this cap. So for
    // that shape the sweep can report `Open` where live/simulate exit.
    //
    // Left as-is deliberately. `replay::run_replay` bounds its tail by the
    // **corpus-wide** last trade, so a single-token replay truncates at exactly this
    // same point (which is why `guard::scan_matches_replay_*` passes) — but a
    // multi-token simulate ticks longer and lands closer to live. Fixing the
    // asymmetry by truncating simulate per-token would move simulate *away* from
    // live, which is the wrong direction; fixing it by extending this tail costs the
    // memory the sparse-grid design exists to bound. Don't "align" the two without
    // deciding which one should move — and it isn't this one.
    //
    // Prior attempt, for the record: extending `tail_end` to `dead_cap.max(horizon)`
    // here makes the scan fire exits a single-token replay never does, and
    // `guard::scan_matches_replay_stall_eq_exit_across_gap` fails immediately.
    let cap = last_trade_at + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS);
    let tail_end = as_of.min(cap);
    let horizon = grid.gap_horizon(created, last_trade_at, series.last_meaningful_at());
    emit_gap_ticks(&mut series, &mut next_tick, tail_end, horizon, tick, created);
    series
}

/// Emit grid ticks in `[next_tick, stop)` but no further than `horizon`; then
/// fast-forward `next_tick` onto the grid to the first tick ≥ `stop` **without**
/// emitting the settled ticks in between (that arithmetic jump is what makes the
/// grid sparse over long quiet gaps). Post-state matches the old dense loop's
/// (`next_tick` = smallest grid tick ≥ `stop`) so the caller's next gap is unaffected.
///
/// Hard-capped at [`MAX_TICKS_PER_GAP`] so a corrupt timestamp / horizon cannot
/// push billions of rows and abort with a multi-hundred-TB allocation.
fn emit_gap_ticks(
    series: &mut MetricSeries,
    next_tick: &mut Ts,
    stop: Ts,
    horizon: Ts,
    tick: Duration,
    created: Ts,
) {
    /// ~2 days of 500 ms ticks — beyond the sparse-grid horizon clamp.
    const MAX_TICKS_PER_GAP: usize = 2 * 24 * 3600 * 2;
    let mut emitted = 0usize;
    while *next_tick < stop && *next_tick <= horizon {
        if emitted >= MAX_TICKS_PER_GAP {
            // Jump to stop without further emits — decisions past the clamp are
            // identical for settled monotone clocks under the horizon cap.
            break;
        }
        series.push_tick(*next_tick);
        *next_tick += tick;
        emitted += 1;
    }
    if *next_tick < stop {
        // Stopped at the horizon (or tick cap), not at `stop` — jump to the next
        // on-grid tick ≥ stop.
        let delta_ms = stop.signed_duration_since(created).num_milliseconds();
        let k = (delta_ms + TICK_MS - 1) / TICK_MS; // ceil ⇒ smallest grid tick ≥ stop
        *next_tick = created + Duration::milliseconds(k * TICK_MS);
    }
}

/// Worst-case row count of a token's sparse series — the admission estimate
/// (plan §P4). Upper-bounds each gap's dense span by the grid horizon so it never
/// under-counts (which would defeat the guard), without building the series.
pub(crate) fn estimate_sparse_rows(token: &CorpusToken, grid: &SparseGrid, as_of: Ts) -> usize {
    let trades = &token.trades;
    if trades.is_empty() {
        return 0;
    }
    let created = token.created_at;
    let tick_ms = TICK_MS.max(1);
    let horizon_ms = ((SparseGrid::clamp_secs(grid.max_window_secs)
        .max(SparseGrid::clamp_secs(grid.time_horizon_secs))
        .max(SparseGrid::clamp_secs(grid.stall_horizon_secs))
        + DEAD_QUIET_SECS as f64)
        * 1000.0)
        .ceil() as i64;
    let mut rows = 0usize;
    let mut prev = created;
    // One row per trade, plus at most `horizon_ms / tick` dense ticks per gap.
    for ct in trades.iter() {
        let gap_ms = ct.block_time.signed_duration_since(prev).num_milliseconds().max(0);
        let ticks = (gap_ms.min(horizon_ms).max(0) / tick_ms) as usize;
        rows = rows.saturating_add(1).saturating_add(ticks);
        if ct.block_time > prev {
            prev = ct.block_time;
        }
    }
    // Tail gap (bounded by DEAD_QUIET + TAIL_MARGIN as well as the run's `as_of`).
    let tail_cap = prev + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS);
    let tail_ms = as_of.min(tail_cap).signed_duration_since(prev).num_milliseconds().max(0);
    rows = rows.saturating_add((tail_ms.min(horizon_ms).max(0) / tick_ms) as usize);
    rows
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

/// The [`SparseGrid`] one compiled rule needs — the guard-test counterpart of the
/// axes-model grid the live sweep builds. Its horizons are the rule's own
/// `time`/`stall` condition ceilings (+ tolerance) and largest flow window, so the
/// sparse series it drives records every tick this rule's scan could branch on.
pub(crate) fn sparse_grid_for(compiled: &CompiledRule) -> SparseGrid {
    let max_window_secs = compiled.windows.iter().cloned().fold(0.0_f64, f64::max);
    // Max condition value + eq-tolerance for a monotone/static metric across both sides.
    let ceiling = |metric: MetricId| -> f64 {
        let mut max = 0.0_f64;
        let mut found = false;
        for req in compiled.entry_reqs.iter().chain(compiled.exit_reqs.iter()) {
            if req.metric == metric {
                for arm in &req.conds {
                    for c in arm {
                        max = max.max(c.value);
                        found = true;
                    }
                }
            }
        }
        if found { max + hunter_engine::metrics::metric_spec(metric).eq_tolerance } else { 0.0 }
    };
    SparseGrid {
        max_window_secs,
        time_horizon_secs: ceiling(MetricId::Time),
        stall_horizon_secs: ceiling(MetricId::Stall),
    }
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

/// Column index (into the flat series) resolved once per requirement, hoisted out
/// of the per-row scan loop (plan §P1). `usize::MAX` = the column wasn't recorded,
/// which reads as `NaN` and satisfies no condition.
const MISSING_COL: usize = usize::MAX;

fn col_idx_of(series: &MetricSeries, metric: MetricId, window: Option<f64>) -> usize {
    series.col_index(col_of(metric, window)).unwrap_or(MISSING_COL)
}

/// Value at a pre-resolved column index (`NaN` for an unrecorded column).
#[inline]
fn value_at_col(series: &MetricSeries, col: usize, row: usize) -> f64 {
    if col == MISSING_COL {
        f64::NAN
    } else {
        series.value_at(row, col)
    }
}

/// One side's requirements paired with their resolved column indices (built once
/// per scan, indexed in lockstep with the `CompiledRule`'s req list).
fn resolve_cols(series: &MetricSeries, reqs: &[hunter_engine::arm::MetricReq]) -> Vec<usize> {
    reqs.iter().map(|r| col_idx_of(series, r.metric, r.window)).collect()
}

/// Entry combinator: AND across metrics (mirror of `arm::reqs_satisfied`).
fn reqs_satisfied(
    series: &MetricSeries,
    reqs: &[hunter_engine::arm::MetricReq],
    cols: &[usize],
    row: usize,
) -> bool {
    reqs.iter()
        .zip(cols)
        .all(|(r, &col)| eval(&r.conds, value_at_col(series, col, row), r.tolerance))
}

/// Exit combinator: OR across metrics (mirror of `arm::reqs_any_satisfied`). Any
/// one satisfied exit metric fires; a single metric's own cond list still ANDs.
fn reqs_any_satisfied(
    series: &MetricSeries,
    reqs: &[hunter_engine::arm::MetricReq],
    cols: &[usize],
    row: usize,
) -> bool {
    reqs.iter()
        .zip(cols)
        .any(|(r, &col)| eval(&r.conds, value_at_col(series, col, row), r.tolerance))
}

fn entry_unsatisfiable(series: &MetricSeries, c: &CompiledRule, mono_cols: &[usize], row: usize) -> bool {
    c.mono_kills
        .iter()
        .zip(mono_cols)
        .any(|(k, &col)| k.permanently_false(value_at_col(series, col, row)))
}

/// Walk the series to the entry decision, mirroring the armed-side `decide_arm`:
/// `Dead > Unsatisfiable > (enter-on-arm | entry conditions)`. The fill lands at
/// the first row with a finite price at or after the decision (an enter-on-arm or
/// tick-timed decision before the first print defers to that print — exactly the
/// engine's `pending_buys` wait-for-price).
pub(crate) fn resolve_entry(series: &MetricSeries, c: &CompiledRule) -> EntryResolution {
    let n = series.n_rows();
    // Resolve each requirement's flat column index once, not per row.
    let entry_cols = resolve_cols(series, &c.entry_reqs);
    let mono_cols: Vec<usize> =
        c.mono_kills.iter().map(|k| col_idx_of(series, k.metric, k.window)).collect();
    let enter_on_arm = c.enter_on_arm();
    for i in 0..n {
        if series.dead[i] {
            return EntryResolution::NoEntry;
        }
        if entry_unsatisfiable(series, c, &mono_cols, i) {
            return EntryResolution::NoEntry;
        }
        if enter_on_arm || reqs_satisfied(series, &c.entry_reqs, &entry_cols, i) {
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
    // Slot of the real trade the entry fills against — the first trade at/after the
    // fill row (the fill row's own trade, or the next print when the fill lands on a
    // tick). Resolved back to a `tx_signature` by the drill-in handler so the chart
    // marks the entry candle *at or after* the entry signal, never a stale earlier one.
    let entry_slot = fill_trade_slot(series, fill_row);
    let n = series.n_rows();
    let has_exit_metrics = c.has_exit_metrics();
    let exit_cols = resolve_cols(series, &c.exit_reqs);
    for j in (fill_row + 1)..n {
        if series.dead[j] {
            return closed(ExitCode::Dead, entry_price, entry_at, entry_slot, series.price[j], series.at[j], fill_trade_slot(series, j), buy_amount_sol, cost);
        }
        let p = series.price[j];
        if p.is_finite() {
            if let Some(sl) = c.stop_loss {
                if p <= entry_price * (1.0 - sl / 100.0) {
                    return closed(ExitCode::StopLoss, entry_price, entry_at, entry_slot, p, series.at[j], fill_trade_slot(series, j), buy_amount_sol, cost);
                }
            }
            if let Some(tp) = c.take_profit {
                if p >= entry_price * (1.0 + tp / 100.0) {
                    return closed(ExitCode::TakeProfit, entry_price, entry_at, entry_slot, p, series.at[j], fill_trade_slot(series, j), buy_amount_sol, cost);
                }
            }
        }
        if has_exit_metrics && reqs_any_satisfied(series, &c.exit_reqs, &exit_cols, j) {
            return closed(ExitCode::Metrics, entry_price, entry_at, entry_slot, p, series.at[j], fill_trade_slot(series, j), buy_amount_sol, cost);
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
        entry_slot,
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

/// The real trade a fill resolved on series `row` executes against: the first trade
/// row at or after `row`. A fill that lands on a trade row returns that trade; a fill
/// that lands on a tick (a time/stall/metrics decision between prints) returns the
/// next print — the trade the chart marker snaps to, at or after the decision.
fn fill_trade_slot(series: &MetricSeries, row: usize) -> Option<u64> {
    (row..series.n_rows()).find_map(|j| series.slot[j])
}

/// The scan as one call (entry then exit) — the guard test's per-token driver and
/// the single-combo drill-in's per-token driver (`simulate_generic_one_combo`).
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
    entry_slot: Option<u64>,
    exit_price: f64,
    exit_at: DateTime<Utc>,
    exit_slot: Option<u64>,
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
        entry_slot,
        exit_time: Some(exit_at),
        exit_price: Some(exit_price),
        exit_slot,
    }
}
