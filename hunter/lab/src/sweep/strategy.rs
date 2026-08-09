//! The strategy surface and the shared fill/cost model.
//!
//! A new strategy implements exactly two traits — [`ParamSpace`] (how to sample
//! its param combos) and [`Strategy`] (the pure `simulate`). The sweep and
//! aggregate layers never know which concrete strategy ran: they only see
//! [`TokenOutcome`] rows.

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::Rng;

use crate::sweep::projection::CorpusTrade;

// The cost model, round-trip pricing, and the exit-reason code are owned by the
// core simulation kernel (one copy of the math, shared by live/paper/sweep). The
// sweep re-exports them so existing `crate::sweep::strategy::{CostModel, …}`
// paths keep resolving while the logic lives in exactly one place .
pub use trading_core::strategies::kernel::{
    quantize_f32, round_trip_with_costs, CostModel, ExitCode,
};

/// Bound on the per-combo `ExitCode::Metrics` breakdown ([`ComboAgg`](crate::sweep::aggregate::ComboAgg)'s
/// `metrics_by_slot`) — a fixed-size counter array, not one entry per distinct
/// metric label, so the aggregate stays O(1) memory per combo regardless of how
/// many conditions a rule authors (the whole reason the sweep can hold `combos ×
/// ComboAgg` resident for hundreds of thousands of combos). 8 covers every
/// authored rule seen in practice (a handful of exit conditions); a rule with
/// more folds its extra conditions into the last slot — never worse than the old
/// single `n_exit_metrics` bucket. See `hunter/docs/plans/sweep/sweep-engine-detail.md`.
pub const N_EXIT_METRIC_SLOTS: usize = 8;

/// How a sweep samples a strategy's param space. Pluggable so a strategy can
/// grid the high-leverage knobs and random/Latin-hypercube the rest, and so the
/// CLI can run a coarse pass then a refine pass around the survivors.
#[derive(Clone, Copy, Debug)]
pub enum SweepMethod {
    /// Full Cartesian grid over the strategy's declared axes.
    Grid,
    /// `n` uniform-random draws (seeded for reproducibility).
    Random { n: usize, seed: u64 },
    /// `n` Latin-hypercube draws (seeded) — better space coverage than random.
    LatinHypercube { n: usize, seed: u64 },
}

impl SweepMethod {
    /// Short tag stored in `combos.parquet` so the analysis layer can tell coarse
    /// from refine and grid from random.
    pub fn tag(&self) -> &'static str {
        match self {
            SweepMethod::Grid => "grid",
            SweepMethod::Random { .. } => "random",
            SweepMethod::LatinHypercube { .. } => "lhs",
        }
    }

    /// Parse the wire form: `grid` | `random:N` | `lhs:N` (default `grid`).
    pub fn parse(s: &str) -> SweepMethod {
        if let Some(n) = s.strip_prefix("random:") {
            SweepMethod::Random { n: n.parse().unwrap_or(500), seed: 42 }
        } else if let Some(n) = s.strip_prefix("lhs:") {
            SweepMethod::LatinHypercube { n: n.parse().unwrap_or(500), seed: 42 }
        } else {
            SweepMethod::Grid
        }
    }
}

/// Coarse→refine search config. After a coarse sampling pass (LHS) over the param
/// space, the grouped driver takes each group's top-`top_k` combos, asks the
/// strategy for a local neighborhood around them ([`ParamSpace::refine`]), and
/// re-sweeps the deduped union of the coarse combos and every group's
/// neighborhood. Because the corpus is already loaded, the refine pass only adds
/// the neighborhood combos — letting a tight coarse budget zoom on the best region
/// instead of spreading a fixed budget evenly over a 15-axis space.
#[derive(Clone, Copy, Debug)]
pub struct RefineSpec {
    /// How many of each group's best combos seed the neighborhood (per group).
    pub top_k: usize,
}

impl RefineSpec {
    /// Default per-group survivor count when `refine:N` omits `:K`.
    pub const DEFAULT_TOP_K: usize = 3;
}

