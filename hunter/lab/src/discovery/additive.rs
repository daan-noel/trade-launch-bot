//! The **additive scan mode** — N independent sub-models swept as one flat combo
//! space over **one shared per-token precompute** (plan §6.1 / D2).
//!
//! Discovery is scan/precompute-bound, not fold-bound: the wall-clock is the corpus
//! load plus the per-token [`MetricSeries`](hunter_engine::metrics::series::MetricSeries)
//! build, not the handful of combos. Running N sub-models as N separate sweeps would
//! rebuild that series N times for what is, by construction, one union of columns.
//!
//! [`AdditiveStrategy`] is the fix, and it is a **scan mode, not a second engine**: it
//! widens every sub-strategy onto the union of all their columns and the widest grid
//! horizons ([`GenericSweepStrategy::share_precompute`]), presents `Σ combo_count`
//! combos as one param space, and delegates every decision back to the same
//! `GenericSweepStrategy` scan. The sweep engine then builds each token's series once
//! for the whole set, and its RAM ladder / wave driver / rayon / cancellation are
//! reused verbatim.
//!
//! Every discovery layer that fans out over sub-models rides this: Layer 1 screens one
//! metric per segment, Layer 2 grids one family per segment and then one interaction
//! check per segment.
//!
//! **Cost model.** `Σ combo_count`, never the product — that is the whole point. The
//! caller is responsible for bounding each sub-model; nothing here silently truncates.

use anyhow::Result;

use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::engine::{combo_batch_size, run_sweep};
use crate::sweep::generic::axes::AxesModel;
use crate::sweep::generic::strategy::{GenericCombo, SparseGrid};
use crate::sweep::generic::{GenericSweepStrategy, Pricing};
use crate::sweep::progress::SweepObserver;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod, TokenOutcome};

use hunter_engine::metrics::flow_split::FlowPatterns;
use hunter_engine::metrics::series::SeriesColumn;
use hunter_engine::metrics::Ts;

/// One combo of the additive space: which sub-model, and which of its combos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdditiveCombo {
    pub segment: u32,
    pub pick: u32,
}

/// N sub-models as one sweep [`Strategy`] over one shared precompute.
pub struct AdditiveStrategy {
    segments: Vec<GenericSweepStrategy>,
    /// `segments[i].model().combo_count()`, cached — the row split every caller uses.
    lens: Vec<usize>,
}

