//! The strategy surface and the shared fill/cost model.
//!
//! A new strategy implements exactly two traits — [`ParamSpace`] (how to sample
//! its param combos) and [`Strategy`] (the pure `simulate`). The sweep and
//! aggregate layers never know which concrete strategy ran: they only see
//! [`TokenOutcome`] rows.

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::Rng;

use crate::sweep::projection::SweepTrade;
use pump_trader::constants::{
    COMPUTE_UNIT_LIMIT_CURVE_BUY, COMPUTE_UNIT_LIMIT_CURVE_SELL,
    COMPUTE_UNIT_PRICE_MICRO_LAMPORTS, LAMPORTS_PER_SOL,
};

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
/// reads the individual timestamps, so the hot-path cost is four `None` writes
/// per outcome (register-level, no allocation).
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
    /// Block time of the simulated entry fill (`None` when not fired).
    pub entry_time: Option<DateTime<Utc>>,
    /// Simulated entry fill price in SOL/token (`None` when not fired).
    pub entry_price: Option<f64>,
    /// Block time of the simulated exit fill (`None` when not fired or still open).
    pub exit_time: Option<DateTime<Utc>>,
    /// Simulated exit fill price in SOL/token (`None` when not fired or still open).
    pub exit_price: Option<f64>,
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
            entry_time: None,
            entry_price: None,
            exit_time: None,
            exit_price: None,
        }
    }
}

/// Compact exit-reason code. Mirrors tpsl2's `ExitReason` plus the two
/// non-exit terminal states (`Open`, `NoEntry`) the sweep needs to distinguish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    NoEntry = 0,
    Open = 1,
    TakeProfit = 2,
    StopLoss = 3,
    TrailingStop = 4,
    Stall = 5,
    TimeStop = 6,
    LiquidityExit = 7,
    CohortExit = 8,
}

impl ExitCode {
    /// Map a strategy's exit-reason string (the live ladder's `as_str`) to a code.
    pub fn from_reason(reason: &str) -> Self {
        match reason {
            "TakeProfit" => ExitCode::TakeProfit,
            "StopLoss" => ExitCode::StopLoss,
            "TrailingStop" => ExitCode::TrailingStop,
            "Stall" => ExitCode::Stall,
            "TimeStop" => ExitCode::TimeStop,
            "LiquidityExit" => ExitCode::LiquidityExit,
            "CohortExit" => ExitCode::CohortExit,
            "Open" => ExitCode::Open,
            _ => ExitCode::Open,
        }
    }
}

/// Net economics of one simulated round-trip.
#[derive(Clone, Copy, Debug)]
pub struct LegEconomics {
    pub pnl_sol: f64,
    pub pnl_percent: f64,
}

/// Frictionless PnL of a buy@`entry_price` / sell@`exit_price` round-trip sized
/// at `notional_sol` — pure price-to-price, no fees/slippage/latency. Kept for
/// unit tests and as the analytic baseline; the sweep itself prices trades with
/// [`round_trip_with_costs`] so the table reflects what the live trader pays.
#[cfg_attr(not(test), allow(dead_code))] // test + analytic baseline for the cost model
pub fn round_trip(entry_price: f64, exit_price: f64, notional_sol: f64) -> LegEconomics {
    if entry_price <= 0.0 || notional_sol <= 0.0 {
        return LegEconomics { pnl_sol: 0.0, pnl_percent: 0.0 };
    }
    let tokens = notional_sol / entry_price;
    let pnl_sol = tokens * exit_price - notional_sol;
    LegEconomics {
        pnl_sol,
        pnl_percent: pnl_sol / notional_sol * 100.0,
    }
}

/// Representative landed Jito tip per leg, in SOL. The live trader sizes the tip
/// per-trade from the auction floor, clamped to `[MIN_JITO_TIP_SOL,
/// MAX_JITO_TIP_SOL]` (see `pump_trader`); for the backtest a fixed mid-range
/// stand-in is enough to stop the sweep treating tips as free. Tune toward
/// `MAX_JITO_TIP_SOL` to model hotter auctions.
const SWEEP_REPRESENTATIVE_JITO_TIP_SOL: f64 = 0.001;