/// Parse the method wire form into a coarse sampler plus an optional refine pass.
/// `refine:N` / `refine:N:K` ⇒ a coarse LHS pass of `N` draws followed by a
/// per-group neighborhood refine seeded by each group's top-`K` combos (`K`
/// default [`RefineSpec::DEFAULT_TOP_K`]). Every other form parses as a plain
/// [`SweepMethod`] with no refine pass.
pub fn parse_method(s: &str) -> (SweepMethod, Option<RefineSpec>) {
    if let Some(rest) = s.strip_prefix("refine:") {
        let mut parts = rest.split(':');
        let n = parts.next().and_then(|x| x.parse().ok()).unwrap_or(500);
        let top_k = parts
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(RefineSpec::DEFAULT_TOP_K)
            .max(1);
        (SweepMethod::LatinHypercube { n, seed: 42 }, Some(RefineSpec { top_k }))
    } else {
        (SweepMethod::parse(s), None)
    }
}

/// The adjacent candidate indices to `i` on an axis of `len` values — `i-1` and
/// `i+1` where they exist. The coordinate-move neighborhood a refine pass walks
/// one axis at a time (holding the others fixed), so a survivor yields at most
/// `2 ×` (multi-valued axes) neighbors — linear in axes, not the `3^axes` a full
/// local grid would cost.
pub fn neighbor_indices(i: usize, len: usize) -> impl Iterator<Item = usize> {
    let prev = i.checked_sub(1);
    let next = if i + 1 < len { Some(i + 1) } else { None };
    prev.into_iter().chain(next)
}

/// Index of `v` in `xs` by exact equality. Generic on purpose: the float axes
/// route their lookup through here so `clippy::float_cmp` doesn't trip — the
/// values being matched come straight from the candidate list, so exact equality
/// is correct.
pub fn index_of<T: PartialEq>(xs: &[T], v: &T) -> Option<usize> {
    xs.iter().position(|x| x == v)
}

/// A Latin-hypercube **index plan** over discrete axes: for `n` draws across axes
/// whose candidate counts are `axis_lens`, returns one column of `n` value-indices
/// per axis. Within a column every candidate index appears ⌊n/len⌋ or ⌈n/len⌉
/// times (balanced strata — no value is over- or under-sampled), and each column
/// is shuffled independently so the axes decorrelate. Combo `i` reads its value on
/// each axis from `plan[axis][i]`.
///
/// This is the discrete analogue of classic continuous LHS (one sample per bin per
/// axis, permuted per axis): it guarantees the even per-axis coverage that a plain
/// uniform draw ([`SweepMethod::Random`]) only reaches in expectation — the point
/// of preferring LHS on a tight sample budget. Axes with `len == 0` yield an empty
/// column (callers guard against empty axes before sampling). Shares the caller's
/// seeded `rng`, so a fixed seed reproduces the plan.
pub fn lhs_index_plan(rng: &mut StdRng, n: usize, axis_lens: &[usize]) -> Vec<Vec<usize>> {
    axis_lens
        .iter()
        .map(|&len| {
            if len == 0 {
                return Vec::new();
            }
            // Balanced strata: index `i % len` — each candidate appears ⌊n/len⌋ or
            // ⌈n/len⌉ times across the column.
            let mut col: Vec<usize> = (0..n).map(|i| i % len).collect();
            // Fisher–Yates shuffle this column independently of the others, so the
            // axes' strata don't line up into a diagonal.
            for i in (1..col.len()).rev() {
                let j = rng.gen_range(0..=i);
                col.swap(i, j);
            }
            col
        })
        .collect()
}