impl AdditiveStrategy {
    /// Widen every sub-model onto the shared column union + widest grid.
    ///
    /// Widening is decision-neutral (see [`GenericSweepStrategy::share_precompute`]):
    /// a segment records columns it never reads and ticks the sparse grid would have
    /// proved static, and reads exactly the values it would have read alone.
    pub fn new(
        models: Vec<AxesModel>,
        pricing: Pricing,
        as_of: Ts,
        flow: Option<&FlowPatterns>,
    ) -> Self {
        let mut segments: Vec<GenericSweepStrategy> = models
            .into_iter()
            .map(|m| GenericSweepStrategy::new(m, pricing, as_of, flow.cloned()))
            .collect();
        let mut columns: Vec<SeriesColumn> = Vec::new();
        let mut grid = SparseGrid::default();
        for s in &segments {
            for c in s.columns() {
                if !columns.contains(c) {
                    columns.push(*c);
                }
            }
            let g = s.grid();
            grid.max_window_secs = grid.max_window_secs.max(g.max_window_secs);
            grid.time_horizon_secs = grid.time_horizon_secs.max(g.time_horizon_secs);
            grid.stall_horizon_secs = grid.stall_horizon_secs.max(g.stall_horizon_secs);
        }
        for s in &mut segments {
            s.share_precompute(columns.clone(), grid);
        }
        let lens = segments.iter().map(|s| s.model().combo_count()).collect();
        Self { segments, lens }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Combos in segment `i`.
    pub fn pick_count(&self, i: usize) -> usize {
        self.lens[i]
    }

    /// The sub-model of segment `i` — how a caller reads back which values a combo
    /// picked ([`AxesModel::combo_picks`]) or its promotable params.
    pub fn model(&self, i: usize) -> &AxesModel {
        self.segments[i].model()
    }

    /// The canonical [`RuleParams`](hunter_engine::rule_params::RuleParams) JSON for
    /// one combo — the handoff into promote / `simulate_one_combo`.
    pub fn params_json_of(&self, c: AdditiveCombo) -> serde_json::Value {
        self.segments[c.segment as usize].params_json(&GenericCombo { idx: c.pick as usize })
    }

    /// The flat combo list, in `(segment, pick)` order — also the order results come
    /// back in, so `combo_id` indexes straight into it.
    pub fn combos(&self) -> Vec<AdditiveCombo> {
        let mut out = Vec::with_capacity(self.lens.iter().sum());
        for (s, len) in self.lens.iter().enumerate() {
            for pick in 0..*len {
                out.push(AdditiveCombo { segment: s as u32, pick: pick as u32 });
            }
        }
        out
    }

    /// Sweep the whole additive space in ONE pass and split the rows per segment.
    /// `rows[i]` has `pick_count(i)` entries, indexed by pick.
    pub fn run(
        &self,
        corpus: &Corpus,
        observer: &dyn SweepObserver,
    ) -> Result<Vec<Vec<ComboMetrics>>> {
        if self.segments.is_empty() {
            return Ok(Vec::new());
        }
        let params = self.combos();
        let threads = rayon::current_num_threads().max(1);
        let batch = combo_batch_size(params.len(), threads);
        observer.set_total(corpus.token_count(), params.len());
        let (_stats, metrics) = run_sweep(self, &params, corpus, observer, batch)?;
        let mut out = Vec::with_capacity(self.segments.len());
        let mut base = 0usize;
        for len in &self.lens {
            out.push(metrics[base..base + len].to_vec());
            base += len;
        }
        Ok(out)
    }

    fn inner(&self, c: &AdditiveCombo) -> (&GenericSweepStrategy, GenericCombo) {
        (&self.segments[c.segment as usize], GenericCombo { idx: c.pick as usize })
    }
}

impl ParamSpace for AdditiveStrategy {
    type Params = AdditiveCombo;

    /// Always the full additive set. Sampling makes no sense here: the space is
    /// already `Σ combo_count` by construction, and dropping combos would punch holes
    /// in the response curves / grids the callers read.
    fn sample(&self, _method: SweepMethod) -> Vec<Self::Params> {
        self.combos()
    }

    // No `order_for_entry_cache` override — deliberately. Each sub-model already
    // orders its entry axes first, so flat `(segment, pick)` order is entry-contiguous
    // within every segment, and leaving it alone keeps `combo_id == flat index`, which
    // is what lets `run` split rows per segment without a lookup table.
}

impl Strategy for AdditiveStrategy {
    type Entry = <GenericSweepStrategy as Strategy>::Entry;
    /// Segment-qualified: two segments' sub-keys are unrelated, so a bare sub-key
    /// would let one segment's cached entry be served to another segment's combo.
    type EntryKey = (u32, <GenericSweepStrategy as Strategy>::EntryKey);
    type EntryCands = <GenericSweepStrategy as Strategy>::EntryCands;
    type TokenState = <GenericSweepStrategy as Strategy>::TokenState;
    type BoundParams = <GenericSweepStrategy as Strategy>::BoundParams;
    type ExitCtx = <GenericSweepStrategy as Strategy>::ExitCtx;
    type ExitCtxKey = <GenericSweepStrategy as Strategy>::ExitCtxKey;

    fn entry_key(&self, p: &Self::Params) -> Self::EntryKey {
        let (s, c) = self.inner(p);
        (p.segment, s.entry_key(&c))
    }

