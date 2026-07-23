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

use hunter_engine::arm::{CompiledRule, MetricReq, ReqOrigin};
use hunter_engine::deadness::DEAD_QUIET_SECS;
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::{eval, Operator};
use hunter_engine::metrics::position::{position_value, PositionCtx};
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{MetricId, TradeLite, Ts};
use hunter_engine::TICK_MS;

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{round_trip_with_costs, CostModel, ExitCode};
use trading_core::strategies::paper_fill::{
    find_paper_entry_at, find_paper_exit_at, FillModel, PaperFill,
};

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

/// How a scan prices a round-trip: the notional, which trade in the fill window
/// prices each leg, and the execution-cost model charged on top.
///
/// **This is part of a run's identity, not a tuning knob.** Two runs under
/// different pricing are not comparable, and the pair must be chosen coherently:
/// a [`FillModel`] already prices execution slippage, so pairing one with
/// [`CostModel::pumpfun_default`] (which charges `slippage_bps` again) double-counts
/// it — [`CostModel::pumpfun_fee_only`] is the honest partner. Carried as one struct
/// so a scan fn can never be handed the fill model without the cost model.
#[derive(Clone, Copy, Debug)]
pub struct Pricing {
    /// Notional (SOL) every round-trip is sized at.
    pub buy_amount_sol: f64,
    /// Which trade in the fill window prices the entry / exit leg.
    pub fill_model: FillModel,
    /// Execution frictions charged on the round-trip.
    pub cost: CostModel,
}

// Deliberately NO `Default` impl: the only sensible default would be the legacy
// worst-case + `pumpfun_default` pair, and that is exactly the silent, unlabelled
// pricing this type exists to make impossible. Callers name both halves.

/// The generic engine as a sweep [`Strategy`]. Holds the resolved axes model, the
/// run's [`Pricing`], and the deadness "now".
pub struct GenericSweepStrategy {
    model: AxesModel,
    as_of: Ts,
    pricing: Pricing,
    /// The union of every axis's precompute columns (built once).
    columns: Vec<SeriesColumn>,
    /// The sparse-grid horizons derived from the swept axes — sizes the precompute
    /// so a long-lived token records rows only where a decision could change
    /// (plan §P2). Built once; the same grid serves every token.
    grid: SparseGrid,
    /// Corpus-wide volume-ix patterns (compiled). `None` ⇒ no flow state / NaN.
    flow_patterns: Option<hunter_engine::metrics::flow_split::FlowPatterns>,
}

impl GenericSweepStrategy {
    /// Build the strategy from a resolved axes model. `as_of` is the run-time now
    /// the deadness clock advances toward (matches `run_replay`'s `ReplayConfig`).
    /// `flow_patterns` is the run's optional `volume_ix_patterns` (corpus-wide).
    /// `pricing` carries the run's notional + fill model + cost model — see
    /// [`Pricing`] for why the last two travel together.
    pub fn new(
        model: AxesModel,
        pricing: Pricing,
        as_of: Ts,
        flow_patterns: Option<hunter_engine::metrics::flow_split::FlowPatterns>,
    ) -> Self {
        let columns = model.columns();
        let grid = SparseGrid {
            max_window_secs: model.max_window_secs(),
            time_horizon_secs: model.metric_value_ceiling(MetricId::Time),
            stall_horizon_secs: model.metric_value_ceiling(MetricId::Stall),
        };
        Self { model, as_of, pricing, columns, grid, flow_patterns }
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
            buy_amount_lamports: sol_to_lamports(self.pricing.buy_amount_sol).max(0) as u64,
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
        //
        // `sort_by_cached_key`, not `sort_by_key`: the key fn is called once per
        // *comparison* by the latter (~n log n times), and `entry_key` allocates a
        // `vec![0; n_axes]` on every call — ~5.4M allocations at 300k combos, twice on
        // a refine run. Caching makes it n calls. Both sorts are stable, which the
        // contiguity above depends on.
        params.sort_by_cached_key(|c| self.model.entry_key(c.idx));
    }
}

impl Strategy for GenericSweepStrategy {
    type Entry = EntryResolution;
    type EntryKey = u64;
    type TokenState = MetricSeries;
    type BoundParams = BoundCombo;
    type ExitCtx = super::exit_index::ExitIndex;

    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey {
        self.model.entry_key(params.idx)
    }

    fn bind_param(&self, params: &Self::Params) -> Self::BoundParams {
        // Column indices resolve here, once per combo, against the run's fixed column
        // set — not per (token, combo) inside the scan. See `BoundCombo`.
        BoundCombo::new(&self.columns, self.compile_combo(params.idx))
    }

    fn prepare_token(&self, token: &CorpusToken) -> Self::TokenState {
        build_series_with_flow(
            token,
            self.columns.clone(),
            &self.grid,
            self.as_of,
            self.flow_patterns.as_ref(),
        )
    }

    fn build_exit_ctx(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        _params: &Self::Params,
        ctx: &mut Self::ExitCtx,
    ) {
        // Built for every Entered path whose exit reqs ALL classified (plan item B):
        // pure TP/SL, `m_position.pnl`/`held` bounds, and trailing stops. A rule with
        // any `General` req walks scalar, so the hulls would be pure waste there.
        //
        // Before bind-time classification this read `!has_exit_metrics()`, which
        // desugaring made false for **every** TP/SL rule — silently disabling the
        // index (and the SIMD scan) for the exact shape they were built for.
        match entry {
            EntryResolution::Entered { fill_row, .. } if wants_exit_index(bound, entry) => {
                ctx.rebuild(series, *fill_row);
            }
            _ => ctx.clear(),
        }
    }

    fn resolve_entry(
        &self,
        trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
    ) -> Self::Entry {
        resolve_entry(trades, series, bound, &self.pricing)
    }