/// pump.fun's standard per-leg trading fee, in basis points (≈ 1%). The live
/// `min_out` math over-estimates this with `CURVE_FEE_BUFFER_BPS` (200) for a
/// safe lower bound; the *actual* fee charged is ≈ 100 bps, which is what a PnL
/// estimate should subtract.
const SWEEP_FEE_BPS_PER_LEG: f64 = 100.0;

/// Modeled per-leg execution slippage, in basis points — the curve/AMM impact of
/// the order itself, applied symmetrically (buy fills higher, sell fills lower).
/// Distinct from, and well under, the live 5% slippage *tolerance*
/// (`DEFAULT_SLIPPAGE_BPS`), which is a max-revert guard, not the expected fill.
const SWEEP_SLIPPAGE_BPS_PER_LEG: f64 = 100.0;

/// Priority fee for one leg, in SOL: `cu_price (µlamports/cu) × cu_limit`, scaled
/// from micro-lamports to SOL. Charged on every included tx (success or revert).
fn priority_fee_sol(cu_limit: u32) -> f64 {
    let lamports = COMPUTE_UNIT_PRICE_MICRO_LAMPORTS as f64 * cu_limit as f64 / 1_000_000.0;
    lamports / LAMPORTS_PER_SOL as f64
}

/// The execution-cost model the sweep prices every round-trip with, so backtest
/// PnL reflects the frictions the live trader actually pays rather than a
/// frictionless price-to-price delta. Resolved once per run from protocol
/// constants ([`CostModel::pumpfun_default`]) and threaded into every
/// [`Strategy::resolve_exit`], so backtest and live agree on what a trade costs.
///
/// All three knobs apply to **both** legs, keeping entry/exit symmetric — the old
/// model charged the entry an adverse worst-case fill but let the exit settle at
/// the frictionless decision price, which systematically inflated expectancy on
/// exactly the small, fast round-trips the strategy lives on.
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    /// Trading fee per leg, basis points (pump.fun curve ≈ 100 bps = 1%).
    pub fee_bps_per_leg: f64,
    /// Execution slippage per leg, basis points — worsens both fills symmetrically.
    pub slippage_bps: f64,
    /// Fixed SOL cost per leg (Jito tip + priority fee), charged on buy and sell.
    pub fixed_cost_sol_per_leg: f64,
}

impl CostModel {
    /// The default cost model, derived from the live `pump_trader` constants so the
    /// backtest and the real trade path agree on fees/tips. The fixed per-leg cost
    /// is a representative Jito tip plus the priority fee at the average curve
    /// buy/sell compute-unit limit.
    pub fn pumpfun_default() -> Self {
        let avg_priority_sol = (priority_fee_sol(COMPUTE_UNIT_LIMIT_CURVE_BUY)
            + priority_fee_sol(COMPUTE_UNIT_LIMIT_CURVE_SELL))
            / 2.0;
        Self {
            fee_bps_per_leg: SWEEP_FEE_BPS_PER_LEG,
            slippage_bps: SWEEP_SLIPPAGE_BPS_PER_LEG,
            fixed_cost_sol_per_leg: SWEEP_REPRESENTATIVE_JITO_TIP_SOL + avg_priority_sol,
        }
    }
}

