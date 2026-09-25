//! `GenericSweepStrategy` — the grouped sweep's [`Strategy`]: a precompute-then-scan
//! sweep over the axes grid.
//!
//! * **precompute** ([`prepare_token`](Strategy::prepare_token)) — one fold over a
//!   token builds a [`MetricSeries`] with every column the axes read, over the same
//!   event stream `run_replay` folds (trades plus the sparse tick grid, same tail, same
//!   `TradeLite` mapping), so the scan reads the values the engine would see.
//! * **scan** ([`super::scan`], [`super::fast_exit`], [`super::frozen_tail`]) — per
//!   combo, the engine's own entry and held-side decisions over the series rows. Caps
//!   do not apply: the sweep judges each token alone.
//!
//! The guard (`super::guard`) locks the scan to a full `run_replay`.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use uuid::Uuid;

use hunter_engine::arm::CompiledRule;
use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::FingerprintId;
use hunter_engine::metrics::buffers::Buffers;
use hunter_engine::metrics::grid::{estimate_sparse_rows as grid_estimate_rows, fold_sparse};
/// The sparse tick grid lives in `hunter-engine` so the sweep precompute and the
/// metric-series chart drive a `MetricSeries` through the ONE loop. Re-exported here
/// because the axes model and the discovery screen build one.
pub use hunter_engine::metrics::grid::SparseGrid;
use hunter_engine::metrics::series::{MetricSeries, SeriesColumn, SERIES_ANCHOR_CAP};
use hunter_engine::metrics::tags::config::CompiledTag;
use hunter_engine::metrics::{Metric, TradeLite, Ts};
use hunter_engine::rule_params::{RuleParams, Stage};

use trading_core::config::constants::sol_to_lamports;
use trading_core::strategies::kernel::CostModel;
use trading_core::strategies::paper_fill::FillModel;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod, TokenOutcome};

use super::axes::AxesModel;
use super::fast_exit;
use super::frozen_tail::frozen_tail_horizon;
use super::scan::{self, BoundCombo, EntryCandidates, EntryResolution};

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
/// a [`FillModel`] already prices execution slippage, so a flat per-leg slippage
/// charge on top would double-count it — which is why no such cost model exists.
/// [`CostModel::pumpfun_with_impact`] is the honest partner to any fill model: impact
/// is our own footprint on the curve, orthogonal to which print we transact against,
/// and a live trade pays both. Carried as one struct so a scan fn can never be handed
/// the fill model without the cost model.
#[derive(Clone, Copy, Debug)]
pub struct Pricing {
    /// Notional (SOL) every round-trip is sized at.
    pub buy_amount_sol: f64,
    /// Which trade in the fill window prices the entry / exit leg.
    pub fill_model: FillModel,
    /// Execution frictions charged on the round-trip.
    pub cost: CostModel,
}

impl Pricing {
    /// What one position takes from the wallet at this notional —
    /// [`CostModel::capital_sol`]. Every percent's denominator, so a cohort's capital
    /// is `n × capital_sol()`, never `n × buy_amount_sol`.
    pub fn capital_sol(&self) -> f64 {
        self.cost.capital_sol(self.buy_amount_sol)
    }

    /// This pricing for a position that entered at series row `fill_row`: the buy
    /// pays the entry pool's own fee when it filled on a PumpSwap swap that recorded
    /// one. Each exit leg carries its own pool's fee (`ExitLeg::venue_fee_bps`).
    pub(crate) fn at_entry(&self, trades: &[CorpusTrade], series: &MetricSeries, fill_row: usize) -> Self {
        Self { cost: self.cost.at_venue_fee(scan::venue_fee_at_row(trades, series, fill_row)), ..*self }
    }
}

// Deliberately NO `Default` impl. `CostModelKind` carries one (for an omitted
// request field) and `FillModel` carries one, but a *pair* assembled from two
// independent defaults is exactly the silent, unlabelled pricing this type exists to
// make impossible — the pair is the run's identity, so callers name both halves.

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
    /// The run's tags (its tags document, compiled): what every `@tag` axis reads.
    /// Scoped to [`SWEEP_FLOW_FP`](super::axes::SWEEP_FLOW_FP).
    tags: Vec<CompiledTag>,
    /// When set, [`compile_combo`](Self::compile_combo) gives every combo these stages
    /// (Pass-2 overlay scan only). `None` on the main Pass-1 strategy, so the axes grid
    /// keeps its flat, fast exit. One overlay clone exists per candidate in
    /// [`ScaleOutPass2::variants`].
    scale_out_overlay: Option<Vec<Stage>>,
    /// Pass-2 config: after each group's cheap fold, re-score top-K combos against
    /// every ladder in [`ScaleOutPass2::variants`]. `None` ⇒ skip Pass 2.
    scale_out_pass2: Option<ScaleOutPass2>,
}