    fn resolve_exit(
        &self,
        trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        _params: &Self::Params,
        ctx: &Self::ExitCtx,
    ) -> TokenOutcome {
        // AVX-512 toggle still selectable (exit-index plan Phase 5). Default path
        // is the O(log n) index; SIMD remains for A/B. Both must match scalar.
        if crate::sweep::registry::use_simd() {
            resolve_exit_simd(trades, series, bound, entry, &self.pricing, ctx)
        } else {
            resolve_exit_indexed(trades, series, bound, entry, &self.pricing, ctx)
        }
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
    /// Largest registered trailing window (secs) across BOTH window families —
    /// `m_flow_window`/`m_flow_split_window` and `m_price_window`; `0` if the rule
    /// reads neither. A trade keeps changing windowed metrics until `trade + this`
    /// (flows decay to 0; a rolling price high/low drops as the print ages out),
    /// after which every windowed read is settled for that gap.
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
///
/// When `flow_patterns` is set, seeds fingerprint-scoped flow state for every
/// [`SeriesColumn::Flow`] fingerprint in `columns` (V2.2).
pub(crate) fn build_series_with_flow(
    token: &CorpusToken,
    columns: Vec<SeriesColumn>,
    grid: &SparseGrid,
    as_of: Ts,
    flow_patterns: Option<&hunter_engine::metrics::flow_split::FlowPatterns>,
) -> MetricSeries {
    let created = token.created_at;
    let mut series = MetricSeries::new(created, columns);
    if let Some(patterns) = flow_patterns {
        let windows: Vec<f64> = series
            .columns()
            .iter()
            .filter_map(|c| match c {
                SeriesColumn::Flow(_, Some(w), _) => Some(*w),
                SeriesColumn::Window(_, w) => Some(*w),
                _ => None,
            })
            .collect();
        // Seed every fingerprint id that Flow columns read — sweep uses
        // `SWEEP_FLOW_FP` (nil); the scan≡replay guard uses the rule's real id.
        let mut fps: Vec<FingerprintId> = series
            .columns()
            .iter()
            .filter_map(|c| match c {
                SeriesColumn::Flow(_, _, fp) => Some(*fp),
                _ => None,
            })
            .collect();
        fps.sort_by_key(|fp| fp.0);
        fps.dedup();
        for fp in fps {
            series.ensure_flow(fp, patterns, &windows);
        }
    }
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
    crate::sweep::projection::to_trade_lite(ct)
}

/// The union of precompute columns a compiled rule reads (both sides). Used by the
/// guard test to build a series for one rule without the axes model.
pub(crate) fn columns_for(compiled: &CompiledRule) -> Vec<SeriesColumn> {
    let mut cols = Vec::new();
    for req in compiled.entry_reqs.iter().chain(compiled.exit_reqs.iter()) {
        // Position-scoped reqs (`m_position`) read the per-entry `PositionCtx` in the
        // exit scan, not a token series column — see `exit_position_metrics_fired`.
        if req.position_scoped {
            continue;
        }
        let c = col_of(req);
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
    // BOTH window families: a `m_price_window` rolling high decays as old prints age
    // out of the window, so `trail`/`rise` keep changing between trades exactly like a
    // flow window does. Counting only `flow_windows` here would drop the decay-region
    // ticks and let the scan miss a dip trigger a full replay sees.
    let max_window_secs = compiled
        .flow_windows
        .iter()
        .chain(compiled.price_windows.iter())
        .cloned()
        .fold(0.0_f64, f64::max);
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

fn col_of(req: &hunter_engine::arm::MetricReq) -> SeriesColumn {
    match (req.fingerprint, req.window) {
        (Some(fp), ws) => SeriesColumn::Flow(req.metric, ws, fp),
        (None, Some(w)) => SeriesColumn::Window(req.metric, w),
        (None, None) => SeriesColumn::Static(req.metric),
    }
}

// ─────────────────────────────── scan ──────────────────────────────────────

/// The resolved entry for one combo on one token.
#[derive(Clone, Copy, Debug)]
pub enum EntryResolution {
    /// Never armed→entered: dead/unsatisfiable before entry, or no worst-case fill.
    NoEntry,
    /// Entered — the worst-case fill landed at series row `fill_row`.
    Entered { fill_row: usize, price: f64, at: Ts },
}

/// Column index (into the flat series) resolved once per requirement, hoisted out
/// of the per-row scan loop (plan §P1). `usize::MAX` = the column wasn't recorded,
/// which reads as `NaN` and satisfies no condition.
const MISSING_COL: usize = usize::MAX;

fn col_idx_of(series: &MetricSeries, req: &hunter_engine::arm::MetricReq) -> usize {
    series.col_index(col_of(req)).unwrap_or(MISSING_COL)
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

/// One side's requirements paired with their resolved column indices, indexed in
/// lockstep with the `CompiledRule`'s req list. Resolved against the run's **fixed**
/// column set — see [`BoundCombo`].
fn resolve_cols_in(columns: &[SeriesColumn], reqs: &[hunter_engine::arm::MetricReq]) -> Vec<usize> {
    reqs.iter().map(|r| col_idx_in(columns, r)).collect()
}

/// Column index within a fixed column set (`MISSING_COL` when not recorded).
fn col_idx_in(columns: &[SeriesColumn], req: &hunter_engine::arm::MetricReq) -> usize {
    let want = col_of(req);
    columns.iter().position(|c| *c == want).unwrap_or(MISSING_COL)
}

fn col_idx_mono(columns: &[SeriesColumn], k: &hunter_engine::arm::MonoMetricKill) -> usize {
    // Synthetic req so Flow/Static/Window routing matches `col_of`. Mono-kills are
    // always token-scoped entry metrics, so the scope/origin fields are inert here.
    let req = hunter_engine::arm::MetricReq {
        metric: k.metric,
        window: k.window,
        fingerprint: k.fingerprint,
        tolerance: 0.0,
        conds: vec![],
        position_scoped: false,
        origin: hunter_engine::arm::ReqOrigin::Authored,
    };
    col_idx_in(columns, &req)
}

/// A compiled combo **plus its resolved series-column indices**.
///
/// Every token's series is built with `self.columns.clone()` (see `prepare_token`),
/// so the column layout is fixed for the whole run and a combo's column indices are
/// the same on every token. They used to be resolved inside `resolve_entry` /
/// `resolve_exit`, i.e. **once per (token, combo)** — `resolve_exit` is uncached and
/// runs for every combo on every token, which made that the single most-executed
/// heap allocation in a sweep. Resolving at bind time makes it once per combo.
///
/// The precomputed indices are only valid while that fixed-columns invariant holds;
/// a `debug_assert` in each scan re-derives them from the series and would fail loudly
/// in tests if a future change made the column set vary per token.
pub struct BoundCombo {
    pub(crate) rule: CompiledRule,
    entry_cols: Vec<usize>,
    mono_cols: Vec<usize>,
    exit_cols: Vec<usize>,
    /// One [`ExitClass`] per `rule.exit_reqs`, in lockstep — how the fast paths may
    /// resolve each req's first firing row.
    exit_classes: Vec<ExitClass>,
    /// `true` ⇔ **every** exit req classified (no [`ExitClass::General`]), so the
    /// index / SIMD paths can resolve the whole exit. One `General` req forces the
    /// scalar walk for the entire rule: that walk is O(n) and must evaluate every
    /// req anyway, so resolving the others by index would only add work.
    pub(crate) fast_exit: bool,
}

impl BoundCombo {
    /// Bind `rule` against the run's fixed `columns`.
    pub(crate) fn new(columns: &[SeriesColumn], rule: CompiledRule) -> Self {
        let entry_cols = resolve_cols_in(columns, &rule.entry_reqs);
        let exit_cols = resolve_cols_in(columns, &rule.exit_reqs);
        let mono_cols = rule.mono_kills.iter().map(|k| col_idx_mono(columns, k)).collect();
        let exit_classes: Vec<ExitClass> = rule.exit_reqs.iter().map(classify_exit_req).collect();
        let fast_exit = !exit_classes.iter().any(|c| matches!(c, ExitClass::General));
        Self { rule, entry_cols, mono_cols, exit_cols, exit_classes, fast_exit }
    }
}

// ─────────────────────── exit-req classification (bind time) ───────────────────
//
// Phase 2 collapsed TP/SL into position `pnl` reqs, but the fast paths still keyed
// off `has_exit_metrics()` (`!exit_reqs.is_empty()`) — true for every TP/SL rule
// after desugaring — so both the exit index and the AVX-512 scan quietly stopped
// being *taken*. Correctness never moved (scalar is the reference they fall back
// to), which is exactly why nothing caught it.
//
// The fix is to ask a sharper question once per combo, here: not "are there exit
// metrics?" but "can each exit req's FIRST firing row be found without walking
// every row?". Recognition is deliberately conservative — anything not provably
// resolvable lands in `General` and keeps the scalar walk, because a correct
// scalar walk always beats a clever wrong index.

/// How one exit req's first firing row may be resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ExitClass {
    /// `m_position.pnl` under a single monotone bound. `pnl` is strictly increasing
    /// in price, so the condition is upward-closed in price for `>`/`>=` (`up`) and
    /// downward-closed for `<`/`<=` — either way the prefix-extrema hull answers it
    /// in **O(log n)**. This is where the desugared TP/SL land.
    PnlBound { up: bool },
    /// `m_position.held` under a `>`/`>=` bound: `held` is monotone in row time, so
    /// a binary search on `series.at` finds the crossing in **O(log n)**.
    HeldBound,
    /// `m_position.retrace` — needs the running since-entry peak, which no static
    /// prefix query supplies. **O(n)**, vectorized (see `first_trailing_row`). Any
    /// condition shape is fine: the resolver is a linear scan through `eval`.
    Trailing,
    /// Token-scoped columns, multi-arm DNF, tolerance-sensitive `=`/`!=`, anything
    /// unrecognised — the scalar walk stays the SSOT reference.
    General,
}

/// The single condition of a single-arm req, if that's what this is.
fn lone_cond(req: &MetricReq) -> Option<hunter_engine::metrics::evaluator::Condition> {
    match req.conds.as_slice() {
        [arm] => match arm.as_slice() {
            [c] => Some(*c),
            _ => None,
        },
        _ => None,
    }
}

/// Whether this (combo, entry) warrants building the exit index — **the one
/// predicate** `Strategy::build_exit_ctx` branches on, named so the reachability
/// guard can assert the real condition instead of a copy of it.
///
/// Item B exists because a fast path can silently stop being *taken* while staying
/// correct: desugaring flipped the old `!has_exit_metrics()` gate to false for every
/// TP/SL rule and nothing failed, because the scalar fallback is right. Equality
/// tests can't catch that class of rot — only an assertion on reachability can.
pub(crate) fn wants_exit_index(bound: &BoundCombo, entry: &EntryResolution) -> bool {
    bound.fast_exit && matches!(entry, EntryResolution::Entered { .. })
}

/// Classify one exit req (bind time — never per row, never per token).
fn classify_exit_req(req: &MetricReq) -> ExitClass {
    if !req.position_scoped {
        // Token-scoped metrics are arbitrary functions of the series columns; only
        // the scalar walk knows when they flip.
        return ExitClass::General;
    }
    match req.metric {
        // A linear scan evaluates any condition shape correctly, so `retrace`
        // classifies unconditionally.
        MetricId::Retrace => ExitClass::Trailing,
        MetricId::Pnl => match lone_cond(req).map(|c| c.operator) {
            Some(Operator::Gt | Operator::Gte) => ExitClass::PnlBound { up: true },
            Some(Operator::Lt | Operator::Lte) => ExitClass::PnlBound { up: false },
            // `=`/`!=` are tolerance bands (not monotone) and multi-arm DNF can be
            // an interval — neither is a prefix query.
            _ => ExitClass::General,
        },
        MetricId::Held => match lone_cond(req).map(|c| c.operator) {
            // `held` only ever rises, so a lower bound is a single crossing. An
            // upper bound (`held <= X`) is satisfied from the fill onward and would
            // need the opposite search — not worth a second code path.
            Some(Operator::Gt | Operator::Gte) => ExitClass::HeldBound,
            _ => ExitClass::General,
        },
        _ => ExitClass::General,
    }
}

/// Re-derive this combo's column indices from `series` and confirm they match the
/// ones cached at bind time — the fixed-columns invariant [`BoundCombo`] rests on.
/// Debug-only: in release the scan trusts the cache, which is the whole point.
fn debug_assert_cols_match(series: &MetricSeries, c: &BoundCombo) {
    debug_assert!(
        c.rule
            .entry_reqs
            .iter()
            .zip(&c.entry_cols)
            .chain(c.rule.exit_reqs.iter().zip(&c.exit_cols))
            .all(|(r, &col)| col == col_idx_of(series, r))
            && c.rule.mono_kills.iter().zip(&c.mono_cols).all(|(k, &col)| {
                let req = hunter_engine::arm::MetricReq {
                    metric: k.metric,
                    window: k.window,
                    fingerprint: k.fingerprint,
                    tolerance: 0.0,
                    conds: vec![],
                    position_scoped: false,
                    origin: hunter_engine::arm::ReqOrigin::Authored,
                };
                col == col_idx_of(series, &req)
            }),
        "BoundCombo column indices disagree with this token's series — the run's \
         column set is no longer fixed across tokens, so bind-time resolution is unsound"
    );
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

/// The exit label a fired req carries — mirror of `CompiledRule::exit_fired`'s
/// `ReqOrigin` → `ExitReason` mapping, so a desugared TP/SL still reads
/// `TakeProfit`/`StopLoss` and an authored metric reads `Metrics`.
#[inline]
fn exit_code_of(origin: ReqOrigin) -> ExitCode {
    match origin {
        ReqOrigin::TakeProfit => ExitCode::TakeProfit,
        ReqOrigin::StopLoss => ExitCode::StopLoss,
        ReqOrigin::Authored => ExitCode::Metrics,
    }
}

/// Whether one exit req holds at `row`. Token-scoped reqs read the precomputed
/// series column; position-scoped reqs (`m_position`) read the running
/// [`PositionCtx`] at this row's price/time. **The one place a req is judged** —
/// every fast path below must agree with it.
#[inline]
fn exit_req_fires(
    series: &MetricSeries,
    req: &MetricReq,
    col: usize,
    row: usize,
    ctx: &PositionCtx,
) -> bool {
    let reading = if req.position_scoped {
        position_value(req.metric, ctx, series.price[row], series.at[row])
    } else {
        value_at_col(series, col, row)
    };
    eval(&req.conds, reading, req.tolerance)
}

/// Held-side exit combinator — the scan's mirror of `CompiledRule::exit_fired`.
/// Walks `exit_reqs` **in order** (compile prepends the desugared SL then TP ahead
/// of the authored metrics) and returns the first that holds, labelled by its
/// [`ReqOrigin`]. That order is what preserves the ladder's
/// `StopLoss > TakeProfit > Metrics` priority now that TP/SL are ordinary `pnl`
/// reqs rather than a separate price-threshold branch.
fn first_exit_req_fired(
    series: &MetricSeries,
    reqs: &[MetricReq],
    cols: &[usize],
    row: usize,
    ctx: &PositionCtx,
) -> Option<ExitCode> {
    reqs.iter()
        .zip(cols)
        .find(|(r, &col)| exit_req_fires(series, r, col, row, ctx))
        .map(|(r, _)| exit_code_of(r.origin))
}

fn entry_unsatisfiable(series: &MetricSeries, c: &CompiledRule, mono_cols: &[usize], row: usize) -> bool {
    c.mono_kills
        .iter()
        .zip(mono_cols)
        .any(|(k, &col)| k.permanently_false(value_at_col(series, col, row)))
}

/// Walk the series to the entry decision, mirroring the armed-side `decide_arm`:
/// `Dead > Unsatisfiable > can_enter` (entry conditions hold and exit metrics, if
/// any, do not). The fill is the buy the run's [`FillModel`] picks out of the window
/// after the trigger trade (trigger slot + next slot within
/// [`MAX_FILL_WAIT_SLOTS`](trading_core::config::constants::MAX_FILL_WAIT_SLOTS)) —
/// the same model `run_replay` threads through `ReplayConfig.fill_model`. Fill
/// *eligibility* is identical across models, so the taken set never moves; only the
/// price does. A tick-timed decision before any print uses the first later trade as
/// the trigger; an empty fill window ⇒ `NoEntry`.
pub(crate) fn resolve_entry(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    pricing: &Pricing,
) -> EntryResolution {
    debug_assert_cols_match(series, b);
    let c = &b.rule;
    let n = series.n_rows();
    // Column indices come pre-resolved from `bind_param` — see `BoundCombo`.
    let (entry_cols, mono_cols, exit_cols) = (&b.entry_cols, &b.mono_cols, &b.exit_cols);
    let enter_on_arm = c.enter_on_arm();
    let has_exit_metrics = c.has_exit_metrics();
    for i in 0..n {
        if series.dead[i] {
            return EntryResolution::NoEntry;
        }
        if entry_unsatisfiable(series, c, mono_cols, i) {
            return EntryResolution::NoEntry;
        }
        if enter_on_arm || reqs_satisfied(series, &c.entry_reqs, entry_cols, i) {
            // Mirror `CompiledRule::can_enter`: never buy while exit metrics already hold.
            if has_exit_metrics && reqs_any_satisfied(series, &c.exit_reqs, exit_cols, i) {
                continue;
            }
            let Some(trigger_idx) = entry_trigger_trade_idx(series, i) else {
                return EntryResolution::NoEntry;
            };
            let Some(fill) = find_paper_entry_at(trades, trigger_idx, pricing.fill_model) else {
                return EntryResolution::NoEntry;
            };
            let Some(fill_row) = series_row_for_trade_idx(series, fill.trade_idx) else {
                return EntryResolution::NoEntry;
            };
            return EntryResolution::Entered {
                fill_row,
                price: fill.price,
                at: fill.block_time,
            };
        }
    }
    EntryResolution::NoEntry
}

/// Class-directed exit resolution over a prebuilt [`super::exit_index::ExitIndex`]:
/// each exit req's **first firing row** is found by whichever query its bind-time
/// [`ExitClass`] allows (O(log n) hull / binary search; O(n) vectorized for a
/// trailing stop), then the earliest of those rows — and `Dead` — decides.
///
/// Falls back to the scalar [`resolve_exit`] for `NoEntry`, an unready index, any
/// `General` req, or a monotonicity guard trip. The scalar walk is never deleted:
/// it is the reference every one of these queries is proven equal to.
pub(crate) fn resolve_exit_indexed(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    index: &super::exit_index::ExitIndex,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    if !b.fast_exit || !index.is_ready() {
        return resolve_exit(trades, series, b, entry, pricing);
    }

    // Earliest firing row across the classified reqs. On a tie the req EARLIER in
    // `exit_reqs` wins — the same order the scalar walk breaks ties by, which is what
    // keeps the desugared SL ahead of TP ahead of the authored metrics. (A req whose
    // first firing row is `min` is exactly a req that fires at `min`: an earlier first
    // row would contradict minimality.)
    let mut best: Option<(usize, ExitCode)> = None;
    for (i, req) in b.rule.exit_reqs.iter().enumerate() {
        let row = match b.exit_classes[i] {
            ExitClass::PnlBound { up } => match first_pnl_row(index, req, entry_price, up) {
                Ok(row) => row,
                // Non-monotone `pnl` over this token's price range (overflow) — the
                // hull query would be unsound, so hand the whole rule to scalar.
                Err(()) => return resolve_exit(trades, series, b, entry, pricing),
            },
            ExitClass::HeldBound => match first_held_row(series, index, fill_row, entry_at, req) {
                Ok(row) => row,
                // Block time regressed inside the scan range, so `held` is not
                // monotone and a binary search could land anywhere.
                Err(()) => return resolve_exit(trades, series, b, entry, pricing),
            },
            ExitClass::Trailing => first_trailing_row(series, fill_row, entry_price, req),
            ExitClass::General => unreachable!("fast_exit implies no General req"),
        };
        if let Some(row) = row {
            if best.is_none_or(|(br, _)| row < br) {
                best = Some((row, exit_code_of(req.origin)));
            }
        }
    }
    if let Some(dead) = index.dead_row() {
        // Dead outranks every strategy exit, at any row (scalar checks it first).
        if best.is_none_or(|(br, _)| dead <= br) {
            best = Some((dead, ExitCode::Dead));
        }
    }

    match best {
        Some((row, exit)) => close_at_fire(
            trades, series, exit, entry_price, entry_at, fill_trade_slot(series, fill_row), row,
            pricing,
        ),
        // Open: mark to last finite (precomputed on the index for the whole series).
        None => {
            let last_price = index
                .last_finite_row()
                .map(|k| series.price[k])
                .filter(|p| p.is_finite())
                .unwrap_or(entry_price);
            open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
        }
    }
}

/// Walk from the entry fill to the exit decision, mirroring the open-side
/// `decide_arm`: `Dead` first, then the first `exit_reqs` entry that holds (compile
/// prepends the desugared SL/TP, so the ladder's `StopLoss > TakeProfit > Metrics`
/// order survives). No exit by the tail ⇒ `Open`, marked to the last known price.
/// Exit *price* is the run's [`FillModel`] fill after the firing row (analysis:
/// market-fill fallback on an empty window).
///
/// **This walk is the SSOT.** TP/SL used to be re-derived here as an
/// `entry_price · (1 ∓ pct/100)` price branch — a second representation of the same
/// fact the engine already desugars into a `pnl` req, and one that compared in price
/// space where the fold compares in pnl space. That branch is gone: the sweep now
/// evaluates the very reqs `CompiledRule::exit_fired` does.
pub(crate) fn resolve_exit(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    let c = &b.rule;
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
    let has_exit_reqs = c.has_exit_metrics();
    // Pre-resolved at bind time — this was the run's most-executed heap allocation.
    let exit_cols = &b.exit_cols;
    // Running since-entry peak for the position-scoped exit metrics (`retrace`/`pnl`).
    // Seeded to the fill price — exactly as `reduce.rs` seeds `ArmState::Entered`'s
    // `peak_price` — and folded forward each event BEFORE that event's exit decision,
    // mirroring `evaluate_token`'s per-event peak fold.
    let mut peak_price = entry_price;
    for j in (fill_row + 1)..n {
        if series.dead[j] {
            return close_at_fire(
                trades, series, ExitCode::Dead, entry_price, entry_at, entry_slot, j, pricing,
            );
        }
        let p = series.price[j];
        if p.is_finite() && p > peak_price {
            peak_price = p;
        }
        if has_exit_reqs {
            let ctx = PositionCtx { entry_price, peak_price, entered_at: entry_at };
            if let Some(exit) = first_exit_req_fired(series, &c.exit_reqs, exit_cols, j, &ctx) {
                return close_at_fire(
                    trades, series, exit, entry_price, entry_at, entry_slot, j, pricing,
                );
            }
        }
    }
    // Open: mark to the last finite price (unrealized — excluded from the realized
    // stats by `RunAgg`, but priced for the drill-in / row view).
    let last_price = (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
    open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
}

/// The still-`Open` outcome, marked to `last_price`. One copy shared by every exit
/// path so the scalar / index / SIMD tails can't drift.
fn open_outcome(
    series: &MetricSeries,
    fill_row: usize,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    last_price: f64,
    pricing: &Pricing,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) =
        round_trip_with_costs(entry_price, last_price, pricing.buy_amount_sol, &pricing.cost);
    TokenOutcome {
        fired: true,
        holding_secs: 0,
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::Open,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: fill_trade_slot(series, fill_row),
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

// ───────────────────── per-class first-firing-row resolvers ────────────────────
//
// Each of these must return **exactly** the row the scalar walk would stop at for
// that one req — same `eval`, same value, same NaN handling. They differ only in
// how they get there.

/// First row satisfying a monotone `m_position.pnl` bound, via the prefix-extrema
/// hull — O(log n).
///
/// `pnl` is strictly increasing in price (for `entry_price > 0`), so an upward-closed
/// condition holds at some row `≤ i` iff it holds at the running **max** price up to
/// `i`; symmetrically for the running min. The predicate is the same `eval` the
/// scalar walk applies, so the two cannot disagree about inclusivity or NaN.
///
/// `Err(())` ⇒ the monotonicity premise doesn't hold on this token (a price extreme
/// whose `pnl` overflows to ±inf, which `eval` then rejects out of order); the caller
/// must fall back to scalar.
fn first_pnl_row(
    index: &super::exit_index::ExitIndex,
    req: &MetricReq,
    entry_price: f64,
    up: bool,
) -> Result<Option<usize>, ()> {
    if !usable_entry_price(entry_price) {
        // `pnl` is NaN throughout — no row can fire, exactly as scalar finds.
        return Ok(None);
    }
    let ctx = PositionCtx { entry_price, peak_price: entry_price, entered_at: DateTime::UNIX_EPOCH };
    let extreme = if up { index.hull_max_last() } else { index.hull_min_last() };
    if let Some(x) = extreme {
        // The extreme bounds every value the hull can carry; if `pnl` stays finite
        // there it is finite (and monotone) everywhere in range.
        if x.is_finite() && !ctx.pnl(x).is_finite() {
            return Err(());
        }
    }
    let pred = |price: f64| eval(&req.conds, ctx.pnl(price), req.tolerance);
    Ok(if up { index.first_max_row(pred) } else { index.first_min_row(pred) })
}

/// First row satisfying a `>=`/`>` bound on `m_position.held`, by binary search on
/// the series' `at` column — O(log n).
///
/// `held` is `now − entered_at` floored at zero, so it rises with `at`; the search is
/// only sound while `at` is non-decreasing over the scan range, which
/// [`ExitIndex`](super::exit_index::ExitIndex) checks during its rebuild. `Err(())`
/// ⇒ it isn't; fall back to scalar.
fn first_held_row(
    series: &MetricSeries,
    index: &super::exit_index::ExitIndex,
    fill_row: usize,
    entry_at: Ts,
    req: &MetricReq,
) -> Result<Option<usize>, ()> {
    if !index.at_nondecreasing() {
        return Err(());
    }
    let n = series.n_rows();
    let start = fill_row.saturating_add(1);
    if start >= n {
        return Ok(None);
    }
    let ctx = PositionCtx { entry_price: 1.0, peak_price: 1.0, entered_at: entry_at };
    let at = &series.at[start..n];
    // `partition_point` wants the "not yet fired" prefix — true then false.
    let i = at.partition_point(|&now| !eval(&req.conds, ctx.held(now), req.tolerance));
    Ok((i < at.len()).then_some(start + i))
}

/// First row satisfying a `m_position.retrace` condition — O(n) with the running
/// since-entry peak, vectorized on AVX-512 hosts for a single ordering condition.
///
/// **Not** O(log n), and there is no cheap index for it: the peak is a running
/// quantity, so this is a genuine prefix-dependent scan, not a static prefix query.
fn first_trailing_row(
    series: &MetricSeries,
    fill_row: usize,
    entry_price: f64,
    req: &MetricReq,
) -> Option<usize> {
    let n = series.n_rows();
    let start = fill_row.saturating_add(1);
    if start >= n {
        return None;
    }
    if let Some(c) = lone_cond(req).filter(|c| is_ordering_op(c.operator)) {
        return first_trailing_row_cmp(&series.price, start, n, entry_price, c.operator, c.value);
    }
    first_trailing_row_scalar(series, start, n, entry_price, req)
}

/// Scalar reference for [`first_trailing_row`] — the exact per-row work the scalar
/// walk does for a `retrace` req, extracted so the vector kernel has one definition
/// to be proven against.
fn first_trailing_row_scalar(
    series: &MetricSeries,
    start: usize,
    n: usize,
    entry_price: f64,
    req: &MetricReq,
) -> Option<usize> {
    let mut ctx = PositionCtx {
        entry_price,
        peak_price: entry_price,
        entered_at: DateTime::UNIX_EPOCH,
    };
    for j in start..n {
        let p = series.price[j];
        if p.is_finite() && p > ctx.peak_price {
            ctx.peak_price = p;
        }
        if eval(&req.conds, ctx.retrace(p), req.tolerance) {
            return Some(j);
        }
    }
    None
}

/// Whether `entry_price` is a usable reference for the position metrics.
/// [`PositionCtx::pnl`] / [`PositionCtx::retrace`] yield `NaN` for anything else, so
/// no row can fire and the fast paths have nothing to search for.
#[inline]
fn usable_entry_price(p: f64) -> bool {
    p.is_finite() && p > 0.0
}

/// Ordering operators the vector kernels can replicate exactly (`=`/`!=` are
/// tolerance bands and stay on the scalar path).
#[inline]
fn is_ordering_op(op: Operator) -> bool {
    matches!(op, Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte)
}

/// `eval_one` for an ordering operator, spelled out so the vector kernels and the
/// scalar remainder share one definition of the compare.
#[inline]
fn cmp_ordering(op: Operator, value: f64, threshold: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match op {
        Operator::Gt => value > threshold,
        Operator::Gte => value >= threshold,
        Operator::Lt => value < threshold,
        Operator::Lte => value <= threshold,
        // Never reached: `is_ordering_op` gates every caller.
        _ => false,
    }
}

// ───────────────────────────── SIMD exit scan ──────────────────────────────
//
// AVX-512 counterpart of the pure-TP/SL branch of `resolve_exit` (plan §P1). It
// vectorizes ONLY the *search* for the first exit row; classification and the money
// math (`closed` → `round_trip_with_costs`) stay the one shared scalar copy, so the
// SSOT surface reduces to a single question — "did the vector scan find the same
// first-crossing row as scalar?" — which the guard test (`super::guard`) proves.
//
// Note the price column is `f64`, so the vector width is 8 lanes (`__m512d`), not
// the 16×f32 the plan's prose assumed — the search still runs 8-wide per instruction.

/// AVX-512 counterpart of the [`ExitClass::PnlBound`] exit scan (plan §P1, locked
/// design decision 1). Finds the first exit *row* with the vector unit, then
/// classifies + closes through the exact same shared kernel the scalar path uses —
/// the money math is never duplicated.
///
/// Handles the shape it can vectorize end to end: every exit req is a monotone
/// `m_position.pnl` bound with a single ordering condition — which, after the Phase-2
/// desugaring, is precisely a TP/SL rule. **This is the shape the vector scan was
/// built for and had silently stopped being reached on**: the old gate was
/// `has_exit_metrics()`, which desugaring made true for every TP/SL rule. Anything
/// else (a trailing stop, a `held` bound, a `General` req) delegates to
/// [`resolve_exit_indexed`], which resolves those classes properly instead of
/// pretending they aren't there. Also delegates on a host without AVX-512, so a
/// direct caller (the parity guard) is always safe.
pub(crate) fn resolve_exit_simd(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    index: &super::exit_index::ExitIndex,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    let Some(bounds) = pnl_bounds_for_vector_scan(b, entry_price) else {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index);
    };
    if !simd_available() {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index);
    }
    let entry_slot = fill_trade_slot(series, fill_row);
    let n = series.n_rows();

    if let Some(j) =
        first_pnl_exit_row(&series.price, &series.dead, fill_row + 1, n, entry_price, &bounds)
    {
        // Same within-row priority the scalar walk applies: Dead first, else the
        // EARLIEST req in `exit_reqs` order that holds here. A req firing at `j`
        // whose own first firing row were earlier would contradict `j` being first,
        // so re-judging this one row reproduces the walk exactly.
        let exit = if series.dead[j] {
            ExitCode::Dead
        } else {
            let ctx = PositionCtx { entry_price, peak_price: entry_price, entered_at: entry_at };
            first_exit_req_fired(series, &b.rule.exit_reqs, &b.exit_cols, j, &ctx)
                .unwrap_or(ExitCode::Metrics)
        };
        return close_at_fire(
            trades, series, exit, entry_price, entry_at, entry_slot, j, pricing,
        );
    }

    // Open: identical tail to the scalar path — mark to the last finite price.
    let last_price = (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
    open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
}

/// One lane-comparable exit bound: `pnl <op> value`, with `op` an ordering operator.
#[derive(Clone, Copy, Debug)]
struct PnlBound {
    op: Operator,
    value: f64,
}

/// The rule's exit reqs as vector-comparable `pnl` bounds, or `None` when even one
/// req is something the kernel can't replicate exactly (a different class, a
/// multi-arm DNF, a tolerance-banded `=`/`!=`) — the caller then takes the class-
/// directed path. Also `None` for a non-positive entry price, where `pnl` is `NaN`
/// throughout and the vector compares would have nothing to say.
fn pnl_bounds_for_vector_scan(b: &BoundCombo, entry_price: f64) -> Option<Vec<PnlBound>> {
    if !b.fast_exit || !usable_entry_price(entry_price) || b.rule.exit_reqs.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(b.rule.exit_reqs.len());
    for (i, req) in b.rule.exit_reqs.iter().enumerate() {
        if !matches!(b.exit_classes[i], ExitClass::PnlBound { .. }) {
            return None;
        }
        let c = lone_cond(req).filter(|c| is_ordering_op(c.operator))?;
        out.push(PnlBound { op: c.operator, value: c.value });
    }
    Some(out)
}

/// True only when the host has the AVX-512 features the kernels here use. They need
/// just `avx512f` (the dead lanes are built scalar), matching
/// [`crate::sweep::registry::avx512_available`].
#[cfg(target_arch = "x86_64")]
#[inline]
fn simd_available() -> bool {
    std::is_x86_feature_detected!("avx512f")
}
#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn simd_available() -> bool {
    false
}

/// First series row in `start..n` at which the scalar exit predicate holds:
/// `dead[j] || any bound holds on pnl(price[j])`. Returned in the same scan order
/// the scalar loop uses, so the caller classifies exactly one row identically to
/// scalar. Vectorized 8×`f64` on AVX-512; scalar remainder + scalar fallback.
///
/// `pnl` is computed per lane with the same `(p − entry) / entry · 100` op sequence
/// [`PositionCtx::pnl`] uses. IEEE-754 basic ops are exactly rounded, so the vector
/// and scalar values are **bit-identical** — which is why this can compare in pnl
/// space rather than inverting each bound back into a price threshold (an inversion
/// that would only be correct to within a rounding step).
#[cfg(target_arch = "x86_64")]
fn first_pnl_exit_row(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    if start >= n {
        return None;
    }
    if simd_available() {
        // SAFETY: `avx512f` confirmed present just above. The kernel reads `price`
        // and `dead` only within `[start, n)`, and `n == series.n_rows()` bounds both
        // parallel columns (see `MetricSeries`), so every access is in range.
        unsafe { first_pnl_exit_row_avx512(price, dead, start, n, entry_price, bounds) }
    } else {
        first_pnl_exit_row_scalar(price, dead, start, n, entry_price, bounds)
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn first_pnl_exit_row(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    first_pnl_exit_row_scalar(price, dead, start, n, entry_price, bounds)
}

/// Scalar reference for [`first_pnl_exit_row`] — the exact predicate the scalar
/// [`resolve_exit`] loop applies for a set of `pnl` bounds, extracted so the vector
/// path's remainder tail, the non-AVX-512 fallback, and the parity guard all share
/// one definition of it.
#[inline]
fn first_pnl_exit_row_scalar(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    let ctx = PositionCtx {
        entry_price,
        peak_price: entry_price,
        entered_at: DateTime::UNIX_EPOCH,
    };
    for j in start..n {
        if dead[j] {
            return Some(j);
        }
        let v = ctx.pnl(price[j]);
        if bounds.iter().any(|b| cmp_ordering(b.op, v, b.value)) {
            return Some(j);
        }
    }
    None
}

/// AVX-512 kernel for [`first_pnl_exit_row`]: scans 8 `f64` prices per instruction
/// for the first exit row, handling the `< 8` tail via
/// [`first_pnl_exit_row_scalar`].
///
/// # Safety
/// The caller must have verified `avx512f` is available (see [`first_pnl_exit_row`]).
/// `start ≤ n` and `n` must not exceed `price.len()` or `dead.len()`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn first_pnl_exit_row_avx512(
    price: &[f64],
    dead: &[bool],
    start: usize,
    n: usize,
    entry_price: f64,
    bounds: &[PnlBound],
) -> Option<usize> {
    use std::arch::x86_64::*;
    const LANES: usize = 8;

    let max_v = _mm512_set1_pd(f64::MAX);
    let entry_v = _mm512_set1_pd(entry_price);
    let hundred_v = _mm512_set1_pd(100.0);

    let mut j = start;
    while j + LANES <= n {
        // 8 prices (unaligned — the flat series buffer isn't 64-B aligned).
        let pv = _mm512_loadu_pd(price.as_ptr().add(j));
        // finite lanes: |p| ≤ f64::MAX — false for NaN and ±inf, exactly `is_finite`.
        let finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pv), max_v);
        // pnl per lane, same op order as `PositionCtx::pnl` ⇒ bit-identical.
        let pnl = _mm512_mul_pd(
            _mm512_div_pd(_mm512_sub_pd(pv, entry_v), entry_v),
            hundred_v,
        );
        let mut hit: __mmask8 = 0;
        for b in bounds {
            let t = _mm512_set1_pd(b.value);
            // Ordered compares so a NaN lane never matches, matching `eval_one`'s
            // non-finite rejection; the `finite` AND below covers ±inf prices, whose
            // pnl would otherwise compare as a genuine ±inf.
            hit |= match b.op {
                Operator::Gt => _mm512_cmp_pd_mask::<_CMP_GT_OQ>(pnl, t),
                Operator::Gte => _mm512_cmp_pd_mask::<_CMP_GE_OQ>(pnl, t),
                Operator::Lt => _mm512_cmp_pd_mask::<_CMP_LT_OQ>(pnl, t),
                _ => _mm512_cmp_pd_mask::<_CMP_LE_OQ>(pnl, t),
            };
        }
        // A finite price can still overflow `pnl` to ±inf on a tiny entry price;
        // `is_finite` on the pnl VALUE is what `eval_one` checks, so mask on both.
        let pnl_finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pnl), max_v);
        hit &= finite & pnl_finite;
        // dead lanes: built scalar (dead is rarely set → cheap predictable branches),
        // which keeps the kernel to plain AVX-512F (no BW/VL byte-mask intrinsics).
        let mut dead_hit: __mmask8 = 0;
        for lane in 0..LANES {
            if dead[j + lane] {
                dead_hit |= 1u8 << lane;
            }
        }
        let hit = hit | dead_hit;
        if hit != 0 {
            return Some(j + hit.trailing_zeros() as usize);
        }
        j += LANES;
    }
    // Tail (< 8 rows left): the shared scalar predicate.
    first_pnl_exit_row_scalar(price, dead, j, n, entry_price, bounds)
}

// ─────────────────── AVX-512 trailing stop (running-peak scan) ─────────────────
//
// `retrace` compares against the since-entry PEAK, so its first crossing is
// `first j where price[j] <= k · max(price[fill..j])` — a prefix-dependent scan, not
// a static prefix query. There is no cheap index for it and none is claimed: the
// honest target is a **vectorized O(n)**, which is what this is. The prefix max is
// built with a 3-step Hillis-Steele shift-and-max inside each 8-lane block, seeded
// with the max carried out of the previous block.

/// First row in `start..n` where `retrace <op> value` holds against the running
/// since-entry peak (seeded at `entry_price`). Vectorized on AVX-512; the scalar
/// reference below is the SSOT both the tail and non-AVX-512 hosts use.
#[cfg(target_arch = "x86_64")]
fn first_trailing_row_cmp(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    if start >= n {
        return None;
    }
    if simd_available() {
        // SAFETY: `avx512f` confirmed present just above; the kernel reads `price`
        // only within `[start, n)` and `n ≤ price.len()` by construction.
        unsafe { first_trailing_row_avx512(price, start, n, entry_price, op, value) }
    } else {
        first_trailing_row_cmp_scalar(price, start, n, entry_price, op, value)
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn first_trailing_row_cmp(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    first_trailing_row_cmp_scalar(price, start, n, entry_price, op, value)
}

/// Scalar reference for [`first_trailing_row_cmp`] — the same running-peak +
/// [`PositionCtx::retrace`] the scalar walk computes per row.
#[inline]
fn first_trailing_row_cmp_scalar(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    let mut ctx = PositionCtx {
        entry_price,
        peak_price: entry_price,
        entered_at: DateTime::UNIX_EPOCH,
    };
    for (j, &p) in price.iter().enumerate().take(n).skip(start) {
        if p.is_finite() && p > ctx.peak_price {
            ctx.peak_price = p;
        }
        if cmp_ordering(op, ctx.retrace(p), value) {
            return Some(j);
        }
    }
    None
}

/// AVX-512 kernel for [`first_trailing_row_cmp`].
///
/// Per 8-lane block: replace non-finite prices with `−inf` (the scalar peak update
/// only admits finite prices, and `−inf` never wins a max), inclusive-prefix-max the
/// block, fold in the carried peak, compute `retrace = (peak − p) / peak · 100` with
/// the same op sequence [`PositionCtx::retrace`] uses (⇒ bit-identical), then compare
/// with an ordered predicate ANDed with the finite-price mask.
///
/// # Safety
/// The caller must have verified `avx512f` (see [`first_trailing_row_cmp`]).
/// `start ≤ n ≤ price.len()`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn first_trailing_row_avx512(
    price: &[f64],
    start: usize,
    n: usize,
    entry_price: f64,
    op: Operator,
    value: f64,
) -> Option<usize> {
    use std::arch::x86_64::*;
    const LANES: usize = 8;

    let max_v = _mm512_set1_pd(f64::MAX);
    let neg_inf = _mm512_set1_pd(f64::NEG_INFINITY);
    let hundred_v = _mm512_set1_pd(100.0);
    let thresh_v = _mm512_set1_pd(value);
    let zero_v = _mm512_setzero_pd();
    // Lane-shift permutations for the Hillis-Steele prefix max: lane i reads lane
    // i−d (lanes < d are masked to −inf, the max identity).
    let sh1 = _mm512_set_epi64(6, 5, 4, 3, 2, 1, 0, 0);
    let sh2 = _mm512_set_epi64(5, 4, 3, 2, 1, 0, 0, 0);
    let sh4 = _mm512_set_epi64(3, 2, 1, 0, 0, 0, 0, 0);

    let mut carry = entry_price;
    let mut j = start;
    while j + LANES <= n {
        let pv = _mm512_loadu_pd(price.as_ptr().add(j));
        let finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(pv), max_v);
        // Only finite prices may raise the peak — everything else becomes −inf.
        let clean = _mm512_mask_blend_pd(finite, neg_inf, pv);
        // Inclusive prefix max, 3 shift-and-max steps (no NaN survives `clean`, so
        // plain `max` is well-defined here).
        let mut pm = clean;
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xFE, sh1, pm));
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xFC, sh2, pm));
        pm = _mm512_max_pd(pm, _mm512_mask_permutexvar_pd(neg_inf, 0xF0, sh4, pm));
        // Fold in the peak carried out of every earlier row.
        let peak = _mm512_max_pd(pm, _mm512_set1_pd(carry));
        // retrace = (peak − p) / peak · 100, with `peak > 0 && p finite` (else NaN,
        // which satisfies nothing).
        let retrace = _mm512_mul_pd(
            _mm512_div_pd(_mm512_sub_pd(peak, pv), peak),
            hundred_v,
        );
        let peak_pos = _mm512_cmp_pd_mask::<_CMP_GT_OQ>(peak, zero_v);
        let retrace_finite = _mm512_cmp_pd_mask::<_CMP_LE_OQ>(_mm512_abs_pd(retrace), max_v);
        let mut hit = match op {
            Operator::Gt => _mm512_cmp_pd_mask::<_CMP_GT_OQ>(retrace, thresh_v),
            Operator::Gte => _mm512_cmp_pd_mask::<_CMP_GE_OQ>(retrace, thresh_v),
            Operator::Lt => _mm512_cmp_pd_mask::<_CMP_LT_OQ>(retrace, thresh_v),
            _ => _mm512_cmp_pd_mask::<_CMP_LE_OQ>(retrace, thresh_v),
        };
        hit &= finite & peak_pos & retrace_finite;
        if hit != 0 {
            return Some(j + hit.trailing_zeros() as usize);
        }
        // Carry the block's last (== whole-block) prefix max forward.
        let mut lanes = [0f64; LANES];
        _mm512_storeu_pd(lanes.as_mut_ptr(), peak);
        carry = lanes[LANES - 1];
        j += LANES;
    }
    // Tail (< 8 rows left): the shared scalar predicate, resumed at the carried peak.
    let mut ctx =
        PositionCtx { entry_price, peak_price: carry, entered_at: DateTime::UNIX_EPOCH };
    for (k, &p) in price.iter().enumerate().take(n).skip(j) {
        if p.is_finite() && p > ctx.peak_price {
            ctx.peak_price = p;
        }
        if cmp_ordering(op, ctx.retrace(p), value) {
            return Some(k);
        }
    }
    None
}

