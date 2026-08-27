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
use hunter_engine::deadness::{DEAD_QUIET_SECS, TAIL_MARGIN_SECS};
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::evaluator::{eval, Condition, Operator};
use hunter_engine::metrics::grid::{estimate_sparse_rows as grid_estimate_rows, fold_sparse};
/// The sparse tick grid moved to `hunter-engine` so the sweep precompute and the
/// metric-series chart endpoint drive a `MetricSeries` through the ONE loop (a
/// trade-only fold silently mis-samples every time-decaying metric). Re-exported
/// here because the axes model and the discovery screen build one.
pub use hunter_engine::metrics::grid::SparseGrid;
use hunter_engine::metrics::position::{
    is_trailing, position_value, trailing_armed, PositionCtx,
};
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn};
use hunter_engine::metrics::{MetricId, TradeLite, Ts};
use hunter_engine::TICK_MS;

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::{
    round_trip_multi_leg, round_trip_with_costs, CostModel, ExitCode, ExitLeg,
};
use trading_core::strategies::paper_fill::{
    find_paper_entry_at, find_paper_exit_at, FillModel, PaperFill,
};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod, TokenOutcome};

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
    /// The corpus-wide frozen-tail resolve horizon (D1) — `min(as_of, corpus_last_trade
    /// + DEAD_QUIET + TAIL_MARGIN)`, the same tail cap `run_replay` bounds its tick loop
    /// by. `None` (the default) leaves the legacy per-token bounded-tail behavior:
    /// [`set_corpus`] opts a run in, computing it from the corpus this strategy scans.
    /// See [`resolve_frozen_tail`].
    corpus_horizon: Option<Ts>,
    pricing: Pricing,
    /// The union of every axis's precompute columns (built once).
    columns: Vec<SeriesColumn>,
    /// The sparse-grid horizons derived from the swept axes — sizes the precompute
    /// so a long-lived token records rows only where a decision could change
    /// (plan §P2). Built once; the same grid serves every token.
    grid: SparseGrid,
    /// Corpus-wide volume-ix patterns (compiled). `None` ⇒ no flow state / NaN.
    flow_patterns: Option<hunter_engine::metrics::flow_split::FlowPatterns>,
    /// When set, [`compile_combo`](Self::compile_combo) merges this ladder into every
    /// combo's `RuleParams` (Pass-2 overlay scan only). Leave `None` on the main
    /// Pass-1 strategy so the axes grid keeps `fast_exit`. One overlay clone exists
    /// per candidate ladder in [`ScaleOutPass2::variants`] — see [`with_overlay`].
    scale_out_overlay: Option<Vec<hunter_engine::rule_params::ExitStage>>,
    /// Pass-2 config: after each group's cheap fold, re-score top-K combos against
    /// every ladder in [`ScaleOutPass2::variants`]. `None` ⇒ skip Pass 2.
    scale_out_pass2: Option<ScaleOutPass2>,
}