/// PnL of a buy@`entry_price` / sell@`exit_price` round-trip sized at
/// `notional_sol`, **net of the [`CostModel`]**: symmetric slippage worsens both
/// fills, a trading fee is charged on each leg's SOL value, and a fixed per-leg
/// cost (tip + priority fee) is subtracted twice. This is the single cost gate —
/// no `Strategy` impl prices a trade any other way.
pub fn round_trip_with_costs(
    entry_price: f64,
    exit_price: f64,
    notional_sol: f64,
    costs: &CostModel,
) -> LegEconomics {
    if entry_price <= 0.0 || notional_sol <= 0.0 {
        return LegEconomics { pnl_sol: 0.0, pnl_percent: 0.0 };
    }
    let slip = costs.slippage_bps / 10_000.0;
    let fee = costs.fee_bps_per_leg / 10_000.0;
    // Adverse fills: pay up on the buy, receive less on the sell.
    let eff_entry = entry_price * (1.0 + slip);
    let eff_exit = exit_price * (1.0 - slip);
    let tokens = notional_sol / eff_entry;
    let gross_proceeds = tokens * eff_exit;
    // Trading fee on each leg's SOL value, plus the fixed per-leg cost twice.
    let costs_sol = (notional_sol + gross_proceeds) * fee + costs.fixed_cost_sol_per_leg * 2.0;
    let pnl_sol = gross_proceeds - notional_sol - costs_sol;
    LegEconomics {
        pnl_sol,
        pnl_percent: pnl_sol / notional_sol * 100.0,
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
    /// single-slot entry cache that only hits while consecutive combos share a key,
    /// so a full `Grid` (entry knobs are the high-order digits ⇒ already
    /// contiguous) resolves each entry once per tuple — but `Random`/`LatinHypercube`/
    /// `refine` hand out a shuffled order, collapsing the cache to ~one entry
    /// resolve per combo. A strategy whose entry is expensive overrides this to
    /// stable-sort by its entry key, restoring the contiguous-block property under
    /// every sampler. Called once on the shared combo vec before the per-group
    /// sweeps, so `combo_id` (= position) stays consistent across groups. The
    /// default is a no-op (param-free entries gain nothing). Decision-neutral: it
    /// only changes evaluation order, never any combo's resolved outcome.
    fn order_for_entry_cache(&self, _params: &mut [Self::Params]) {}
}

/// The pure black-box backtest, **factored into entry then exit** so the engine
/// can resolve the (expensive) entry **once per distinct entry-param tuple per
/// token** and reuse it across that tuple's whole exit sub-grid — see
/// [`crate::sweep::engine::run_sweep`]. A new strategy owns its own entry/exit
/// logic and just returns a [`TokenOutcome`]; the engine never inspects how.
///
/// The split is sound because the resolved entry depends **only** on the entry
/// params (the scalp gates), never on the exit ladder — so two combos with the
/// same [`Strategy::entry_key`] resolve byte-identical entries on a given token.
/// A strategy whose entry can't be separated (or is param-free) sets `EntryKey`
/// to a unit-like constant so the entry resolves once per token regardless.
pub trait Strategy: ParamSpace + Send + Sync {
    /// The resolved per-token entry under one entry-param tuple (price/time, or a
    /// "no entry" variant). Cached by the engine and passed into every
    /// [`Strategy::resolve_exit`] sharing the same [`Strategy::entry_key`].
    type Entry;

    /// The entry-param identity of a combo. Two combos with equal `EntryKey`
    /// resolve the same entry on any token, so the engine resolves the entry once
    /// per distinct key. `PartialEq` is all the engine needs (it keeps the last
    /// resolved `(key, entry)` and recomputes only when the key changes — which,
    /// on a grid, is once per contiguous exit-block).
    type EntryKey: PartialEq;

    /// Param-independent, **per-token** state the engine computes once before a
    /// token's combo loop and threads into every [`Strategy::resolve_entry`] on
    /// that token. Lets a strategy hoist work that depends only on the trade slice
    /// (tpsl2's launch-cohort wallet `HashSet`) out of the per-entry-key resolve,
    /// where it would otherwise rebuild once per distinct entry tuple. A strategy
    /// with no such state sets this to `()`.
    type TokenState;

    /// The entry-param signature of a combo. Combos sharing it share an entry.
    fn entry_key(&self, params: &Self::Params) -> Self::EntryKey;

    /// Compute the shared [`Strategy::TokenState`] for one token, **once** before
    /// any combo runs against it. Pure over the slim [`SweepTrade`] slice.
    fn prepare_token(&self, trades: &[SweepTrade]) -> Self::TokenState;

    /// Resolve the entry on one token's full trade history under a combo's **entry**
    /// params, given the pre-computed [`Strategy::TokenState`]. The expensive half
    /// (the scalp walk); called once per distinct [`Strategy::entry_key`] per token.
    /// The history is the slim, wallet-interned [`SweepTrade`] projection (not the
    /// heavy `Trade`); the shared entry fns run over it via the `TradeRow`
    /// abstraction. Pure — safe from many `rayon` threads.
    fn resolve_entry(
        &self,
        trades: &[SweepTrade],
        state: &Self::TokenState,
        params: &Self::Params,
    ) -> Self::Entry;

    /// Resolve the exit + economics given a pre-resolved `entry`, under a combo's
    /// **exit** params. Called once per combo. Also receives the shared
    /// [`Strategy::TokenState`] so an exit feature can reuse param-independent
    /// per-token precompute (tpsl2's E5 launch cohort) instead of rebuilding it per
    /// combo. Returns a `Copy` [`TokenOutcome`]; PnL is frictionless (see
    /// [`round_trip`]).
    fn resolve_exit(
        &self,
        trades: &[SweepTrade],
        state: &Self::TokenState,
        entry: &Self::Entry,
        params: &Self::Params,
    ) -> TokenOutcome;

    /// Flatten one param set to a JSON object stored with the combo's result row,
    /// so the UI can show/sort by any knob without a per-strategy schema.
    fn params_json(&self, params: &Self::Params) -> serde_json::Value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frictionless_round_trip_is_pure_price_ratio() {
        let e = round_trip(1.0, 2.0, 1.0);
        assert!((e.pnl_sol - 1.0).abs() < 1e-9);
        assert!((e.pnl_percent - 100.0).abs() < 1e-6);
    }

    #[test]
    fn flat_price_is_breakeven() {
        let e = round_trip(1.0, 1.0, 0.1);
        assert!(e.pnl_sol.abs() < 1e-9, "no frictions → flat price is breakeven");
    }

    #[test]
    fn costs_make_a_flat_round_trip_a_loss() {
        // With no price move the frictionless trip is breakeven, but fees + tip +
        // priority fee on both legs must turn it negative.
        let costs = CostModel::pumpfun_default();
        let e = round_trip_with_costs(1.0, 1.0, 0.1, &costs);
        assert!(e.pnl_sol < 0.0, "frictional flat round-trip must lose money");
    }

    #[test]
    fn costs_are_always_worse_than_frictionless() {
        // For the same prices, the cost model can never report more PnL than the
        // frictionless baseline — the whole point of finding #1.
        let costs = CostModel::pumpfun_default();
        for &(entry, exit) in &[(1.0, 2.0), (1.0, 0.5), (0.001, 0.0015)] {
            let net = round_trip_with_costs(entry, exit, 0.1, &costs);
            let gross = round_trip(entry, exit, 0.1);
            assert!(
                net.pnl_sol < gross.pnl_sol,
                "net ({}) must be below gross ({})",
                net.pnl_sol,
                gross.pnl_sol
            );
        }
    }

    #[test]
    fn costs_guard_degenerate_inputs() {
        let costs = CostModel::pumpfun_default();
        assert_eq!(round_trip_with_costs(0.0, 1.0, 0.1, &costs).pnl_sol, 0.0);
        assert_eq!(round_trip_with_costs(1.0, 1.0, 0.0, &costs).pnl_sol, 0.0);
    }

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

    #[test]
    fn from_reason_maps_known_and_unknown() {
        assert_eq!(ExitCode::from_reason("TakeProfit"), ExitCode::TakeProfit);
        assert_eq!(ExitCode::from_reason("StopLoss"), ExitCode::StopLoss);
        // Unknown reasons fall back to Open.
        assert_eq!(ExitCode::from_reason("???"), ExitCode::Open);
    }
}