/// A small **grid** of candidate stage plans (ladders) + per-group top-K for Pass 2
/// (see `docs/arch/sweep.md`, *Pass-2 overlay*). Dynamic, not fixed: each top-K combo
/// is re-scored under its own Pass-1 exit PLUS every plan here, and keeps whichever
/// wins. Bounded cost: `variants.len() + 1` scans per top-K combo.
#[derive(Clone, Debug)]
pub struct ScaleOutPass2 {
    pub variants: Vec<Vec<Stage>>,
    pub top_k: usize,
}

impl GenericSweepStrategy {
    /// Build the strategy from a resolved axes model. `as_of` is the run-time now the
    /// deadness clock advances toward (as `run_replay`'s `ReplayConfig`). `tags` is the
    /// run's compiled tags document. `pricing` carries the notional, fill model and
    /// cost model — see [`Pricing`] for why the last two travel together.
    pub fn new(model: AxesModel, pricing: Pricing, as_of: Ts, tags: Vec<CompiledTag>) -> Self {
        let columns = model.columns();
        let grid = SparseGrid {
            max_window_secs: model.max_window_secs(),
            time_horizon_secs: model.metric_value_ceiling(Metric::AgeSec),
            // `held` climbs from the entry, at or before the last trade, so it rides
            // the horizon measured from the last trade.
            stall_horizon_secs: model
                .metric_value_ceiling(Metric::StallSec)
                .max(model.metric_value_ceiling(Metric::HeldSec)),
        };
        Self { model, as_of, corpus_horizon: None, pricing, columns, grid, tags, scale_out_overlay: None, scale_out_pass2: None }
    }

    /// Enable Pass 2: after each group ranks on the cheap axes path, re-score its
    /// top-`top_k` combos against every ladder in `variants` (plus each combo's own
    /// baseline) and keep whichever wins per combo.
    ///
    /// The grid widens to every plan's clocks (a stage deadline must get its tick), so
    /// call this before any token is prepared.
    pub fn set_scale_out_pass2(&mut self, variants: Vec<Vec<Stage>>, top_k: usize) {
        for v in &variants {
            let h = CompiledRule::compile(&self.loaded(RuleParams { stages: v.clone(), ..Default::default() })).clock_horizons;
            self.grid.max_window_secs = self.grid.max_window_secs.max(h.max_window_secs);
            self.grid.time_horizon_secs = self.grid.time_horizon_secs.max(h.time_secs);
            self.grid.stall_horizon_secs = self.grid.stall_horizon_secs.max(h.stall_secs).max(h.held_secs).max(h.stage_secs);
        }
        self.scale_out_pass2 = Some(ScaleOutPass2 { variants, top_k: top_k.max(1) });
    }

    /// Clone this strategy with a compile-time stage overlay (no nested Pass 2).
    fn with_overlay(&self, stages: Vec<Stage>) -> Self {
        Self {
            model: self.model.clone(),
            as_of: self.as_of,
            corpus_horizon: self.corpus_horizon,
            pricing: self.pricing,
            columns: self.columns.clone(),
            grid: self.grid,
            tags: self.tags.clone(),
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
            params.stages = stages.clone();
        }
        CompiledRule::compile(&self.loaded(params))
    }