/// The real trade a fill resolved on series `row` executes against: the first trade
/// row at or after `row`. A fill that lands on a trade row returns that trade; a fill
/// that lands on a tick (a time/stall/metrics decision between prints) returns the
/// next print — the trade the chart marker snaps to, at or after the decision.
fn fill_trade_slot(series: &MetricSeries, row: usize) -> Option<u64> {
    (row..series.n_rows()).find_map(|j| series.slot[j])
}

/// Corpus trade index that *triggers* an entry decision at series `decision_row`:
/// the trade on that row when it is a print, else the first print after it
/// (tick / enter-on-arm before a price — same deferral as replay `pending_buys`).
fn entry_trigger_trade_idx(series: &MetricSeries, decision_row: usize) -> Option<usize> {
    if series.slot.get(decision_row).copied().flatten().is_some() {
        return trade_idx_at_row(series, decision_row);
    }
    first_trade_idx_after_row(series, decision_row)
}

/// Corpus index of the trade on series `row` (`None` when `row` is a tick).
fn trade_idx_at_row(series: &MetricSeries, row: usize) -> Option<usize> {
    if series.slot.get(row).copied().flatten().is_none() {
        return None;
    }
    Some(series.slot[..=row].iter().filter(|s| s.is_some()).count() - 1)
}