/// Compact, `Copy` per-(combo, token) result. Holds no `String` so the hot loop
/// never allocates and the value stays register-friendly; the mint is recovered
/// from the corpus by token index at emit time. Exit reason is a small code, not
/// a string.
///
/// The `entry_*`/`exit_*` time/price fields are `Option<DateTime<Utc>>` and
/// `Option<f64>` — `DateTime<Utc>` is `Copy`, so the struct stays `Copy`.
/// They are populated only in the single-combo re-simulation path (the drill-in
/// endpoint); the full sweep folds these into `ComboAgg` aggregates and never
/// reads the individual timestamps, so the hot-path cost is a handful of `None`
/// writes per outcome (register-level, no allocation). The `exit_metric*` fields
/// are the exception: [`ComboAgg::record`](crate::sweep::aggregate::ComboAgg::record)
/// DOES read `exit_metric_slot` (to bucket `n_exit_metrics_by_slot`), but it's
/// still a `Copy` field already resolved at bind time — no per-token cost either.
#[derive(Clone, Copy, Debug)]
pub struct TokenOutcome {
    /// Whether the strategy took a position on this token under these params.
    pub fired: bool,
    /// Seconds entry→exit (0 when not fired or still open).
    pub holding_secs: i64,
    /// Net round-trip PnL after the fill/cost model, as % of notional.
    pub pnl_percent: f32,
    /// Net round-trip PnL after the fill/cost model, in SOL.
    pub pnl_sol: f32,
    /// Why it exited (or `Open`/`NoEntry`).
    pub exit: ExitCode,
    /// The authored metric an `ExitCode::Metrics` exit fired on (`None` for every
    /// other exit / still-`Open` / `NoEntry`). Together with `exit_operator` /
    /// `exit_metric_value` this reconstructs the same `metric op value` label
    /// [`hunter_engine::event::format_metric_exit_label`] renders for live/paper —
    /// without it the grouped sweep collapsed every authored condition down to
    /// the bare `"Metrics"` code name. Resolved once per (combo, token) from the
    /// winning `MetricReq` (bind-time data, no live-value recompute), not per row.
    pub exit_metric: Option<hunter_engine::metrics::MetricId>,
    /// The authored condition operator paired with `exit_metric` (see above).
    pub exit_operator: Option<hunter_engine::metrics::evaluator::Operator>,
    /// The authored condition **threshold** paired with `exit_metric` — not the
    /// live metric reading at exit, mirroring `ExitReason::Metrics::value`.
    pub exit_metric_value: Option<f64>,
    /// 0-based position among this rule's OWN authored exit reqs (capped at
    /// `N_EXIT_METRIC_SLOTS - 1`) — the aggregate's bounded per-metric bucket
    /// index. `None` for every non-metric exit.
    pub exit_metric_slot: Option<u8>,
    /// Block time of the simulated entry fill (`None` when not fired).
    pub entry_time: Option<DateTime<Utc>>,
    /// Simulated entry fill price in SOL/token (`None` when not fired).
    pub entry_price: Option<f64>,
    /// Slot of the entry fill trade (`None` when not fired). Lets the drill-in
    /// endpoint resolve the fill's real `tx_signature` from the `trades` table —
    /// the slim `CorpusTrade` carries no signature, so it can't ride along here.
    pub entry_slot: Option<u64>,
    /// Block time of the simulated exit fill (`None` when not fired or still open).
    pub exit_time: Option<DateTime<Utc>>,
    /// Simulated exit fill price in SOL/token (`None` when not fired or still open).
    pub exit_price: Option<f64>,
    /// Slot of the exit fill trade (`None` when not fired or still open). See
    /// [`Self::entry_slot`].
    pub exit_slot: Option<u64>,
}

impl TokenOutcome {
    /// The strategy never entered this token under these params.
    pub fn no_entry() -> Self {
        Self {
            fired: false,
            holding_secs: 0,
            pnl_percent: 0.0,
            pnl_sol: 0.0,
            exit: ExitCode::NoEntry,
            exit_metric: None,
            exit_operator: None,
            exit_metric_value: None,
            exit_metric_slot: None,
            entry_time: None,
            entry_price: None,
            entry_slot: None,
            exit_time: None,
            exit_price: None,
            exit_slot: None,
        }
    }
}

/// How a sweep samples a strategy's param space. One of the two traits a new
/// strategy implements.
pub trait ParamSpace {
    /// The concrete param set the strategy's `simulate` consumes. `Copy`/`Clone`
    /// and `Send + Sync` so the sweep can fan a slice of them across `rayon`.
    type Params: Clone + Send + Sync + 'static;

    /// Materialise the combos to evaluate. The sweep treats the returned `Vec`'s
    /// index as the `combo_id` written to `combos.parquet`.
    fn sample(&self, method: SweepMethod) -> Vec<Self::Params>;

    /// Local neighborhood around a set of promising combos (a coarse pass's
    /// per-group survivors), for the coarse→refine search. Each survivor is
    /// expanded by moving one axis at a time to an adjacent candidate value
    /// ([`neighbor_indices`]), holding the others fixed. The default returns
    /// nothing (a strategy that opts out of coarse→refine). Output may duplicate
    /// the survivors or each other — the grouped driver dedups by `params_json`
    /// before re-sweeping.
    fn refine(&self, _survivors: &[Self::Params]) -> Vec<Self::Params> {
        Vec::new()
    }