    fn exit_ctx_key(&self, bound: &Self::BoundParams, entry: &Self::Entry) -> Self::ExitCtxKey {
        // Segment-independent: every segment shares one series/column layout, so the
        // sub-strategy's key (fill row + wants-index) is the whole identity.
        self.segments[0].exit_ctx_key(bound, entry)
    }

    fn bind_param(&self, p: &Self::Params) -> Self::BoundParams {
        let (s, c) = self.inner(p);
        s.bind_param(&c)
    }

    /// One series for the whole run — every segment shares the same columns/grid, so
    /// segment 0 speaks for all of them.
    fn prepare_token(&self, token: &CorpusToken) -> Self::TokenState {
        self.segments[0].prepare_token(token)
    }

    fn build_exit_ctx(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        p: &Self::Params,
        ctx: &mut Self::ExitCtx,
    ) {
        let (s, c) = self.inner(p);
        s.build_exit_ctx(trades, state, bound, entry, &c, ctx);
    }

    fn resolve_entry(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        p: &Self::Params,
    ) -> Self::Entry {
        let (s, c) = self.inner(p);
        s.resolve_entry(trades, state, bound, &c)
    }

    fn entry_candidates(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        p: &Self::Params,
        out: &mut Self::EntryCands,
    ) {
        let (s, c) = self.inner(p);
        s.entry_candidates(trades, state, bound, &c, out)
    }

    fn resolve_entry_from(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        p: &Self::Params,
        cands: &mut Self::EntryCands,
    ) -> Self::Entry {
        let (s, c) = self.inner(p);
        s.resolve_entry_from(trades, state, bound, &c, cands)
    }

    fn resolve_exit(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        p: &Self::Params,
        ctx: &Self::ExitCtx,
    ) -> TokenOutcome {
        let (s, c) = self.inner(p);
        s.resolve_exit(trades, state, bound, entry, &c, ctx)
    }

    fn params_json(&self, p: &Self::Params) -> serde_json::Value {
        let (s, c) = self.inner(p);
        s.params_json(&c)
    }