/// Corpus index of the first trade row strictly after `row`.
fn first_trade_idx_after_row(series: &MetricSeries, row: usize) -> Option<usize> {
    let n = series.n_rows();
    if n == 0 {
        return None;
    }
    let before = if row >= n {
        series.slot.iter().filter(|s| s.is_some()).count()
    } else {
        series.slot[..=row].iter().filter(|s| s.is_some()).count()
    };
    let from = row.saturating_add(1).min(n);
    let after = series.slot[from..].iter().filter(|s| s.is_some()).count();
    (after > 0).then_some(before)
}

/// Series row of the `trade_idx`-th trade print (`None` if out of range).
fn series_row_for_trade_idx(series: &MetricSeries, trade_idx: usize) -> Option<usize> {
    let mut seen = 0usize;
    for (row, slot) in series.slot.iter().enumerate() {
        if slot.is_some() {
            if seen == trade_idx {
                return Some(row);
            }
            seen += 1;
        }
    }
    None
}

/// Exit fill after the ladder fires at series row `fire_row`, priced by the run's
/// [`FillModel`]. Analysis path: market-fill at the firing trade when the window is
/// empty (`market_fill_on_empty_window: true`, as every analysis path books).
fn exit_fill(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    fire_row: usize,
    fill_model: FillModel,
) -> Option<PaperFill> {
    let fire_idx = trade_idx_at_row(series, fire_row)
        .or_else(|| {
            // Tick-timed fire: use the last trade at or before the row as the signal.
            (0..=fire_row)
                .rev()
                .find_map(|r| trade_idx_at_row(series, r))
        })?;
    find_paper_exit_at(trades, fire_idx, true, fill_model)
}