    /// Reorder the *final* combo set in place so combos sharing an entry-param
    /// identity ([`Strategy::entry_key`]) land **contiguously**. The engine keeps a
    /// single-slot cache of [`Strategy::EntryCands`] that only hits while consecutive
    /// combos share a key, so a full `Grid` (entry knobs are the high-order digits ⇒
    /// already contiguous) walks each entry class once — but `Random`/`LatinHypercube`/
    /// `refine` hand out a shuffled order, collapsing the cache to ~one Stage-A walk
    /// per combo. A strategy whose entry is expensive overrides this to
    /// stable-sort by its entry key, restoring the contiguous-block property under
    /// every sampler. Called once on the shared combo vec before the per-group
    /// sweeps, so `combo_id` (= position) stays consistent across groups. The
    /// default is a no-op (param-free entries gain nothing). Decision-neutral: it
    /// only changes evaluation order, never any combo's resolved outcome.
    fn order_for_entry_cache(&self, _params: &mut [Self::Params]) {}
}

/// The pure black-box backtest, **factored into entry then exit** so the engine
/// can hoist the expensive, *exit-independent* part of the entry to **once per
/// distinct entry-param tuple per token** — see
/// [`crate::sweep::engine::run_sweep`]. A new strategy owns its own entry/exit
/// logic and just returns a [`TokenOutcome`]; the engine never inspects how.
///
/// **The entry is two-stage on purpose.** The *resolved* entry is NOT a function
/// of the entry params alone: the engine's `can_enter` refuses to buy while the
/// exit conditions already hold, so two combos sharing an
/// [`Strategy::entry_key`] but differing on the exit side can legitimately enter
/// on different rows. Caching the resolved entry by `entry_key` silently donates the
/// first combo's entered set to every sibling in its class — the entry-cache
/// poisoning bug (mechanism and proof in `docs/plans/sweep/sim-parity.md`). So the
/// engine caches only what is provably exit-independent:
///
/// * [`Strategy::entry_candidates`] (**Stage A**) — the expensive walk, run once
///   per `entry_key` per token; its output is what the engine caches.
/// * [`Strategy::resolve_entry_from`] (**Stage B**) — resolves *this* combo's entry
///   out of those shared candidates, applying whatever exit-dependent veto the
///   strategy has. Runs per combo, so it must be cheap.
///
/// [`Strategy::resolve_entry`] stays as the fused one-shot reference (Stage A+B in
/// one call, no cache). The default Stage A/B bodies *are* that reference, so a
/// strategy that opts out is correct by construction and merely re-walks per combo.
pub trait Strategy: ParamSpace + Send + Sync {
    /// The resolved per-token entry for one combo (price/time, or a "no entry"
    /// variant), passed into that combo's [`Strategy::resolve_exit`].
    type Entry;

    /// The entry-param identity of a combo. Two combos with equal `EntryKey` share
    /// the same [`Strategy::EntryCands`] on any token, so the engine runs Stage A
    /// once per distinct key. `PartialEq` is all the engine needs (it keeps the last
    /// key and recomputes only when it changes — which, on a grid, is once per
    /// contiguous exit-block).
    ///
    /// Equal keys do **not** imply equal resolved entries — that is exactly the
    /// assumption the poisoning bug rested on. See the trait doc.
    type EntryKey: PartialEq;

    /// Stage A's output: the exit-**independent** entry work for one (token, entry
    /// class), shared by every combo in that class. Reused in place across combos
    /// and tokens (the engine keeps one instance per token scan), so a strategy
    /// should recycle its buffers rather than reallocate. `()` for a strategy that
    /// keeps the fused [`Strategy::resolve_entry`] default.
    type EntryCands: Default + Send;

    /// Param-independent, **per-token** state the engine computes once before a
    /// token's combo loop and threads into every [`Strategy::resolve_entry`] on
    /// that token. Lets a strategy hoist work that depends only on the trade
    /// slice out of the per-entry-key resolve, where it would otherwise rebuild
    /// once per distinct entry tuple. A strategy with no such state sets this to
    /// `()`. Must be `Send + Sync` — the wave driver builds/scans states across
    /// rayon threads.
    type TokenState: Send + Sync;