    fn token_state_bytes_estimate(&self, token: &CorpusToken) -> usize {
        self.segments[0].token_state_bytes_estimate(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::generic::axes::{AxesRequest, AxisSide, AxisSpec};
    use crate::sweep::progress::NoopObserver;
    use chrono::Utc;
    use hunter_engine::metrics::MetricId;

    use super::super::fixtures::{corpus, pricing};

    fn metric_axis(group: &str, metric: &str, window: Option<f64>, vals: Vec<f64>) -> AxisSpec {
        AxisSpec {
            kind: "metric".to_string(),
            side: Some(AxisSide::Entry),
            group: Some(group.to_string()),
            metric: Some(metric.to_string()),
            operator: Some(hunter_engine::metrics::evaluator::Operator::Gte),
            window,
            // `off` first, then the values — the with-vs-without sentinel.
            values: std::iter::once(None).chain(vals.into_iter().map(Some)).collect(),
        }
    }

    fn tp(v: f64) -> AxisSpec {
        AxisSpec {
            kind: "take_profit".to_string(),
            side: None,
            group: None,
            metric: None,
            operator: None,
            window: None,
            values: vec![Some(v)],
        }
    }

    fn model(axes: Vec<AxisSpec>) -> AxesModel {
        AxesModel::resolve(&AxesRequest { axes }).expect("axes resolve")
    }

    fn two_segment() -> AdditiveStrategy {
        AdditiveStrategy::new(
            vec![
                model(vec![metric_axis("m_snapshot", "time", None, vec![5.0, 30.0, 60.0]), tp(30.0)]),
                model(vec![metric_axis("m_snapshot", "liquidity", None, vec![40.0, 50.0]), tp(30.0)]),
            ],
            pricing(),
            Utc::now(),
            None,
        )
    }

    /// The shared-precompute contract: every segment is widened onto ONE column union
    /// + grid, so the engine builds a token's series once for the whole run.
    #[test]
    fn segments_share_one_precompute() {
        let s = AdditiveStrategy::new(
            vec![
                model(vec![metric_axis("m_snapshot", "time", None, vec![5.0, 30.0]), tp(30.0)]),
                model(vec![
                    metric_axis("m_price_window", "trail", Some(45.0), vec![10.0]),
                    tp(30.0),
                ]),
            ],
            pricing(),
            Utc::now(),
            None,
        );
        let cols: Vec<_> = s.segments.iter().map(|x| x.columns().to_vec()).collect();
        assert_eq!(cols[0], cols[1], "segments must share the column union");
        assert!(cols[0].contains(&SeriesColumn::Static(MetricId::Time)));
        assert!(cols[0].contains(&SeriesColumn::Window(MetricId::WinTrail, 45.0)));
        // The union grid takes the widest horizon of either segment: segment 0 owns
        // the `time` ceiling, segment 1 the price window.
        let grids: Vec<_> = s.segments.iter().map(|x| x.grid()).collect();
        assert_eq!(grids[0].time_horizon_secs, grids[1].time_horizon_secs);
        assert!(grids[0].time_horizon_secs >= 30.0);
        assert_eq!(grids[0].max_window_secs, 45.0);
    }

    /// The budget is `Σ combo_count`, never the product — the whole point.
    #[test]
    fn combo_space_is_additive() {
        let s = two_segment();
        // (off + 3) + (off + 2) = 7, not 4 × 3 = 12.
        assert_eq!(s.combos().len(), 7);
        assert_eq!(s.pick_count(0), 4);
        assert_eq!(s.pick_count(1), 3);
        let combos = s.combos();
        assert_eq!(combos[0], AdditiveCombo { segment: 0, pick: 0 });
        assert_eq!(combos[4], AdditiveCombo { segment: 1, pick: 0 });
    }

    /// Entry keys must be segment-qualified — otherwise the engine's single-slot entry
    /// cache can hand one segment's resolved entry to another segment's combo.
    #[test]
    fn entry_keys_are_segment_qualified() {
        let s = two_segment();
        let a = s.entry_key(&AdditiveCombo { segment: 0, pick: 1 });
        let b = s.entry_key(&AdditiveCombo { segment: 1, pick: 1 });
        assert_ne!(a, b, "same sub-key on two segments must not collide");
    }

    /// End-to-end: one sweep, rows split back per segment in pick order, and every
    /// segment's `off` pick resolving to the same bare baseline rule.
    #[test]
    fn run_splits_rows_per_segment_in_pick_order() {
        let s = two_segment();
        let c = corpus(6);
        let rows = s.run(&c, &NoopObserver).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 4);
        assert_eq!(rows[1].len(), 3);
        // `combo_id` stays the FLAT index across the split — that is the property the
        // no-reorder decision buys, and callers rely on it.
        assert_eq!(rows[0][0].combo_id, 0);
        assert_eq!(rows[1][0].combo_id, 4);
        // Both segments' `off` picks are the bare TP-only rule ⇒ identical results.
        assert_eq!(rows[0][0].n_fired, rows[1][0].n_fired);
        assert_eq!(rows[0][0].total_pnl_sol, rows[1][0].total_pnl_sol);
        // A `time >= 30` gate can only fire on a subset of what `off` fires on.
        assert!(rows[0][2].n_fired <= rows[0][0].n_fired);
    }

    /// A combo's params round-trip to the canonical `RuleParams` JSON — the handoff
    /// into promote / `simulate_one_combo` that Layer 3 depends on.
    #[test]
    fn params_json_is_promotable() {
        let s = two_segment();
        let j = s.params_json_of(AdditiveCombo { segment: 0, pick: 2 });
        hunter_engine::rule_params::RuleParams::parse(&j).expect("promotable params");
        // Pick 2 is the second real value on an off-first menu (`time >= 30`).
        assert_eq!(
            s.model(0).combo_picks(2)[0],
            2,
            "pick indexes the metric axis directly (it is the high-order digit)"
        );
    }
}