/// Book a closed outcome at the run's fill-model price after fire row `j`.
#[allow(clippy::too_many_arguments)]
fn close_at_fire(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    exit: ExitCode,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    fire_row: usize,
    pricing: &Pricing,
) -> TokenOutcome {
    let fill = exit_fill(trades, series, fire_row, pricing.fill_model).unwrap_or_else(|| {
        // No mappable fire trade (should be rare): fall back to the series spot.
        PaperFill {
            trade_idx: 0,
            price: series.price[fire_row],
            token_amount: 0.0,
            slot: series.slot[fire_row].unwrap_or(0),
            block_time: series.at[fire_row],
            tx_signature: String::new(),
        }
    });
    let exit_row = series_row_for_trade_idx(series, fill.trade_idx).unwrap_or(fire_row);
    closed(
        exit,
        entry_price,
        entry_at,
        entry_slot,
        fill.price,
        fill.block_time,
        fill_trade_slot(series, exit_row),
        pricing,
    )
}

/// The scan as one call (entry then exit) — the guard test's per-token driver and
/// the single-combo drill-in's per-token driver (`simulate_generic_one_combo`).
/// Binds the rule against **this series'** columns, so a single-token caller needs no
/// separate bind step. The sweep does NOT go through here — it binds once per combo
/// (`bind_param`) and reuses that across every token, which is the whole point of
/// [`BoundCombo`].
pub(crate) fn scan(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    c: &CompiledRule,
    pricing: &Pricing,
) -> TokenOutcome {
    let bound = BoundCombo::new(series.columns(), c.clone());
    let entry = resolve_entry(trades, series, &bound, pricing);
    resolve_exit(trades, series, &bound, &entry, pricing)
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
    pricing: &Pricing,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) =
        round_trip_with_costs(entry_price, exit_price, pricing.buy_amount_sol, &pricing.cost);
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