    /// Per-combo state the engine materialises **once per combo batch** via
    /// [`Strategy::bind_param`] before scanning tokens (e.g. `CompiledRule` for
    /// the generic engine). Lets `Params` stay index-only / tiny while the hot
    /// loop reads a bound form. Unit `()` for strategies whose `Params` are
    /// already the scan input.
    type BoundParams: Send + Sync;

    /// Per-`(token, entry)` exit context rebuilt when the resolved entry moves
    /// (e.g. the generic engine's prefix-extrema [`ExitIndex`](crate::sweep::generic::exit_index::ExitIndex)).
    /// Unit `()` for strategies that resolve exits without shared entry-local state.
    /// Must be `Default` so the engine can recycle one instance per token scan.
    type ExitCtx: Default + Send;

    /// Identity of the [`Strategy::ExitCtx`] a `(bound combo, resolved entry)` pair
    /// needs. The engine rebuilds the context only when this key changes — under
    /// per-combo entries the context is not stale-once-per-entry-class, and
    /// rebuilding it per combo would undo the point of caching it at all.
    ///
    /// The key must distinguish every context the strategy would build differently,
    /// **including "no context wanted"** (else a combo that cleared the context
    /// would keep a later combo from rebuilding it). It is an optimization key, not
    /// a correctness one: a strategy's `resolve_exit` must still be correct given a
    /// cleared/absent context, which is what makes a conservative key safe.
    type ExitCtxKey: PartialEq;

    /// The entry-param signature of a combo. Combos sharing it share Stage A's
    /// [`Strategy::EntryCands`] — **not** necessarily a resolved entry.
    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey;

    /// Bind one combo's [`Strategy::Params`] into [`Strategy::BoundParams`] once
    /// per batch (not per token). Default strategies return `()`.
    fn bind_param(&self, params: &Self::Params) -> Self::BoundParams;

    /// Compute the shared [`Strategy::TokenState`] for one token, **once** before
    /// any combo runs against it. Receives the whole [`CorpusToken`] (not just its
    /// trade slice) so the generic engine can anchor its metric clock at the token's
    /// `created_at`; a strategy that resolves purely from trades reads `token.trades`.
    fn prepare_token(&self, token: &crate::sweep::corpus::CorpusToken) -> Self::TokenState;

    /// The [`Strategy::ExitCtxKey`] this `(bound, entry)` pair induces — see the
    /// associated type. No default: a wrong (too coarse) key silently disables a
    /// rebuild the strategy needed, so every impl states its own.
    fn exit_ctx_key(&self, bound: &Self::BoundParams, entry: &Self::Entry) -> Self::ExitCtxKey;

    /// Rebuild [`Strategy::ExitCtx`] for a freshly resolved entry. Called by the
    /// engine exactly when [`Strategy::exit_ctx_key`] changes. Default is a no-op
    /// for unit `ExitCtx`.
    fn build_exit_ctx(
        &self,
        _trades: &[CorpusTrade],
        _state: &Self::TokenState,
        _bound: &Self::BoundParams,
        _entry: &Self::Entry,
        _params: &Self::Params,
        _ctx: &mut Self::ExitCtx,
    ) {
    }

    /// Resolve the entry on one token's full trade history under a combo's **entry**
    /// params, given the pre-computed [`Strategy::TokenState`] and batch-bound
    /// [`Strategy::BoundParams`]. Pure — safe from many `rayon` threads.
    ///
    /// The fused Stage A+B reference. The engine's fold does **not** call this (it
    /// runs the two stages so Stage A can be shared); single-combo callers and the
    /// guards do, and the two-stage path is asserted equal to it in debug builds.
    fn resolve_entry(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        params: &Self::Params,
    ) -> Self::Entry;

    /// **Stage A** — compute the exit-independent entry candidates for one token
    /// under this combo's *entry* params, into the engine's recycled `out`. Called
    /// once per distinct [`Strategy::entry_key`] per token; every combo in that
    /// class then resolves through [`Strategy::resolve_entry_from`].
    ///
    /// Must depend only on the entry side. Anything read off the exit params here
    /// re-introduces the poisoning bug the two-stage split exists to kill.
    ///
    /// Default: a no-op, paired with the fused default below.
    fn entry_candidates(
        &self,
        _trades: &[CorpusTrade],
        _state: &Self::TokenState,
        _bound: &Self::BoundParams,
        _params: &Self::Params,
        _out: &mut Self::EntryCands,
    ) {
    }