    /// `params` as the sweep's rule: the sweep fingerprint, unlimited caps.
    fn loaded(&self, params: RuleParams) -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::nil()),
            fingerprint_id: super::axes::SWEEP_FLOW_FP,
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: sol_to_lamports(self.pricing.buy_amount_sol).max(0) as u64,
            // Unlimited caps — see `compile_combo`; changing this changes the meaning
            // of every sweep number, it does not "fix" a parity bug.
            max_concurrent_tokens: u32::MAX,
            max_total_tokens: 0,
            params,
            entry_enabled: true,
        }
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
            EntryResolution::Entered { fill_row, .. } if fast_exit::wants_exit_index(bound, entry) => {
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
        build_series(token, self.columns.clone(), &self.grid, self.as_of, &self.tags)
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
        // Built for every entry whose held side is flat (`fast_exit::FastPlan`): a rule
        // that walks would never read the hulls.
        match entry {
            EntryResolution::Entered { fill_row, .. } if fast_exit::wants_exit_index(bound, entry) => {
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
        scan::resolve_entry(trades, series, bound, &self.pricing)
    }

    fn entry_candidates(
        &self,
        _trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
        out: &mut Self::EntryCands,
    ) {
        scan::entry_candidates(series, bound, out)
    }

    fn resolve_entry_from(
        &self,
        trades: &[CorpusTrade],
        series: &Self::TokenState,
        bound: &Self::BoundParams,
        _params: &Self::Params,
        cands: &mut Self::EntryCands,
    ) -> Self::Entry {
        scan::resolve_entry_from(trades, series, bound, cands, &self.pricing)
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
        // The AVX-512 scan stays selectable for A/B; the default is the O(log n) index.
        // Both must match the walk.
        if crate::sweep::registry::use_simd() {
            fast_exit::resolve_exit_simd(trades, series, bound, entry, &self.pricing, ctx, self.corpus_horizon)
        } else {
            fast_exit::resolve_exit_indexed(trades, series, bound, entry, &self.pricing, ctx, self.corpus_horizon)
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
                // The one canonical stage-JSON writer is `RuleParams::to_value`.
                let wrapper = RuleParams { stages: pass2.variants[vi].clone(), ..Default::default() };
                let ladder_json = wrapper.to_value().get("stages").cloned().unwrap_or(serde_json::Value::Null);
                gr.scale_out_winners.insert(orig_id, ladder_json);
            }
            // Else: no candidate beat this combo's own exit — `gr.metrics` already
            // holds its Pass-1 baseline untouched, and it carries no stages.
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
/// `run_replay` folds: trades interleaved with the tick grid anchored at creation, then
/// a tail up to `min(as_of, last_trade + DEAD_QUIET + TAIL_MARGIN)`. Trades map to
/// `TradeLite` exactly as `replay::load_tokens` does.
///
/// Ticks are emitted **sparsely** ([`SparseGrid`]): omitted ticks are provably
/// identical, for the swept conditions, to the last emitted row, so a week-long token
/// records rows in proportion to its trades, not its lifespan.
///
/// Every tag a column reads is registered from `tags` (scoped to each column's
/// fingerprint), with the spans it is read over — the registration the engine makes
/// from a rule's buffers — and the creator stand-in is seeded before the first fold.
///
/// The tail stops at **this token's own** cut: extending it to the corpus-wide horizon
/// would cost the RAM the sparse grid exists to bound. Clock decisions past the cut are
/// the frozen-tail resolve's (`super::frozen_tail`).
pub(crate) fn build_series(
    token: &CorpusToken,
    columns: Vec<SeriesColumn>,
    grid: &SparseGrid,
    as_of: Ts,
    tags: &[CompiledTag],
) -> MetricSeries {
    let created = token.created_at;
    let mut series = MetricSeries::new(created, columns);
    register_tags(&mut series, tags);
    // The creator a `creator` / `sticky` tag reads, seeded before the first fold as the
    // replay's `FirstSlotSettled` does (it settles ahead of the creation slot's own
    // prints, which share its block time). The lake carries no creator wallet, so this
    // is the stand-in the fold uses when the create names none; a simulate that knows
    // the real creator from `tokens` reads that wallet instead (sim-parity D8).
    if let Some(h) = crate::sweep::projection::creation_slot_first_buyer(&token.trades) {
        series.seed_creator(h);
    }
    fold_sparse(&mut series, created, token.trades.iter().map(|ct| (trade_lite(ct), Some(ct.slot))), grid, as_of, None);
    series
}

/// Register every tag the series' columns read, from `tags`, scoped to each column's
/// fingerprint and over the spans it is read by — the registration the engine makes
/// from a rule's buffers. A tag `tags` does not define stays unregistered (its columns
/// read `NaN`).
pub(crate) fn register_tags(series: &mut MetricSeries, tags: &[CompiledTag]) {
    let reads = Buffers::of(series.columns().iter().map(|c| c.r), SERIES_ANCHOR_CAP);
    let mut scoped: Vec<(FingerprintId, hunter_engine::metrics::tags::TagKey)> =
        series.columns().iter().filter_map(|c| Some((c.fp?, c.r.tag?.key))).collect();
    scoped.sort();
    scoped.dedup();
    for (fp, key) in scoped {
        let (Some(read), Some(tag)) = (reads.tags.iter().find(|t| t.key == key), tags.iter().find(|t| t.key == key)) else {
            continue;
        };
        if read.trade {
            series.ensure_tag(fp, key, &tag.patterns, &read.windows);
        }
        if read.template {
            if let Some(tp) = tag.patterns.templates() {
                series.ensure_template_tag(fp, key, &tp);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
                pnl_sol: p / 100.0,
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
}