// ──────────────── vector-kernel + classification parity (plan §P3) ────────────
//
// A vector kernel is only sound if it returns the exact same row the scalar
// predicate does — for every threshold config, at every block boundary, and for the
// non-finite values `eval_one` rejects. These tests drive crafted `price`/`dead`
// arrays (no DB, no sparse grid) so the 8-lane block loop, the `< 8` remainder tail,
// and the finite masks are each exercised at precise positions. On an AVX-512 host
// the real kernels run; on any other host the entry points delegate to their scalar
// reference, so the assertions hold either way.
//
// The classification tests are the other half of item B: a fast path that is
// *correct* but never *taken* rots silently (that is exactly what happened to the
// exit index after TP/SL desugaring), so reachability is asserted, not just equality.
#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::event::LoadedRule;
    use hunter_engine::metrics::evaluator::Condition;
    use hunter_engine::rule_params::RuleParams;

    // ── pnl-bound exit scan ────────────────────────────────────────────────────

    fn bound(op: Operator, value: f64) -> PnlBound {
        PnlBound { op, value }
    }

    /// Assert the (possibly vectorized) `first_pnl_exit_row` agrees with the scalar
    /// reference for **every** start offset — so a block-vs-remainder split at any
    /// alignment is covered.
    fn assert_agrees(price: &[f64], dead: &[bool], entry: f64, bounds: &[PnlBound]) {
        let n = price.len();
        assert_eq!(dead.len(), n, "test arrays must be parallel");
        for start in 0..=n {
            let got = first_pnl_exit_row(price, dead, start, n, entry, bounds);
            let want = first_pnl_exit_row_scalar(price, dead, start, n, entry, bounds);
            assert_eq!(
                got, want,
                "first_pnl_exit_row != scalar (start={start}, entry={entry}, \
                 bounds={bounds:?}, price={price:?}, dead={dead:?})"
            );
        }
    }

    /// Lengths straddling the 8-lane boundary: exact blocks, block+remainder, and
    /// sub-block. A cross planted at each index exercises the tzcnt first-lane pick.
    const LENS: [usize; 10] = [1, 7, 8, 9, 15, 16, 17, 24, 25, 40];

    #[test]
    fn simd_sl_cross_every_position() {
        // entry 1.0, `pnl <= −50` ⇔ price ≤ 0.5.
        let sl = [bound(Operator::Lte, -50.0)];
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 0.4;
                assert_agrees(&price, &vec![false; n], 1.0, &sl);
            }
        }
    }

    #[test]
    fn simd_tp_cross_every_position() {
        // entry 1.0, `pnl >= 100` ⇔ price ≥ 2.0.
        let tp = [bound(Operator::Gte, 100.0)];
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 2.5;
                assert_agrees(&price, &vec![false; n], 1.0, &tp);
            }
        }
    }

    #[test]
    fn simd_strict_operators_match_scalar() {
        // `>` / `<` must not fire exactly at the threshold, in vector or scalar.
        for &n in &LENS {
            for cross in 0..n {
                let mut price = vec![1.0f64; n];
                price[cross] = 2.0; // pnl == 100 exactly
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Gt, 100.0)]);
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Gte, 100.0)]);
                price[cross] = 0.5; // pnl == −50 exactly
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Lt, -50.0)]);
                assert_agrees(&price, &vec![false; n], 1.0, &[bound(Operator::Lte, -50.0)]);
            }
        }
    }

    #[test]
    fn simd_dead_every_position_beats_price() {
        // Dead has priority over a same/later price cross, and fires with no bounds.
        for &n in &LENS {
            for k in 0..n {
                let mut dead = vec![false; n];
                dead[k] = true;
                let mut price = vec![1.0f64; n];
                if k + 1 < n {
                    price[k + 1] = 0.1;
                }
                assert_agrees(&price, &dead, 1.0, &[bound(Operator::Lte, -50.0)]);
                assert_agrees(&price, &dead, 1.0, &[]); // dead-only
            }
        }
    }

    #[test]
    fn simd_both_bounds_and_no_cross() {
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        // No cross anywhere → None.
        assert_agrees(&vec![1.0f64; 33], &[false; 33], 1.0, &both);
        // SL and TP crosses on the same run — the earlier row wins (scalar order).
        let mut price = vec![1.0f64; 33];
        price[20] = 2.9; // TP
        price[9] = 0.2; // SL earlier → this one
        assert_agrees(&price, &[false; 33], 1.0, &both);
    }

    #[test]
    fn simd_non_finite_prices_never_cross() {
        // NaN / ±inf must be ignored exactly as `eval_one`'s non-finite guard does,
        // even though an ordered `−inf ≤ t` / `+inf ≥ t` compare would "match".
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        let n = 20;
        let mut price = vec![1.0f64; n];
        price[3] = f64::NAN;
        price[5] = f64::INFINITY;
        price[9] = f64::NEG_INFINITY;
        assert_agrees(&price, &vec![false; n], 1.0, &both);
        assert_eq!(first_pnl_exit_row(&price, &vec![false; n], 0, n, 1.0, &both), None);
        // Add a real finite cross after the non-finite noise: it, not the ±inf, is found.
        price[12] = 0.3;
        assert_agrees(&price, &vec![false; n], 1.0, &both);
        assert_eq!(first_pnl_exit_row(&price, &vec![false; n], 0, n, 1.0, &both), Some(12));
    }

    #[test]
    fn simd_empty_range_is_none() {
        let price = vec![1.0f64; 8];
        let dead = vec![false; 8];
        let both = [bound(Operator::Lte, -50.0), bound(Operator::Gte, 100.0)];
        assert_eq!(first_pnl_exit_row(&price, &dead, 8, 8, 1.0, &both), None);
        assert_eq!(first_pnl_exit_row(&[], &[], 0, 0, 1.0, &both), None);
    }

    // ── trailing-stop (running-peak) scan ──────────────────────────────────────

    /// Assert the (possibly vectorized) trailing scan agrees with its scalar
    /// reference for every start offset. Start matters more here than for a static
    /// threshold: the peak is seeded at `entry` and carried, so a shifted start is a
    /// genuinely different scan, not just a shifted window.
    fn assert_trailing_agrees(price: &[f64], entry: f64, op: Operator, value: f64) {
        let n = price.len();
        for start in 0..=n {
            let got = first_trailing_row_cmp(price, start, n, entry, op, value);
            let want = first_trailing_row_cmp_scalar(price, start, n, entry, op, value);
            assert_eq!(
                got, want,
                "first_trailing_row_cmp != scalar (start={start}, entry={entry}, \
                 {op:?} {value}, price={price:?})"
            );
        }
    }

    #[test]
    fn trailing_cross_every_position_after_a_run_up() {
        // Rise to a peak at `peak_at`, then dump — the retrace crossing must be found
        // at the dump row for every peak position and every array length.
        for &n in &LENS {
            for peak_at in 0..n {
                let mut price = vec![1.0f64; n];
                price[peak_at] = 4.0;
                if peak_at + 1 < n {
                    price[peak_at + 1] = 1.0; // 75% off the 4.0 peak
                }
                assert_trailing_agrees(&price, 1.0, Operator::Gte, 50.0);
                assert_trailing_agrees(&price, 1.0, Operator::Gt, 0.0);
            }
        }
    }

    #[test]
    fn trailing_peak_carries_across_block_boundaries() {
        // Peak in block 0, crossing in block 2 — only a correct carried max finds it.
        let mut price = vec![1.0f64; 24];
        price[2] = 10.0;
        price[20] = 4.0; // 60% off the 10.0 peak, two blocks later
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 50.0);
        assert_eq!(
            first_trailing_row_cmp(&price, 0, price.len(), 1.0, Operator::Gte, 50.0),
            Some(3),
            "the row right after the peak is already 90% off it"
        );
    }

    #[test]
    fn trailing_seeded_peak_is_the_entry_price() {
        // Before any run-up the peak IS the fill price, so `retrace` measures the drop
        // from entry — a soft stop. Nothing here ever exceeds entry.
        let price = vec![1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2];
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 25.0);
        assert_eq!(
            first_trailing_row_cmp(&price, 0, price.len(), 1.0, Operator::Gte, 25.0),
            Some(3),
            "0.7 is 30% below the 1.0 seed peak"
        );
    }

    #[test]
    fn trailing_non_finite_prices_neither_raise_the_peak_nor_fire() {
        for &n in &LENS {
            if n < 8 {
                continue;
            }
            let mut price = vec![2.0f64; n];
            price[1] = f64::INFINITY; // must NOT become the peak
            price[2] = f64::NAN; // must not fire
            price[3] = f64::NEG_INFINITY;
            price[n - 1] = 1.0; // 50% off the real 2.0 peak
            assert_trailing_agrees(&price, 2.0, Operator::Gte, 40.0);
            assert_eq!(
                first_trailing_row_cmp(&price, 0, n, 2.0, Operator::Gte, 40.0),
                Some(n - 1),
                "an +inf print must not inflate the peak into an early trigger"
            );
        }
    }

    #[test]
    fn trailing_no_cross_and_empty_range() {
        let price = vec![1.0f64; 33];
        assert_trailing_agrees(&price, 1.0, Operator::Gte, 1.0);
        assert_eq!(first_trailing_row_cmp(&price, 0, 33, 1.0, Operator::Gte, 1.0), None);
        assert_eq!(first_trailing_row_cmp(&price, 33, 33, 1.0, Operator::Gte, 1.0), None);
    }

    // ── bind-time classification ───────────────────────────────────────────────

    fn compiled(params: serde_json::Value) -> CompiledRule {
        CompiledRule::compile(&LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: FingerprintId(Uuid::from_u128(2)),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).expect("valid params"),
        })
    }

    #[test]
    fn tp_sl_desugars_into_classified_pnl_bounds() {
        // THE regression lock, at the classification layer: after Phase 2 a pure
        // TP/SL rule compiles to two `pnl` exit reqs, which made `has_exit_metrics()`
        // true and silently disabled every fast path. Both must classify, and the
        // combo must be `fast_exit`.
        let c = compiled(serde_json::json!({ "take_profit": 50, "stop_loss": 30 }));
        assert!(c.has_exit_metrics(), "desugaring makes the old gate true — the bug");
        let b = BoundCombo::new(&[], c);
        assert_eq!(
            b.exit_classes,
            vec![ExitClass::PnlBound { up: false }, ExitClass::PnlBound { up: true }],
            "SL is the downward bound and is prepended ahead of TP"
        );
        assert!(b.fast_exit, "a pure TP/SL rule must take the index path");
    }

    #[test]
    fn position_metrics_classify_by_kind() {
        let retrace = compiled(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }));
        let b = BoundCombo::new(&[], retrace);
        assert_eq!(b.exit_classes, vec![ExitClass::Trailing]);
        assert!(b.fast_exit);

        let held = compiled(serde_json::json!({
            "exit": { "m_position": { "held": [{ "operator": ">=", "value": 60 }] } }
        }));
        let b = BoundCombo::new(&[], held);
        assert_eq!(b.exit_classes, vec![ExitClass::HeldBound]);
        assert!(b.fast_exit);
    }

    #[test]
    fn unrecognised_shapes_stay_general() {
        // A token-scoped exit column is an arbitrary function of the series.
        let token_scoped = compiled(serde_json::json!({
            "exit": { "m_price_lifetime": { "trail": [{ "operator": ">", "value": 50 }] } }
        }));
        let b = BoundCombo::new(&[], token_scoped);
        assert_eq!(b.exit_classes, vec![ExitClass::General]);
        assert!(!b.fast_exit, "a General req forces the scalar walk for the whole rule");

        // `=` on `pnl` is a tolerance band, not a monotone bound — no prefix query.
        let eq_band = compiled(serde_json::json!({
            "exit": { "m_position": { "pnl": [{ "operator": "=", "value": 10 }] } }
        }));
        assert_eq!(BoundCombo::new(&[], eq_band).exit_classes, vec![ExitClass::General]);

        // An upper bound on `held` needs the opposite search — deliberately General.
        let held_upper = compiled(serde_json::json!({
            "exit": { "m_position": { "held": [{ "operator": "<=", "value": 60 }] } }
        }));
        assert_eq!(BoundCombo::new(&[], held_upper).exit_classes, vec![ExitClass::General]);

        // Multi-arm DNF on `pnl` is an OR of bands, not one crossing.
        let mut multi = compiled(serde_json::json!({ "take_profit": 50 }));
        multi.exit_reqs[0].conds = vec![
            vec![Condition { operator: Operator::Gte, value: 50.0 }],
            vec![Condition { operator: Operator::Lte, value: -20.0 }],
        ];
        assert_eq!(BoundCombo::new(&[], multi).exit_classes, vec![ExitClass::General]);
    }

    #[test]
    fn mixed_classes_stay_fast_but_a_general_req_does_not() {
        // TP/SL + a trailing stop: every req classified ⇒ the index path resolves all
        // three (hull, hull, vectorized scan) and picks the earliest row.
        let mixed = compiled(serde_json::json!({
            "take_profit": 50,
            "stop_loss": 30,
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }));
        let b = BoundCombo::new(&[], mixed);
        assert!(b.fast_exit);
        assert_eq!(b.exit_classes.len(), 3);
        assert_eq!(b.exit_classes[2], ExitClass::Trailing);

        // Add one token-scoped req and the whole rule drops to scalar.
        let with_general = compiled(serde_json::json!({
            "take_profit": 50,
            "exit": { "m_price_lifetime": { "trail": [{ "operator": ">", "value": 50 }] } }
        }));
        assert!(!BoundCombo::new(&[], with_general).fast_exit);
    }
}