    /// **Stage B** — resolve *this* combo's entry from the shared Stage-A
    /// candidates, applying the exit-dependent veto. Runs per combo, so it must be
    /// cheap; `cands` is `&mut` so a strategy can memoize per-candidate work
    /// (e.g. one fill resolution shared by every combo landing on the same row)
    /// across the class.
    ///
    /// Default: ignore the candidates and run the fused [`Strategy::resolve_entry`]
    /// — correct, just uncached.
    fn resolve_entry_from(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        params: &Self::Params,
        _cands: &mut Self::EntryCands,
    ) -> Self::Entry {
        self.resolve_entry(trades, state, bound, params)
    }

    /// Resolve the exit + economics given a pre-resolved `entry` and its
    /// [`Strategy::ExitCtx`], under a combo's **exit** params / bound form.
    /// Returns a `Copy` [`TokenOutcome`].
    fn resolve_exit(
        &self,
        trades: &[CorpusTrade],
        state: &Self::TokenState,
        bound: &Self::BoundParams,
        entry: &Self::Entry,
        params: &Self::Params,
        ctx: &Self::ExitCtx,
    ) -> TokenOutcome;

    /// Flatten one param set to a JSON object stored with the combo's result row,
    /// so the UI can show/sort by any knob without a per-strategy schema.
    fn params_json(&self, params: &Self::Params) -> serde_json::Value;

    /// Estimated resident bytes of [`Strategy::prepare_token`] for this token.
    /// Used to size the series **wave** (how many heavy `TokenState`s may stay
    /// alive together). Default `0` — unit/`()` state strategies (legacy tpsl /
    /// swing wrappers) keep full rayon parallelism. The generic engine returns
    /// its `MetricSeries` estimate.
    fn token_state_bytes_estimate(&self, _token: &crate::sweep::corpus::CorpusToken) -> usize {
        0
    }

    /// Optional Pass-2 hook: after a group's cheap fold, re-score selected combos
    /// under an alternate compiled form (e.g. fixed `scale_out` overlay). Default
    /// no-op. Called with metrics still resident, before the sink persists.
    fn post_group_rescore(
        &self,
        _params: &[Self::Params],
        _corpus: &crate::sweep::corpus::Corpus,
        _token_idx: &[usize],
        _gr: &mut crate::sweep::grouped_engine::GroupResult,
        _coverage: crate::sweep::grouped_engine::CoverageFloor,
        _observer: &dyn crate::sweep::progress::SweepObserver,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cost model (`round_trip_with_costs`/`CostModel`) and `ExitCode` live in
    // `trading_core::strategies::kernel`, and their unit tests live there too — the
    // sweep must not re-test the same math. What remains here is sweep-only.

    #[test]
    fn lhs_plan_is_balanced_shaped_and_reproducible() {
        use rand::SeedableRng;
        let lens = [3usize, 2, 1, 4];
        let n = 10;

        let mut rng = StdRng::seed_from_u64(7);
        let plan = lhs_index_plan(&mut rng, n, &lens);

        assert_eq!(plan.len(), lens.len(), "one column per axis");
        for (axis, &len) in lens.iter().enumerate() {
            let col = &plan[axis];
            assert_eq!(col.len(), n, "every column has n rows");
            // Balanced strata: each candidate index appears ⌊n/len⌋ or ⌈n/len⌉ times,
            // and only valid indices appear.
            let mut counts = vec![0usize; len];
            for &ix in col {
                assert!(ix < len, "index in range");
                counts[ix] += 1;
            }
            let lo = n / len;
            let hi = (n + len - 1) / len;
            assert!(
                counts.iter().all(|&c| c == lo || c == hi),
                "axis {axis} strata unbalanced: {counts:?}"
            );
        }

        // Same seed ⇒ identical plan (reproducible runs).
        let mut rng2 = StdRng::seed_from_u64(7);
        let plan2 = lhs_index_plan(&mut rng2, n, &lens);
        assert_eq!(plan, plan2);
    }
}