/// A small **grid** of candidate scale-out ladders + per-group top-K for Pass 2
/// (see `docs/arch/sweep.md`, *Pass-2 overlay*). Dynamic, not fixed: each
/// top-K combo is independently re-scored under its own baseline (no ladder) PLUS
/// every ladder here, and keeps whichever wins — a combo the ladder doesn't help
/// stays on its own Pass-1 exit. Bounded cost: `variants.len() + 1` staged scans
/// per top-K combo, never a swept axis over the whole grid.
#[derive(Clone, Debug)]
pub struct ScaleOutPass2 {
    pub variants: Vec<Vec<hunter_engine::rule_params::ExitStage>>,
    pub top_k: usize,
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
        Self {
            model,
            as_of,
            corpus_horizon: None,
            pricing,
            columns,
            grid,
            flow_patterns,
            scale_out_overlay: None,
            scale_out_pass2: None,
        }
    }

    /// Enable Pass 2: after each group ranks on the cheap axes path, re-score its
    /// top-`top_k` combos against every ladder in `variants` (plus each combo's own
    /// baseline) and keep whichever wins per combo.
    pub fn set_scale_out_pass2(
        &mut self,
        variants: Vec<Vec<hunter_engine::rule_params::ExitStage>>,
        top_k: usize,
    ) {
        self.scale_out_pass2 = Some(ScaleOutPass2 {
            variants,
            top_k: top_k.max(1),
        });
    }

    /// Clone this strategy with a compile-time scale_out overlay (no nested Pass 2).
    fn with_overlay(&self, stages: Vec<hunter_engine::rule_params::ExitStage>) -> Self {
        Self {
            model: self.model.clone(),
            as_of: self.as_of,
            corpus_horizon: self.corpus_horizon,
            pricing: self.pricing,
            columns: self.columns.clone(),
            grid: self.grid,
            flow_patterns: self.flow_patterns.clone(),
            scale_out_overlay: Some(stages),
            scale_out_pass2: None,
        }
    }

    /// Opt this run into the analytic frozen-tail resolve (D1) by anchoring it to the
    /// corpus it scans: sets [`corpus_horizon`](Self::corpus_horizon) to the same tail
    /// cap `run_replay` uses (`min(as_of, corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`),
    /// so a deterministic clock exit that lands past a token's OWN per-token series cut
    /// still closes in the sweep — matching simulate over the same tokens. Call it once,
    /// with the whole token set, before scanning. A trade-less corpus leaves it `None`.
    pub fn set_corpus(&mut self, tokens: &[CorpusToken]) {
        self.corpus_horizon = frozen_tail_horizon(self.as_of, tokens);
    }

    /// The precompute columns this strategy records per token (the union its axes
    /// read), in the order [`BoundCombo`] resolves indices against.
    pub fn columns(&self) -> &[SeriesColumn] {
        &self.columns
    }

    /// The sparse-grid horizons that size this strategy's tick stream.
    pub fn grid(&self) -> SparseGrid {
        self.grid
    }

    /// Widen the precompute to a **superset** of this strategy's own columns and
    /// horizons, so several strategies can share ONE per-token [`MetricSeries`].
    ///
    /// This is the additive-scan seam the discovery screen is built on
    /// (`lab::discovery::screen`, plan §6.1 / D2): N one-metric strategies over the
    /// same cohort would otherwise each rebuild the series — N precompute passes for
    /// what is by construction one union of columns. Given the union, `prepare_token`
    /// on *any* of them yields a series every other one can scan, so the corpus is
    /// precomputed once and the N screens are pure scan.
    ///
    /// Widening is decision-neutral: extra columns are recorded and never read (the
    /// combo's `BoundCombo` resolves only the reqs its own rule carries), and a wider
    /// grid horizon only emits ticks the sparse grid would otherwise have proved
    /// static — the same values a dense series carries. A **narrower** set would drop
    /// columns the scan reads (`MISSING_COL` ⇒ silently unsatisfiable conditions), so
    /// the superset relation is asserted, not assumed.
    pub fn share_precompute(&mut self, columns: Vec<SeriesColumn>, grid: SparseGrid) {
        debug_assert!(
            self.columns.iter().all(|c| columns.contains(c)),
            "share_precompute needs a superset of this model's own columns",
        );
        debug_assert!(
            grid.max_window_secs >= self.grid.max_window_secs
                && grid.time_horizon_secs >= self.grid.time_horizon_secs
                && grid.stall_horizon_secs >= self.grid.stall_horizon_secs,
            "share_precompute needs horizons at least as wide as this model's own",
        );
        self.columns = columns;
        self.grid = grid;
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
        let mut params = self.model.combo_params(idx);
        if let Some(stages) = &self.scale_out_overlay {
            params.scale_out = Some(stages.clone());
        }
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
            entry_enabled: true,
        };
        CompiledRule::compile(&loaded)
    }

    fn combo(&self, idx: usize) -> GenericCombo {
        GenericCombo { idx }
    }

    /// The resolved axes this strategy sweeps.
    pub fn model(&self) -> &AxesModel {
        &self.model
    }

    /// Per-axis value counts, in combo-significance order (index 0 most significant).
    fn axis_lens(&self) -> Vec<usize> {
        self.model.axes.iter().map(|a| a.value_count()).collect()
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
    type EntryCands = EntryCandidates;
    type TokenState = MetricSeries;
    type BoundParams = BoundCombo;
    type ExitCtx = super::exit_index::ExitIndex;
    /// The fill row the hulls are anchored on, or `None` when this combo wants no
    /// index at all — the exact pair [`build_exit_ctx`](Self::build_exit_ctx)
    /// branches on, so "cleared" is a distinct key and a later fast-exit combo on the
    /// same fill row still gets its rebuild.
    type ExitCtxKey = Option<usize>;

    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey {
        self.model.entry_key(params.idx)
    }

    fn exit_ctx_key(&self, bound: &Self::BoundParams, entry: &Self::Entry) -> Self::ExitCtxKey {
        match entry {
            EntryResolution::Entered { fill_row, .. } if wants_exit_index(bound, entry) => {
                Some(*fill_row)
            }
            _ => None,
        }
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

    fn entry_candidates(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
        out: &mut Self::EntryCands,
    ) {
        entry_candidates(series, bound, out)
    }

    fn resolve_entry_from(
        &self,
        trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
        cands: &mut Self::EntryCands,
    ) -> Self::Entry {
        resolve_entry_from(trades, series, bound, cands, &self.pricing)
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
        // AVX-512 toggle still selectable . Default path
        // is the O(log n) index; SIMD remains for A/B. Both must match scalar.
        if crate::sweep::registry::use_simd() {
            resolve_exit_simd(trades, series, bound, entry, &self.pricing, ctx, self.corpus_horizon)
        } else {
            resolve_exit_indexed(trades, series, bound, entry, &self.pricing, ctx, self.corpus_horizon)
        }
    }

    fn params_json(&self, params: &Self::Params) -> serde_json::Value {
        self.model.combo_params(params.idx).to_value()
    }

    fn token_state_bytes_estimate(&self, token: &CorpusToken) -> usize {
        self.series_bytes_estimate(token)
    }

    fn post_group_rescore(
        &self,
        _params: &[Self::Params],
        corpus: &crate::sweep::corpus::Corpus,
        token_idx: &[usize],
        gr: &mut crate::sweep::grouped_engine::GroupResult,
        coverage: crate::sweep::grouped_engine::CoverageFloor,
        observer: &dyn crate::sweep::progress::SweepObserver,
    ) -> anyhow::Result<()> {
        use crate::sweep::aggregate::ComboAgg;
        use crate::sweep::grouped_engine::{best_combo, top_combo_ids};
        use anyhow::bail;

        let Some(pass2) = &self.scale_out_pass2 else {
            return Ok(());
        };
        let top = top_combo_ids(&gr.metrics, pass2.top_k);
        if top.is_empty() || pass2.variants.is_empty() {
            return Ok(());
        }
        // One overlay strategy per candidate ladder — built once, reused across every
        // top-K combo in this group (the ladder is fixed; only the combo varies).
        let overlays: Vec<Self> =
            pass2.variants.iter().map(|v| self.with_overlay(v.clone())).collect();
        for &orig_id in &top {
            if observer.cancelled() {
                bail!("sweep cancelled");
            }
            let combo = GenericCombo { idx: orig_id as usize };
            // The combo's OWN Pass-1 result is baseline candidate #0 — a ladder must
            // beat the combo's own exit to be adopted, not just "do okay". This is
            // what makes the grid dynamic per combo rather than a blanket overlay.
            let baseline = gr
                .metrics
                .iter()
                .find(|m| m.combo_id == orig_id)
                .cloned()
                .expect("top_combo_ids only returns ids present in gr.metrics");
            let mut best: crate::sweep::aggregate::ComboMetrics = baseline.clone();
            let mut best_variant: Option<usize> = None;
            for (vi, overlay) in overlays.iter().enumerate() {
                if observer.cancelled() {
                    bail!("sweep cancelled");
                }
                let bound = overlay.bind_param(&combo);
                let mut agg = ComboAgg::default();
                let mut ctx = Self::ExitCtx::default();
                let mut cands = Self::EntryCands::default();
                let mut last_exit_key: Option<Self::ExitCtxKey> = None;
                for &ti in token_idx {
                    if observer.cancelled() {
                        bail!("sweep cancelled");
                    }
                    let token = &corpus.tokens[ti];
                    let state = overlay.prepare_token(token);
                    let trades = &token.trades;
                    let entry =
                        overlay.resolve_entry_from(trades, &state, &bound, &combo, &mut cands);
                    let exit_key = overlay.exit_ctx_key(&bound, &entry);
                    if last_exit_key.as_ref() != Some(&exit_key) {
                        overlay.build_exit_ctx(trades, &state, &bound, &entry, &combo, &mut ctx);
                        last_exit_key = Some(exit_key);
                    }
                    let outcome =
                        overlay.resolve_exit(trades, &state, &bound, &entry, &combo, &ctx);
                    agg.record(&outcome);
                }
                let mut m = agg.finalize(orig_id);
                m.rescore_for_group(gr.token_count);
                if pass2_candidate_wins(&m, &best) {
                    best = m;
                    best_variant = Some(vi);
                }
            }
            if let Some(vi) = best_variant {
                if let Some(slot) = gr.metrics.iter_mut().find(|x| x.combo_id == orig_id) {
                    *slot = best;
                }
                // `ExitStage` has no `Serialize` impl of its own (see `rule_params`'s
                // module docs) — round-trip through `RuleParams::to_value`, the one
                // canonical stage-JSON writer, instead of hand-rolling a second one.
                let wrapper = hunter_engine::rule_params::RuleParams {
                    scale_out: Some(pass2.variants[vi].clone()),
                    ..Default::default()
                };
                let ladder_json = wrapper
                    .to_value()
                    .get("scale_out")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                gr.scale_out_winners.insert(orig_id, ladder_json);
            }
            // Else: no candidate beat this combo's own exit — `gr.metrics` already
            // holds its Pass-1 baseline untouched, and it carries no `scale_out`.
        }
        let (best_combo_id, best_score, best_expectancy_sol) =
            best_combo(&gr.metrics, gr.token_count, coverage);
        gr.best_combo_id = best_combo_id;
        gr.best_score = best_score;
        gr.best_expectancy_sol = best_expectancy_sol;
        Ok(())
    }
}

/// Whether one Pass-2 candidate's rescored metrics beat the current best (which
/// starts as the combo's own Pass-1 baseline) — the one decision
/// [`GenericSweepStrategy::post_group_rescore`] makes per (combo, candidate).
/// Factored out as a pure fn (no corpus/observer args) so it's unit-testable
/// without a scan: the grid-search "dynamic, per combo" behavior lives entirely
/// in this one comparison, not in the scanning loop around it.
fn pass2_candidate_wins(
    candidate: &crate::sweep::aggregate::ComboMetrics,
    current_best: &crate::sweep::aggregate::ComboMetrics,
) -> bool {
    crate::sweep::grouped_engine::rank_combo(candidate, current_best) == std::cmp::Ordering::Greater
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
    // Static token facts first, exactly as the live `TokenCreated` arm orders them.
    series.seed_ix_count(token.fp.ix_labels.len());
    if let Some(patterns) = flow_patterns {
        let windows: Vec<hunter_engine::metrics::WindowSpec> = series
            .columns()
            .iter()
            // Both axes of a dynamic column: a two-window group needs a buffer for
            // each, and registering only the primary leaves the second read NaN.
            .flat_map(|c| match c {
                SeriesColumn::Flow(_, w, _) => vec![*w],
                SeriesColumn::Window(_, w) => vec![w.primary, w.secondary],
                _ => vec![],
            })
            .flatten()
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
    // The tail cut stays at **this token's own** `last_trade + DEAD_QUIET + TAIL_MARGIN`
    // (the shared driver's cap) — extending it to the corpus-wide horizon would cost the
    // RAM the sparse-grid design exists to bound (and would fire exits a single-token
    // replay never does, which `guard::scan_matches_replay_stall_eq_exit_across_gap`
    // locks against).
    //
    // The D1 asymmetry this leaves — a still-liquid quiet token whose deterministic
    // `time`/`stall`/`held` clock crosses in the gap between this per-token cut and the
    // **corpus-wide** tail `run_replay` ticks to — is resolved not here (by ticking) but
    // in [`resolve_frozen_tail`] (by an O(1) analytic crossing at the frozen price),
    // which the scan calls before reporting `Open`. So the series builder and the guard
    // are unchanged; the extra decision lives entirely on the scan side.
    //
    // Unbounded (`max_rows: None`): the sweep admits by an up-front
    // [`estimate_sparse_rows`] budget instead, so a truncated series never reaches a scan.
    fold_sparse(
        &mut series,
        created,
        token.trades.iter().map(|ct| (trade_lite(ct), Some(ct.slot))),
        grid,
        as_of,
        None,
    );
    series
}

/// Worst-case row count of a token's sparse series — the admission estimate
/// (plan §P4). Thin wrapper over the shared [`estimate_sparse_rows`] in terms of a
/// [`CorpusToken`]'s trade clock.
pub(crate) fn estimate_sparse_rows(token: &CorpusToken, grid: &SparseGrid, as_of: Ts) -> usize {
    grid_estimate_rows(
        token.created_at,
        token.trades.iter().map(|ct| ct.block_time),
        grid,
        as_of,
    )
}

fn trade_lite(ct: &CorpusTrade) -> TradeLite {
    crate::sweep::projection::to_trade_lite(ct)
}

/// The union of precompute columns a compiled rule reads (both sides + every
/// scale-out stage). Used by the guard test to build a series for one rule without
/// the axes model.
pub(crate) fn columns_for(compiled: &CompiledRule) -> Vec<SeriesColumn> {
    let mut cols = Vec::new();
    let stage_reqs = compiled.scale_out.iter().flat_map(|s| s.reqs.iter());
    for req in compiled.entry_reqs.iter().chain(compiled.exit_reqs.iter()).chain(stage_reqs) {
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
        // The grid is a WALL clock, so a slot span converts at the nominal slot time.
        // This only sizes the horizon, never a reading.
        .map(|w| match w.unit {
            hunter_engine::metrics::WindowUnit::Sec => w.size + w.lag,
            hunter_engine::metrics::WindowUnit::Slot => {
                (w.size + w.lag) * hunter_engine::metrics::NOMINAL_SLOT_SECS
            }
        })
        .fold(0.0_f64, f64::max);
    // Max condition value + eq-tolerance for a monotone/static metric across both
    // sides and every scale-out stage (a remainder `held >= N` must size the grid).
    let ceiling = |metric: MetricId| -> f64 {
        let mut max = 0.0_f64;
        let mut found = false;
        let stage_reqs = compiled.scale_out.iter().flat_map(|s| s.reqs.iter());
        for req in compiled.entry_reqs.iter().chain(compiled.exit_reqs.iter()).chain(stage_reqs)
        {
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
    // Mirrors `readout::req_column`: the WHOLE carrier on the windowed arm, so a
    // two-window req keeps its second axis instead of scanning against NaN.
    match (req.fingerprint, req.window.is_windowed()) {
        (Some(fp), _) => SeriesColumn::Flow(req.metric, req.window.primary, fp),
        (None, true) => SeriesColumn::Window(req.metric, req.window),
        (None, false) => SeriesColumn::Static(req.metric),
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
        arm_above_pct: None,
    };
    col_idx_in(columns, &req)
}

/// A compiled combo **plus its resolved series-column indices**.
///
/// Every token's series is built with `self.columns.clone()` (see `prepare_token`),
/// so the column layout is fixed for the whole run and a combo's column indices are
/// the same on every token. Resolving them inside `resolve_entry` / `resolve_exit`
/// instead costs **one resolve per (token, combo)** — `resolve_exit` is uncached and
/// runs for every combo on every token, which makes it the single most-executed heap
/// allocation in a sweep. Binding here makes it once per combo.
///
/// The precomputed indices are only valid while that fixed-columns invariant holds;
/// a `debug_assert` in each scan re-derives them from the series and would fail loudly
/// in tests if a future change made the column set vary per token.
pub struct BoundCombo {
    pub(crate) rule: CompiledRule,
    entry_cols: Vec<usize>,
    mono_cols: Vec<usize>,
    exit_cols: Vec<usize>,
    /// Per scale-out stage: column indices for that stage's `reqs` (lockstep with
    /// [`CompiledRule::scale_out`]). Empty when the rule has no scale-out.
    stage_cols: Vec<Vec<usize>>,
    /// One [`ExitClass`] per `rule.exit_reqs`, in lockstep — how the fast paths may
    /// resolve each req's first firing row.
    exit_classes: Vec<ExitClass>,
    /// For each `rule.exit_reqs[i]`: `Some((metric, operator, value, slot))` when
    /// that req is `ReqOrigin::Authored` (an `ExitCode::Metrics` exit), else
    /// `None` for a desugared TP/SL req. `operator`/`value` are the req's first
    /// OR-arm's first AND-condition — the same simplification
    /// `hunter_engine::arm::exit_fired`'s `first_satisfied_cond` makes when it
    /// stamps a label (a rare multi-arm DNF can report a different arm than the
    /// one that actually fired; the metric name is always correct). `slot` is
    /// 0-based among this rule's own authored exit reqs, capped at
    /// `N_EXIT_METRIC_SLOTS - 1` so the aggregate's per-combo counters stay a
    /// fixed size. Bind-time only — depends solely on the compiled rule, so
    /// resolving an exit looks this up instead of computing anything per token.
    exit_metric_label: Vec<Option<(MetricId, Operator, f64, Option<hunter_engine::metrics::WindowSpec>, u8)>>,
    /// `true` ⇔ **every** exit req classified (no [`ExitClass::General`]) **and**
    /// the rule has no `scale_out` — so the index / SIMD paths can resolve the
    /// whole exit as a single full-bag close. Scale-out forces the staged scalar
    /// walk (plan §5); one `General` req forces the scalar walk for the entire
    /// rule.
    pub(crate) fast_exit: bool,
    /// `true` ⇔ some exit req could hold *before* entry and so veto it (the
    /// `can_enter` gate). `false` ⇒ this combo's entry is a pure function of the
    /// shared [`EntryCandidates`] — the pure-TP/SL shape, which the two-stage entry
    /// must not tax. Computed conservatively at bind time; see [`BoundCombo::new`].
    entry_veto_possible: bool,
}

impl BoundCombo {
    /// Bind `rule` against the run's fixed `columns`.
    pub(crate) fn new(columns: &[SeriesColumn], rule: CompiledRule) -> Self {
        let entry_cols = resolve_cols_in(columns, &rule.entry_reqs);
        let exit_cols = resolve_cols_in(columns, &rule.exit_reqs);
        let stage_cols: Vec<Vec<usize>> =
            rule.scale_out.iter().map(|s| resolve_cols_in(columns, &s.reqs)).collect();
        let mono_cols = rule.mono_kills.iter().map(|k| col_idx_mono(columns, k)).collect();
        let exit_classes: Vec<ExitClass> = rule.exit_reqs.iter().map(classify_exit_req).collect();
        let exit_metric_label = exit_metric_labels(&rule.exit_reqs);
        let fast_exit = rule.scale_out.is_empty()
            && !exit_classes.iter().any(|c| matches!(c, ExitClass::General));
        // Can any exit req hold at a row *before* entry, i.e. veto it? Conservative
        // by construction: a req that reads a recorded column might, and a req with
        // no conditions is vacuously true even on the `NaN` an unrecorded column
        // reads. Everything else — every position-scoped req under the sweep's own
        // column set, which is what a pure TP/SL combo is made of — provably cannot,
        // so those combos skip the veto eval entirely (see `resolve_entry_from`).
        // Scale-out stages do not participate in `can_enter` (engine mirrors this).
        let entry_veto_possible = rule
            .exit_reqs
            .iter()
            .zip(&exit_cols)
            .any(|(r, &col)| col != MISSING_COL || eval(&r.conds, f64::NAN, r.tolerance));
        Self {
            rule,
            entry_cols,
            mono_cols,
            exit_cols,
            stage_cols,
            exit_classes,
            exit_metric_label,
            fast_exit,
            entry_veto_possible,
        }
    }
}

/// Precompute [`BoundCombo::exit_metric_label`] — one entry per `exit_reqs`,
/// `Some` only for `ReqOrigin::Authored` reqs, slot-numbered among just those.
/// Bind-time only (never per token/row).
///
/// `pub(crate)` so the **replay** path can number a rule's authored exit reqs by the
/// same rule the sweep numbers them by. A `run_replay` outcome carries an
/// `ExitReason::Metrics { metric, operator, value, window }` but no slot, and a
/// second slot-numbering implementation would let a replay-sourced attribution
/// disagree with the sweep's `n_exit_metrics_by_slot` on the same rule.
pub(crate) fn exit_metric_labels(
    exit_reqs: &[MetricReq],
) -> Vec<Option<(MetricId, Operator, f64, Option<hunter_engine::metrics::WindowSpec>, u8)>> {
    let mut authored_slots: u8 = 0;
    exit_reqs
        .iter()
        .map(|r| {
            if r.origin != ReqOrigin::Authored {
                return None;
            }
            let slot = authored_slots.min(crate::sweep::strategy::N_EXIT_METRIC_SLOTS as u8 - 1);
            authored_slots = authored_slots.saturating_add(1);
            r.conds
                .first()
                .and_then(|arm| arm.first())
                .map(|c| (r.metric, c.operator, c.value, r.window.primary, slot))
        })
        .collect()
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
    /// `m_position.bounce` — needs the running since-entry trough (mirror of
    /// [`Trailing`]). **O(n)** scalar scan (see `first_bounce_row`).
    Bounce,
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
    // An ARMED trailing req is not a prefix query: it fires at the first row where
    // `retrace >= t` **and** `pnl >= gate`, a conjunction of two different running
    // quantities that the extrema hulls do not index. Send it to the scalar walk
    // rather than weaken the hull — a correct scalar walk beats a clever wrong index.
    if req.arm_above_pct.is_some() && is_trailing(req.metric) {
        return ExitClass::General;
    }
    match req.metric {
        // A linear scan evaluates any condition shape correctly, so `retrace`
        // classifies unconditionally.
        MetricId::Retrace => ExitClass::Trailing,
        MetricId::Bounce => ExitClass::Bounce,
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
            && c
                .rule
                .scale_out
                .iter()
                .zip(&c.stage_cols)
                .all(|(s, cols)| s.reqs.iter().zip(cols).all(|(r, &col)| col == col_idx_of(series, r)))
            && c.rule.mono_kills.iter().zip(&c.mono_cols).all(|(k, &col)| {
                let req = hunter_engine::arm::MetricReq {
                    metric: k.metric,
                    window: k.window,
                    fingerprint: k.fingerprint,
                    tolerance: 0.0,
                    conds: vec![],
                    position_scoped: false,
                    origin: hunter_engine::arm::ReqOrigin::Authored,
                    arm_above_pct: None,
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
    // Mirrors `CompiledRule::exit_fired`: a trailing req held off until the position
    // is `arm_above_pct` in profit does not fire at all while disarmed.
    if !trailing_armed(req.arm_above_pct, ctx, series.price[row]) {
        return false;
    }
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
/// Returns the winning `ExitCode` **and** its index into `reqs` — the caller
/// looks the index up against `BoundCombo::exit_metric_label` to stamp a
/// `ExitCode::Metrics` outcome with its authored metric/operator/value/slot.
fn first_exit_req_fired(
    series: &MetricSeries,
    reqs: &[MetricReq],
    cols: &[usize],
    row: usize,
    ctx: &PositionCtx,
) -> Option<(ExitCode, usize)> {
    reqs.iter()
        .zip(cols)
        .enumerate()
        .find(|(_, (r, &col))| exit_req_fires(series, r, col, row, ctx))
        .map(|(i, (r, _))| (exit_code_of(r.origin), i))
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
/// the trigger. Analysis path: empty fill window market-fills at the trigger
/// (`market_fill_on_empty_window: true`, matching replay); a zero-price trigger
/// still yields `NoEntry`.
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
            return entry_fill_at(trades, series, i, pricing);
        }
    }
    EntryResolution::NoEntry
}

/// The fill half of an entry decision at series `row`: trigger trade → the run's
/// [`FillModel`] fill → the series row it lands on. `NoEntry` when any of the three
/// is unresolvable — a **terminal** answer for the token (that is what the walk in
/// [`resolve_entry`] does at its first admissible row), not a reason to look at a
/// later candidate.
///
/// Shared by the fused walk and Stage B so the two can never price an entry
/// differently.
fn entry_fill_at(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    row: usize,
    pricing: &Pricing,
) -> EntryResolution {
    let Some(trigger_idx) = entry_trigger_trade_idx(series, row) else {
        return EntryResolution::NoEntry;
    };
    let Some(fill) = find_paper_entry_at(trades, trigger_idx, true, pricing.fill_model) else {
        return EntryResolution::NoEntry;
    };
    let Some(fill_row) = series_row_for_trade_idx(series, fill.trade_idx) else {
        return EntryResolution::NoEntry;
    };
    EntryResolution::Entered { fill_row, price: fill.price, at: fill.block_time }
}

// ───────────────── two-stage entry (Stage A candidates / Stage B veto) ─────────────
//
// `resolve_entry` above is exit-**dependent**: the `can_enter` veto skips a row where
// the combo's own exit conditions already hold. The fold's cache is keyed by entry
// picks only, so caching the *resolved* entry made the first combo of each entry class
// donate its entered set to every sibling — wrong counts, rows and prices on any sweep
// with exit-side metric axes (the 2026-07-26 poisoning bug; `sim-parity.md`).
//
// Splitting the walk fixes it for near-free, because everything expensive in it is
// exit-independent: the dead check, the mono-kill check and the entry-condition eval
// all read entry-side state only. Only the veto reads the exit side, and only at rows
// where the entry conditions already hold — few, for any selective entry.

/// **Stage A** — the exit-independent half of the entry walk for one (token, entry
/// class), shared by every combo in that class.
///
/// The walk is **resumable, not eager**. [`resolve_entry`] stops at the first row it
/// can enter on, and a rule whose entry condition holds on thousands of rows must keep
/// that short-circuit: pre-computing every candidate would trade one bug for a
/// pathological scan. So Stage A only *opens* the walk; Stage B drives it one candidate
/// at a time and the progress is what the class shares. A class where nothing is vetoed
/// therefore walks exactly as far as the old cache did — to the first candidate — and a
/// vetoing combo's deeper walk is inherited by its siblings instead of being redone.
///
/// Buffers are reused in place across combos, tokens and classes (the engine keeps one
/// instance per token scan), so steady state allocates nothing.
#[derive(Default)]
pub struct EntryCandidates {
    /// `true` ⇔ the rule enters on arm, so *every* examined row is a candidate.
    /// Recording those as `rows` would cost one `u32` per series row to describe a
    /// range the index arithmetic already does.
    all_rows: bool,
    /// Next row the shared walk will examine.
    next_row: usize,
    /// Candidate rows found so far (ascending). Unused when [`Self::all_rows`].
    rows: Vec<u32>,
    /// Where the walk ended: the first row that is dead or mono-killed (exactly where
    /// [`resolve_entry`] returns `NoEntry`), or `n_rows` if it ran off the end. `None`
    /// while the walk can still be resumed.
    stopped_at: Option<usize>,
    /// [`entry_fill_at`] memo, keyed by the admissible row that produced it. Combos in
    /// a class overwhelmingly land on the same row, so this is what keeps Stage B at
    /// O(1) once the first combo has paid for the fill. Capacity-capped: past
    /// [`FILL_MEMO_CAP`] distinct rows the linear probe stops being cheaper than the
    /// fill it saves, so extra rows simply re-resolve.
    fills: Vec<(u32, EntryResolution)>,
}

/// Distinct admissible rows one entry class memoizes fills for. A class with more
/// than this many *distinct* entry rows is already paying a deep veto walk per combo;
/// keeping the probe short matters more there than saving the fill.
const FILL_MEMO_CAP: usize = 32;

impl EntryCandidates {
    /// Begin a fresh entry class (buffers reused, nothing walked yet).
    fn open(&mut self, enter_on_arm: bool) {
        self.all_rows = enter_on_arm;
        self.next_row = 0;
        self.rows.clear();
        self.stopped_at = None;
        self.fills.clear();
    }

    /// Examine one more row of the shared walk. Reads `dead`, the mono-kills and the
    /// entry reqs — **entry-side only**. An exit-side read here is exactly the
    /// poisoning bug; see the section comment.
    fn step(&mut self, series: &MetricSeries, b: &BoundCombo) {
        let i = self.next_row;
        if i >= series.n_rows() {
            self.stopped_at = Some(i);
            return;
        }
        if series.dead[i] || entry_unsatisfiable(series, &b.rule, &b.mono_cols, i) {
            self.stopped_at = Some(i);
            return;
        }
        if !self.all_rows && reqs_satisfied(series, &b.rule.entry_reqs, &b.entry_cols, i) {
            self.rows.push(i as u32);
        }
        self.next_row = i + 1;
    }

    /// The `k`-th candidate row (ascending), resuming the shared walk as far as needed.
    /// `None` once the walk has ended before reaching a `k`-th candidate.
    ///
    /// `b` must be a combo of the class this instance was [`opened`](Self::open) for —
    /// combos sharing a `Strategy::entry_key` share their whole entry side, which is
    /// what makes resuming another combo's walk sound.
    fn nth(&mut self, k: usize, series: &MetricSeries, b: &BoundCombo) -> Option<usize> {
        if self.all_rows {
            while self.stopped_at.is_none() && self.next_row <= k {
                self.step(series, b);
            }
            match self.stopped_at {
                Some(stop) if k >= stop => None,
                _ => Some(k),
            }
        } else {
            while self.stopped_at.is_none() && self.rows.len() <= k {
                self.step(series, b);
            }
            self.rows.get(k).map(|&r| r as usize)
        }
    }

    /// Memoized [`entry_fill_at`] for one admissible row.
    fn fill_at(&mut self, row: usize, compute: impl FnOnce() -> EntryResolution) -> EntryResolution {
        let key = row as u32;
        if let Some((_, hit)) = self.fills.iter().find(|(k, _)| *k == key) {
            return *hit;
        }
        let resolved = compute();
        if self.fills.len() < FILL_MEMO_CAP {
            self.fills.push((key, resolved));
        }
        resolved
    }
}

/// **Stage A** — open the shared, resumable entry walk for one (token, entry class).
/// Called once per class per token; [`resolve_entry_from`] drives it.
pub(crate) fn entry_candidates(series: &MetricSeries, b: &BoundCombo, out: &mut EntryCandidates) {
    debug_assert_cols_match(series, b);
    out.open(b.rule.enter_on_arm());
}

/// **Stage B** — this combo's entry, resolved out of the shared [`EntryCandidates`]
/// by applying its own `can_enter` veto.
///
/// Byte-identical to [`resolve_entry`] by construction: it evaluates the same veto
/// predicate at the same rows, in the same order, and prices the first admissible one
/// through the same [`entry_fill_at`]. (Asserted, not assumed — see the `cfg(test)`
/// check below, which runs in every guard.)
///
/// Cost per combo:
/// * vacuous veto (pure TP/SL — position-scoped reqs read `NaN` before entry and can
///   never fire): the class's first candidate + a memo hit, i.e. O(1) after the first
///   combo. This is the 1M-combo shape, and it must stay untaxed.
/// * token-scoped exit axes: one `reqs_any_satisfied` per vetoed candidate, plus
///   whatever the shared walk still has to advance — work the old code did per combo
///   too, only now it is shared and it lands on the *right* combo.
pub(crate) fn resolve_entry_from(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    cands: &mut EntryCandidates,
    pricing: &Pricing,
) -> EntryResolution {
    debug_assert_cols_match(series, b);
    let mut k = 0usize;
    let resolved = loop {
        let Some(row) = cands.nth(k, series, b) else { break EntryResolution::NoEntry };
        // Mirror `CompiledRule::can_enter`: never buy while exit metrics already hold.
        if b.entry_veto_possible && reqs_any_satisfied(series, &b.rule.exit_reqs, &b.exit_cols, row)
        {
            k += 1;
            continue;
        }
        break cands.fill_at(row, || entry_fill_at(trades, series, row, pricing));
    };
    #[cfg(test)]
    {
        // The equivalence the split rests on, checked against the fused reference on
        // every combo the tests fold. Deliberately `cfg(test)` and not `debug_assert`:
        // the reference is the O(n) walk this stage exists to avoid, so re-running it
        // per combo would make a plain debug `cargo run` sweep pay the very cost the
        // cache removes — while the guards, which are what must catch a drift, still
        // check every single resolution.
        let reference = resolve_entry(trades, series, b, pricing);
        assert!(
            same_entry(&resolved, &reference),
            "two-stage entry disagreed with the fused reference: {resolved:?} vs {reference:?}"
        );
    }
    resolved
}

/// Exact [`EntryResolution`] equality (bitwise on the price, so an identical
/// computation compares equal without tripping `clippy::float_cmp`).
#[cfg(test)]
fn same_entry(a: &EntryResolution, b: &EntryResolution) -> bool {
    match (a, b) {
        (EntryResolution::NoEntry, EntryResolution::NoEntry) => true,
        (
            EntryResolution::Entered { fill_row: r1, price: p1, at: t1 },
            EntryResolution::Entered { fill_row: r2, price: p2, at: t2 },
        ) => r1 == r2 && p1.to_bits() == p2.to_bits() && t1 == t2,
        _ => false,
    }
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
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    if !b.fast_exit || !index.is_ready() {
        return resolve_exit(trades, series, b, entry, pricing, tail_horizon);
    }

    // Earliest firing row across the classified reqs. On a tie the req EARLIER in
    // `exit_reqs` wins — the same order the scalar walk breaks ties by, which is what
    // keeps the desugared SL ahead of TP ahead of the authored metrics. (A req whose
    // first firing row is `min` is exactly a req that fires at `min`: an earlier first
    // row would contradict minimality.)
    // (row, req_index) of the earliest classified req that fires — the index is
    // kept (not just the derived `ExitCode`) so the caller can label an
    // `ExitCode::Metrics` winner via `BoundCombo::exit_metric_label` below.
    let mut best: Option<(usize, usize)> = None;
    for (i, req) in b.rule.exit_reqs.iter().enumerate() {
        let row = match b.exit_classes[i] {
            ExitClass::PnlBound { up } => match first_pnl_row(index, req, entry_price, up) {
                Ok(row) => row,
                // Non-monotone `pnl` over this token's price range (overflow) — the
                // hull query would be unsound, so hand the whole rule to scalar.
                Err(()) => return resolve_exit(trades, series, b, entry, pricing, tail_horizon),
            },
            ExitClass::HeldBound => match first_held_row(series, index, fill_row, entry_at, req) {
                Ok(row) => row,
                // Block time regressed inside the scan range, so `held` is not
                // monotone and a binary search could land anywhere.
                Err(()) => return resolve_exit(trades, series, b, entry, pricing, tail_horizon),
            },
            ExitClass::Trailing => first_trailing_row(series, fill_row, entry_price, req),
            ExitClass::Bounce => first_bounce_row(series, fill_row, entry_price, req),
            ExitClass::General => unreachable!("fast_exit implies no General req"),
        };
        if let Some(row) = row {
            if best.is_none_or(|(br, _)| row < br) {
                best = Some((row, i));
            }
        }
    }
    // Dead outranks every strategy exit, at any row (scalar checks it first).
    // `None` req-index marks Dead (never req-driven, so never a Metrics label).
    let winner: Option<(usize, Option<usize>)> = match (index.dead_row(), best) {
        (Some(dead), Some((br, _))) if dead <= br => Some((dead, None)),
        (Some(dead), None) => Some((dead, None)),
        (_, Some((br, bi))) => Some((br, Some(bi))),
        (None, None) => None,
    };

    match winner {
        Some((row, req_idx)) => {
            let exit = match req_idx {
                Some(i) => exit_code_of(b.rule.exit_reqs[i].origin),
                None => ExitCode::Dead,
            };
            close_at_fire(
                trades,
                series,
                b,
                exit,
                req_idx,
                entry_price,
                entry_at,
                fill_trade_slot(series, fill_row),
                entry_depth(series, fill_row),
                row,
                pricing,
            )
        }
        // Open by the series' per-token cut: first try the analytic frozen-tail
        // resolve (D1), else mark to last finite (precomputed on the index).
        None => resolve_frozen_tail(
            trades, series, b, fill_row, entry_price, entry_at, pricing, tail_horizon,
        )
        .unwrap_or_else(|| {
            let last_price = index
                .last_finite_row()
                .map(|k| series.price[k])
                .filter(|p| p.is_finite())
                .unwrap_or(entry_price);
            open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
        }),
    }
}

/// Walk from the entry fill to the exit decision, mirroring the open-side
/// `decide_arm`: `Dead` first, then the first `exit_reqs` entry that holds (compile
/// prepends the desugared SL/TP, so the ladder's `StopLoss > TakeProfit > Metrics`
/// order survives). No exit by the tail ⇒ `Open`, marked to the last known price.
/// Exit *price* is the run's [`FillModel`] fill after the firing row (analysis:
/// market-fill fallback on an empty window).
///
/// **This walk is the SSOT.** TP/SL is **not** re-derived here as an
/// `entry_price · (1 ∓ pct/100)` price branch — that is a second representation of a
/// fact the engine already desugars into a `pnl` req, and it compares in price space
/// where the fold compares in pnl space. The sweep evaluates the very reqs
/// `CompiledRule::exit_fired` does.
pub(crate) fn resolve_exit(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    if !b.rule.scale_out.is_empty() {
        return resolve_exit_staged(trades, series, b, entry, pricing, tail_horizon);
    }
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
    // Running since-entry peak/trough for the position-scoped exit metrics
    // (`retrace`/`bounce`/`pnl`). Seeded to the fill price — exactly as `reduce.rs`
    // seeds `ArmState::Entered` — and folded forward each event BEFORE that event's
    // exit decision, mirroring `evaluate_token`'s per-event extrema fold.
    let mut ctx = PositionCtx::at_fill(entry_price, entry_at);
    for j in (fill_row + 1)..n {
        if series.dead[j] {
            return close_at_fire(
                trades,
                series,
                b,
                ExitCode::Dead,
                None,
                entry_price,
                entry_at,
                entry_slot,
                entry_depth(series, fill_row),
                j,
                pricing,
            );
        }
        ctx.fold_price(series.price[j]);
        if has_exit_reqs {
            if let Some((exit, idx)) = first_exit_req_fired(series, &c.exit_reqs, exit_cols, j, &ctx) {
                return close_at_fire(
                    trades,
                    series,
                    b,
                    exit,
                    Some(idx),
                    entry_price,
                    entry_at,
                    entry_slot,
                    entry_depth(series, fill_row),
                    j,
                    pricing,
                );
            }
        }
    }
    // Open by the series' per-token cut. Before reporting `Open`, resolve the frozen
    // quiet tail analytically (D1): a deterministic clock can still fire past this
    // token's own last-trade cut, up to the corpus-wide tail simulate ticks to.
    if let Some(closed) =
        resolve_frozen_tail(trades, series, b, fill_row, entry_price, entry_at, pricing, tail_horizon)
    {
        return closed;
    }
    // Genuinely open: mark to the last finite price (unrealized — excluded from the
    // realized stats by `RunAgg`, but priced for the drill-in / row view).
    let last_price = (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
    open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
}

/// Scale-out exit scan — mirrors `decide_arm`'s open-side priority
/// `Dead > exit_fired > stage_fired` across the ladder. Each partial books an
/// [`ExitLeg`] at the fire-row fill and resumes from the next row with the same
/// `PositionCtx` (peak/trough NOT reseeded). The final/global close sells the
/// remainder (`10_000 - sold_bps`). PnL through [`round_trip_multi_leg`].
///
/// Deliberate divergence vs `reduce` + replay: the sweep has no in-flight-sell
/// blindness — a stage fill is instantaneous, so a global exit that becomes true
/// on the next row after a partial is taken immediately. Replay stays
/// `ExitPending` until the paper fill confirms (possibly deferred to a later
/// trade). Documented in `docs/plans/sweep/sim-parity.md`.
///
/// Frozen-tail (D1) is **not** applied while a ladder is in play: a stage clock
/// that would only fire past the per-token cut stays `Open` here (re-measure
/// through simulate). Legacy no-scale-out rules keep the full D1 path.
fn resolve_exit_staged(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    entry: &EntryResolution,
    pricing: &Pricing,
    _tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let c = &b.rule;
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    let entry_slot = fill_trade_slot(series, fill_row);
    let entry_reserve = entry_depth(series, fill_row);
    let n = series.n_rows();
    let has_exit_reqs = c.has_exit_metrics();
    let mut ctx = PositionCtx::at_fill(entry_price, entry_at);
    let mut stage: usize = 0;
    let mut sold_bps: u16 = 0;
    let mut legs: Vec<ExitLeg> = Vec::new();

    for j in (fill_row + 1)..n {
        if series.dead[j] {
            return close_staged(
                trades,
                series,
                b,
                ExitCode::Dead,
                None,
                entry_price,
                entry_at,
                entry_slot,
                entry_reserve,
                &legs,
                sold_bps,
                j,
                pricing,
            );
        }
        ctx.fold_price(series.price[j]);

        // Global side first — catastrophe path (SL / authored exit / desugared TP).
        if has_exit_reqs {
            if let Some((exit, idx)) =
                first_exit_req_fired(series, &c.exit_reqs, &b.exit_cols, j, &ctx)
            {
                return close_staged(
                    trades,
                    series,
                    b,
                    exit,
                    Some(idx),
                    entry_price,
                    entry_at,
                    entry_slot,
                    entry_reserve,
                    &legs,
                    sold_bps,
                    j,
                    pricing,
                );
            }
        }

        // Current scale-out stage (if any remain).
        let Some(compiled_stage) = c.scale_out.get(stage) else {
            continue;
        };
        let stage_cols = b.stage_cols.get(stage).map(Vec::as_slice).unwrap_or(&[]);
        let Some((exit, _)) =
            first_exit_req_fired(series, &compiled_stage.reqs, stage_cols, j, &ctx)
        else {
            continue;
        };

        match compiled_stage.sell_bps {
            Some(bps) => {
                // Partial: book the leg, advance stage, keep scanning (position stays open).
                let rem = 10_000u16.saturating_sub(sold_bps);
                let sell = bps.min(rem);
                if sell == 0 {
                    stage = stage.saturating_add(1);
                    continue;
                }
                let (price, reserve) = stage_fill_price(trades, series, j, pricing, entry_reserve);
                legs.push(ExitLeg { sell_bps: sell, price, reserve_sol: reserve });
                sold_bps = sold_bps.saturating_add(sell);
                stage = stage.saturating_add(1);
            }
            None => {
                // Remainder stage (`sell_bps` omitted) ⇒ full close under its conditions.
                // Stage-authored metric labels aren't in `exit_metric_label` (global
                // slots only) — stamp `None` and keep the ExitCode from the stage req.
                return close_staged(
                    trades,
                    series,
                    b,
                    exit,
                    None,
                    entry_price,
                    entry_at,
                    entry_slot,
                    entry_reserve,
                    &legs,
                    sold_bps,
                    j,
                    pricing,
                );
            }
        }
    }

    // Still open: mark the unsold remainder. No frozen-tail advance of stages (see
    // fn docs). When nothing was banked this reduces to the legacy open mark.
    let last_price =
        (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
    open_staged(
        series,
        fill_row,
        entry_price,
        entry_at,
        entry_slot,
        entry_reserve,
        &legs,
        sold_bps,
        last_price,
        pricing,
    )
}

/// Paper fill price (+ reserve) at a stage/global fire row — same `exit_fill`
/// helper the single-leg path uses, so SignalPrice / WorstCase / FirstInWindow
/// stay coherent with replay when the fill window collapses to the fire trade.
fn stage_fill_price(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    fire_row: usize,
    pricing: &Pricing,
    entry_reserve: Option<f64>,
) -> (f64, Option<f64>) {
    let fill = exit_fill(trades, series, fire_row, pricing.fill_model).unwrap_or_else(|| PaperFill {
        trade_idx: 0,
        price: series.price[fire_row],
        token_amount: 0.0,
        slot: series.slot[fire_row].unwrap_or(0),
        block_time: series.at[fire_row],
        tx_signature: String::new(),
    });
    let exit_row = series_row_for_trade_idx(series, fill.trade_idx).unwrap_or(fire_row);
    let reserve = series
        .priced_reserve_sol
        .get(exit_row)
        .copied()
        .filter(|r| r.is_finite() && *r > 0.0)
        .or(entry_reserve);
    (fill.price, reserve)
}

/// Final close of a staged position: append the remaining bag as one leg and
/// price through [`round_trip_multi_leg`]. `exit_req_idx` labels global authored
/// metrics only (`None` for Dead / remainder-stage closes).
#[allow(clippy::too_many_arguments)]
fn close_staged(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    exit: ExitCode,
    exit_req_idx: Option<usize>,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    entry_reserve_sol: Option<f64>,
    prior_legs: &[ExitLeg],
    sold_bps: u16,
    fire_row: usize,
    pricing: &Pricing,
) -> TokenOutcome {
    let fill = exit_fill(trades, series, fire_row, pricing.fill_model).unwrap_or_else(|| PaperFill {
        trade_idx: 0,
        price: series.price[fire_row],
        token_amount: 0.0,
        slot: series.slot[fire_row].unwrap_or(0),
        block_time: series.at[fire_row],
        tx_signature: String::new(),
    });
    let exit_row = series_row_for_trade_idx(series, fill.trade_idx).unwrap_or(fire_row);
    let reserve = series
        .priced_reserve_sol
        .get(exit_row)
        .copied()
        .filter(|r| r.is_finite() && *r > 0.0)
        .or(entry_reserve_sol);
    let mut legs: Vec<ExitLeg> = prior_legs.to_vec();
    let rem = 10_000u16.saturating_sub(sold_bps);
    if rem > 0 {
        legs.push(ExitLeg { sell_bps: rem, price: fill.price, reserve_sol: reserve });
    }
    let label = exit_req_idx.and_then(|i| b.exit_metric_label.get(i).copied().flatten());
    closed_multi(
        exit,
        label,
        entry_price,
        entry_at,
        entry_slot,
        entry_reserve_sol,
        &legs,
        fill.price,
        fill.block_time,
        fill_trade_slot(series, exit_row),
        pricing,
    )
}

/// Open (or mid-ladder open) mark: banked legs + a mark-to-`last_price` remainder.
#[allow(clippy::too_many_arguments)]
fn open_staged(
    series: &MetricSeries,
    fill_row: usize,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    entry_reserve_sol: Option<f64>,
    prior_legs: &[ExitLeg],
    sold_bps: u16,
    last_price: f64,
    pricing: &Pricing,
) -> TokenOutcome {
    let mut legs: Vec<ExitLeg> = prior_legs.to_vec();
    let rem = 10_000u16.saturating_sub(sold_bps);
    if rem > 0 {
        legs.push(ExitLeg {
            sell_bps: rem,
            price: last_price,
            reserve_sol: entry_reserve_sol,
        });
    }
    if legs.is_empty() {
        return open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing);
    }
    let (pnl_sol, pnl_pct) = round_trip_multi_leg(
        entry_price,
        pricing.buy_amount_sol,
        entry_reserve_sol,
        &legs,
        &pricing.cost,
    );
    TokenOutcome {
        fired: true,
        holding_secs: 0,
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::Open,
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_window: None,
        exit_metric_slot: None,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: entry_slot.or_else(|| fill_trade_slot(series, fill_row)),
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

/// Multi-leg sibling of [`closed`] — same outcome shape, PnL via
/// [`round_trip_multi_leg`]. `exit_price`/`exit_time` stamp the **final** leg
/// (matches `End`'s last-leg reason/price).
#[allow(clippy::too_many_arguments)]
fn closed_multi(
    exit: ExitCode,
    label: Option<(MetricId, Operator, f64, Option<hunter_engine::metrics::WindowSpec>, u8)>,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    entry_reserve_sol: Option<f64>,
    legs: &[ExitLeg],
    exit_price: f64,
    exit_at: DateTime<Utc>,
    exit_slot: Option<u64>,
    pricing: &Pricing,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = if legs.is_empty() {
        round_trip_with_costs(
            entry_price,
            exit_price,
            pricing.buy_amount_sol,
            entry_reserve_sol,
            &pricing.cost,
        )
    } else {
        round_trip_multi_leg(
            entry_price,
            pricing.buy_amount_sol,
            entry_reserve_sol,
            legs,
            &pricing.cost,
        )
    };
    TokenOutcome {
        fired: true,
        holding_secs: (exit_at - entry_at).num_seconds(),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit,
        exit_metric: label.map(|(m, _, _, _, _)| m),
        exit_operator: label.map(|(_, op, _, _, _)| op),
        exit_metric_value: label.map(|(_, _, v, _, _)| v),
        exit_metric_window: label.and_then(|(_, _, _, w, _)| w),
        exit_metric_slot: label.map(|(_, _, _, _, s)| s),
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot,
        exit_time: Some(exit_at),
        exit_price: Some(exit_price),
        exit_slot,
    }
}

/// SOL-side pool depth at the entry row, for [`CostModel::price_impact`]. `None`
/// when the series has no depth yet (pre-first-trade rows are `NaN`), which the
/// cost model treats as "depth unknown" and charges no impact — never a guess.
///
/// Reads the **priced** depth (`vsol`), not `reserve_sol`: impact on a
/// constant-product curve is `B / vsol`, and the real reserve is `vsol - 30` clamped
/// at zero. See [`TradeLite::priced_reserve_sol`].
fn entry_depth(series: &MetricSeries, fill_row: usize) -> Option<f64> {
    series.priced_reserve_sol.get(fill_row).copied().filter(|r| r.is_finite() && *r > 0.0)
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
    let (pnl_sol, pnl_pct) = round_trip_with_costs(
        entry_price,
        last_price,
        pricing.buy_amount_sol,
        entry_depth(series, fill_row),
        &pricing.cost,
    );
    TokenOutcome {
        fired: true,
        holding_secs: 0,
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::Open,
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_window: None,
        exit_metric_slot: None,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: fill_trade_slot(series, fill_row),
        exit_time: None,
        exit_price: None,
        exit_slot: None,
    }
}

// ───────────────────────── frozen quiet-tail resolve (D1) ──────────────────────
//
// After a token's last trade the price is **frozen** (no later print moves it), so
// the sweep caps each token's series at `own_last_trade + DEAD_QUIET + TAIL_MARGIN`.
// `run_replay` (simulate + live) instead ticks every held token to the **corpus-wide**
// tail `min(as_of, corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`. In the gap between
// the two, a position a multi-token simulate closes reads a false `Open` in the sweep
// — divergence D1 in `docs/plans/sweep/sim-parity.md`.
//
// At a frozen price only the rate-1 clocks keep moving — `time` (since creation),
// `stall` (since the last high; a flat price sets no new high), and `held` (since
// entry). Everything else is constant: `pnl`/`retrace`/`bounce`/`trail`/`liquidity`
// can't newly cross a level they haven't reached, and `Dead` already resolves inside
// the per-token series (its cap covers `last_meaningful + DEAD_QUIET`, and a still-Open
// position has healthy reserves, so it never dies in the extra tail). So the extra tail
// is resolved **analytically in O(1)** — find the earliest grid tick a clock crosses,
// book it at the last trade's market fill — instead of extending the tick grid (which
// is exactly the RAM the sparse-grid design exists to bound).

/// The frozen-tail resolve horizon for a token set scanned at `as_of` — the
/// corpus-wide tail cap `run_replay` bounds its tick loop by, `min(as_of,
/// corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`. `None` for a trade-less set.
pub(crate) fn frozen_tail_horizon(as_of: Ts, tokens: &[CorpusToken]) -> Option<Ts> {
    // The corpus's newest trade — `run_replay`'s `last_trade_at`. Single-sourced with
    // the freshness stamp the run row stores (`Corpus::last_trade_at`), so "data
    // through HH:MM" in the UI names the very instant this horizon is built on.
    let last = crate::sweep::corpus::Corpus::last_trade_at_of(tokens)?;
    Some(as_of.min(last + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS)))
}

/// The clock tick as fractional seconds — the frozen-tail extrapolation step.
const TICK_SECS: f64 = TICK_MS as f64 / 1000.0;

/// D1 resolve: when the in-series scan leaves a position `Open`, book the earliest
/// deterministic-clock exit that fires in the frozen tail `(last_row, tail_horizon]`,
/// or `None` (⇒ the caller reports `Open`) when none does.
///
/// `None` immediately when: the resolve is off (`tail_horizon: None`, e.g. single-token
/// `scan`), the horizon adds no full tick past the series' last row, or **any** exit
/// req is windowed — a rolling flow/price window keeps changing in the tail (flows
/// decay, extrema age out), which this clock-only resolve does not model, so such a
/// rule conservatively keeps the legacy bounded-tail `Open` rather than risk a
/// mis-timed close (documented residual — see the section comment).
///
/// The exit is priced through the shared [`close_at_fire`] at the series' last row, so
/// its fill (the last trade's market fill on an empty window), time, slot and PnL are
/// **byte-identical** to the exit `run_replay`'s `queue_exit_fill` books for the same
/// crossing — this only decides *that* a clock fires and *which* one, never the money.
#[allow(clippy::too_many_arguments)]
fn resolve_frozen_tail(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    fill_row: usize,
    entry_price: f64,
    entry_at: Ts,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> Option<TokenOutcome> {
    let horizon = tail_horizon?;
    // A windowed exit metric is not a rate-1 clock — leave it to the legacy Open.
    if b.rule.exit_reqs.iter().any(|r| r.window.is_windowed()) {
        return None;
    }
    let n = series.n_rows();
    if n == 0 {
        return None;
    }
    let last = n - 1;
    let last_at = series.at[last];
    // `run_replay` ticks while `next_tick < tail_end`, so the last tick is the largest
    // `m` with `last_at + m·TICK < horizon` — match its strict bound exactly.
    let horizon_ms = horizon.signed_duration_since(last_at).num_milliseconds();
    if horizon_ms < TICK_MS {
        return None;
    }
    let m_max = (horizon_ms - 1) / TICK_MS;
    if m_max < 1 {
        return None;
    }
    // The `held` clock reads the per-entry context; `time`/`stall` read their series
    // column at the last row (the scan already proved every req false there).
    let pos_ctx = PositionCtx::at_fill(entry_price, entry_at);
    let mut best_m: Option<i64> = None;
    for (i, req) in b.rule.exit_reqs.iter().enumerate() {
        let Some(base) = frozen_tail_clock_base(series, b, &pos_ctx, i, req, last) else {
            continue;
        };
        if let Some(m) = first_monotone_fire(&req.conds, req.tolerance, base, TICK_SECS, m_max) {
            best_m = Some(best_m.map_or(m, |b| b.min(m)));
        }
    }
    let delta = best_m? as f64 * TICK_SECS;
    // First req (in `exit_reqs` order) that holds at the firing instant labels the exit
    // — a non-clock req reads the frozen values that left the position Open, so only a
    // clock can hold here; this mirrors `first_exit_req_fired` at one row.
    let mut exit: Option<(ExitCode, usize)> = None;
    for (i, req) in b.rule.exit_reqs.iter().enumerate() {
        let Some(base) = frozen_tail_clock_base(series, b, &pos_ctx, i, req, last) else {
            continue;
        };
        if eval(&req.conds, base + delta, req.tolerance) {
            exit = Some((exit_code_of(req.origin), i));
            break;
        }
    }
    let (exit, req_idx) = exit?;
    let entry_slot = fill_trade_slot(series, fill_row);
    Some(close_at_fire(
        trades,
        series,
        b,
        exit,
        Some(req_idx),
        entry_price,
        entry_at,
        entry_slot,
        entry_depth(series, fill_row),
        last,
        pricing,
    ))
}

/// The base reading of a **frozen-tail rate-1 clock** exit req at the series' last row,
/// or `None` when this req is not such a clock. Only `time`/`stall` (token-scoped,
/// non-windowed) and `held` (position-scoped) advance at exactly 1 s/s while the price
/// is frozen; every other exit metric is constant in the tail and so never newly fires.
fn frozen_tail_clock_base(
    series: &MetricSeries,
    b: &BoundCombo,
    pos_ctx: &PositionCtx,
    i: usize,
    req: &MetricReq,
    row: usize,
) -> Option<f64> {
    if req.window.is_windowed() {
        return None;
    }
    match req.metric {
        MetricId::Time | MetricId::Stall if !req.position_scoped => {
            Some(value_at_col(series, b.exit_cols[i], row))
        }
        MetricId::Held if req.position_scoped => {
            Some(position_value(MetricId::Held, pos_ctx, series.price[row], series.at[row]))
        }
        _ => None,
    }
}

/// Earliest grid step `m` (`1 ≤ m ≤ m_max`) at which a monotone-increasing reading
/// `base + m·dt` first satisfies `eval(conds, ·, tol)`, or `None` if it never does in
/// range. The reading is already false at `base` (the scan left the position Open), and
/// a piecewise-constant predicate can only flip false→true at a condition breakpoint,
/// so it suffices to test the grid steps bracketing each breakpoint (`value`, `value ±
/// tol/2`) with the **real** `eval` — exact tolerance / operator / DNF semantics, never
/// re-derived, just sampled where a crossing can occur. O(#conds).
fn first_monotone_fire(
    conds: &[Vec<Condition>],
    tol: f64,
    base: f64,
    dt: f64,
    m_max: i64,
) -> Option<i64> {
    if !base.is_finite() || dt <= 0.0 || dt.is_nan() || m_max < 1 {
        return None;
    }
    let half = tol / 2.0;
    let mut best: Option<i64> = None;
    for arm in conds {
        for c in arm {
            for theta in [c.value, c.value - half, c.value + half] {
                if !theta.is_finite() {
                    continue;
                }
                let m0 = ((theta - base) / dt).floor() as i64;
                for m in (m0 - 1)..=(m0 + 2) {
                    if m < 1 || m > m_max || best.is_some_and(|b| m >= b) {
                        continue;
                    }
                    if eval(conds, base + m as f64 * dt, tol) {
                        best = Some(m);
                    }
                }
            }
        }
    }
    best
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
    let ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
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
    let ctx = PositionCtx::at_fill(1.0, entry_at);
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
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for j in start..n {
        let p = series.price[j];
        ctx.fold_price(p);
        if eval(&req.conds, ctx.retrace(p), req.tolerance) {
            return Some(j);
        }
    }
    None
}

/// First row satisfying a `m_position.bounce` condition — O(n) with the running
/// since-entry trough (mirror of [`first_trailing_row`]).
fn first_bounce_row(
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
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for j in start..n {
        let p = series.price[j];
        ctx.fold_price(p);
        if eval(&req.conds, ctx.bounce(p), req.tolerance) {
            return Some(j);
        }
    }
    None
}

/// Whether `entry_price` is a usable reference for the position metrics.
/// [`PositionCtx::pnl`] / [`PositionCtx::retrace`] / [`PositionCtx::bounce`] yield
/// `NaN` for anything else, so no row can fire and the fast paths have nothing to
/// search for.
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
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    debug_assert_cols_match(series, b);
    let (fill_row, entry_price, entry_at) = match entry {
        EntryResolution::NoEntry => return TokenOutcome::no_entry(),
        EntryResolution::Entered { fill_row, price, at } => (*fill_row, *price, *at),
    };
    let Some(bounds) = pnl_bounds_for_vector_scan(b, entry_price) else {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index, tail_horizon);
    };
    if !simd_available() {
        return resolve_exit_indexed(trades, series, b, entry, pricing, index, tail_horizon);
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
        let (exit, req_idx) = if series.dead[j] {
            (ExitCode::Dead, None)
        } else {
            let ctx = PositionCtx::at_fill(entry_price, entry_at);
            match first_exit_req_fired(series, &b.rule.exit_reqs, &b.exit_cols, j, &ctx) {
                Some((exit, i)) => (exit, Some(i)),
                // Should not happen (the vector scan only lands on `j` because some
                // req fires there) — preserved as a safety fallback; the label is
                // simply unavailable for this one outcome.
                None => (ExitCode::Metrics, None),
            }
        };
        return close_at_fire(
            trades,
            series,
            b,
            exit,
            req_idx,
            entry_price,
            entry_at,
            entry_slot,
            entry_depth(series, fill_row),
            j,
            pricing,
        );
    }

    // Open: identical tail to the scalar path — analytic frozen-tail resolve (D1)
    // first, else mark to the last finite price.
    resolve_frozen_tail(trades, series, b, fill_row, entry_price, entry_at, pricing, tail_horizon)
        .unwrap_or_else(|| {
            let last_price =
                (0..n).rev().map(|k| series.price[k]).find(|p| p.is_finite()).unwrap_or(entry_price);
            open_outcome(series, fill_row, entry_price, entry_at, last_price, pricing)
        })
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
    let ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
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
    let mut ctx = PositionCtx::at_fill(entry_price, DateTime::UNIX_EPOCH);
    for (j, &p) in price.iter().enumerate().take(n).skip(start) {
        ctx.fold_price(p);
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
    // Trough is unused for `retrace` — seed it to the fill so the ctx is well-formed.
    let mut ctx = PositionCtx {
        entry_price,
        peak_price: carry,
        trough_price: entry_price,
        entered_at: DateTime::UNIX_EPOCH,
    };
    for (k, &p) in price.iter().enumerate().take(n).skip(j) {
        ctx.fold_price(p);
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
/// `exit_req_idx` is the winning `b.rule.exit_reqs` index (`None` for `Dead` or
/// any exit that isn't req-driven) — looked up once here against
/// [`BoundCombo::exit_metric_label`] to stamp the detailed metric/operator/value/
/// slot onto the outcome, instead of collapsing every authored condition to the
/// bare `ExitCode::Metrics` code.
#[allow(clippy::too_many_arguments)]
fn close_at_fire(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    exit: ExitCode,
    exit_req_idx: Option<usize>,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    entry_reserve_sol: Option<f64>,
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
    let label = exit_req_idx.and_then(|i| b.exit_metric_label.get(i).copied().flatten());
    closed(
        exit,
        label,
        entry_price,
        entry_at,
        entry_slot,
        entry_reserve_sol,
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
// The no-horizon convenience is exercised only by the single-token `guard` locks now
// that the drill-in threads a corpus horizon through `scan_with_horizon`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn scan(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    c: &CompiledRule,
    pricing: &Pricing,
) -> TokenOutcome {
    // No corpus horizon ⇒ the frozen-tail resolve is off, i.e. the exact single-token
    // behavior the `guard` locks against a single-token `run_replay` (whose tail is this
    // same per-token cut). A multi-token caller uses [`scan_with_horizon`].
    scan_with_horizon(trades, series, c, pricing, None)
}

/// [`scan`] with an explicit corpus frozen-tail horizon (D1) — the per-token driver a
/// multi-token caller (the grouped-sweep drill-in) uses so a deterministic clock exit
/// that lands past a token's own last-trade cut still closes, matching a simulate over
/// the same token set. Pass [`frozen_tail_horizon`] of the scanned set; `None` reduces
/// to [`scan`].
pub(crate) fn scan_with_horizon(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    c: &CompiledRule,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> TokenOutcome {
    let bound = BoundCombo::new(series.columns(), c.clone());
    let entry = resolve_entry(trades, series, &bound, pricing);
    resolve_exit(trades, series, &bound, &entry, pricing, tail_horizon)
}

#[allow(clippy::too_many_arguments)]
fn closed(
    exit: ExitCode,
    label: Option<(MetricId, Operator, f64, Option<hunter_engine::metrics::WindowSpec>, u8)>,
    entry_price: f64,
    entry_at: DateTime<Utc>,
    entry_slot: Option<u64>,
    entry_reserve_sol: Option<f64>,
    exit_price: f64,
    exit_at: DateTime<Utc>,
    exit_slot: Option<u64>,
    pricing: &Pricing,
) -> TokenOutcome {
    let (pnl_sol, pnl_pct) = round_trip_with_costs(
        entry_price,
        exit_price,
        pricing.buy_amount_sol,
        entry_reserve_sol,
        &pricing.cost,
    );
    TokenOutcome {
        fired: true,
        holding_secs: (exit_at - entry_at).num_seconds(),
        pnl_percent: pnl_pct as f32,
        pnl_sol: pnl_sol as f32,
        exit,
        exit_metric: label.map(|(m, _, _, _, _)| m),
        exit_operator: label.map(|(_, op, _, _, _)| op),
        exit_metric_value: label.map(|(_, _, v, _, _)| v),
        exit_metric_window: label.and_then(|(_, _, _, w, _)| w),
        exit_metric_slot: label.map(|(_, _, _, _, s)| s),
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
            entry_enabled: true,
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
    fn scale_out_forces_scalar_staged_walk() {
        // Scale-out is multi-leg; the index / SIMD paths only price a single full-bag
        // close. Even a TP/SL-shaped global side must not take `fast_exit`.
        let c = compiled(serde_json::json!({
            "scale_out": [{ "sell_bps": 7000, "take_profit": 50 }]
        }));
        let b = BoundCombo::new(&[], c);
        assert!(!b.fast_exit);
        assert_eq!(b.stage_cols.len(), 1);
    }

    // ── Pass-2 grid: dynamic per-combo winner selection ────────────────────────

    /// Build one combo's [`ComboMetrics`] the same way `post_group_rescore` does
    /// (`ComboAgg::record` + `finalize` + `rescore_for_group`), from a flat list of
    /// realized per-trade PnL%.
    fn metrics_from_pnls(id: u32, pnls: &[f32], group_tokens: usize) -> crate::sweep::aggregate::ComboMetrics {
        use crate::sweep::aggregate::ComboAgg;
        use trading_core::strategies::kernel::ExitCode;
        let mut agg = ComboAgg::default();
        for &p in pnls {
            agg.record(&TokenOutcome {
                fired: true,
                holding_secs: 60,
                pnl_percent: p,
                pnl_sol: (p / 100.0) as f32,
                exit: ExitCode::TakeProfit,
                ..TokenOutcome::no_entry()
            });
        }
        let mut m = agg.finalize(id);
        m.rescore_for_group(group_tokens);
        m
    }

    #[test]
    fn pass2_keeps_the_combos_own_baseline_when_no_candidate_beats_it() {
        // The grid is dynamic per combo: a combo whose OWN exit already wins must
        // not be overwritten just because a candidate ladder was evaluated.
        let baseline = metrics_from_pnls(7, &[80.0, 90.0, 70.0], 10);
        let worse_ladder = metrics_from_pnls(7, &[-40.0, -60.0, -50.0], 10);
        assert!(
            !pass2_candidate_wins(&worse_ladder, &baseline),
            "a ladder that performs worse than the combo's own exit must not win"
        );
    }

    #[test]
    fn pass2_adopts_a_candidate_that_beats_the_baseline() {
        let baseline = metrics_from_pnls(7, &[-30.0, -20.0, -25.0], 10);
        let better_ladder = metrics_from_pnls(7, &[60.0, 70.0, 65.0], 10);
        assert!(
            pass2_candidate_wins(&better_ladder, &baseline),
            "a ladder that clearly outperforms the combo's own exit must win"
        );
    }

    #[test]
    fn pass2_picks_the_best_of_several_candidates_not_just_the_first_better_one() {
        // Simulates the fold in `post_group_rescore`: start from baseline, keep
        // whichever of a sequence of candidates is currently best — must end up on
        // the GLOBAL best, not the first one that beat the running `best`.
        let baseline = metrics_from_pnls(7, &[-10.0], 10);
        let candidates = [
            (0usize, metrics_from_pnls(7, &[10.0], 10)), // beats baseline
            (1usize, metrics_from_pnls(7, &[95.0], 10)), // the actual best
            (2usize, metrics_from_pnls(7, &[20.0], 10)), // beats baseline, not #1
        ];
        let mut best = baseline;
        let mut best_variant: Option<usize> = None;
        for (vi, m) in candidates {
            if pass2_candidate_wins(&m, &best) {
                best = m;
                best_variant = Some(vi);
            }
        }
        assert_eq!(best_variant, Some(1), "must adopt the single best candidate, not the first improvement");
    }

    #[test]
    fn position_metrics_classify_by_kind() {
        let retrace = compiled(serde_json::json!({
            "exit": { "m_position": { "retrace": [{ "operator": ">=", "value": 3 }] } }
        }));
        let b = BoundCombo::new(&[], retrace);
        assert_eq!(b.exit_classes, vec![ExitClass::Trailing]);
        assert!(b.fast_exit);

        let bounce = compiled(serde_json::json!({
            "exit": { "m_position": { "bounce": [{ "operator": ">=", "value": 15 }] } }
        }));
        let b = BoundCombo::new(&[], bounce);
        assert_eq!(b.exit_classes, vec![ExitClass::Bounce]);
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
