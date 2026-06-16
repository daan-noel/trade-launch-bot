//! The strategy surface and the shared fill/cost model.
//!
//! A new strategy implements exactly two traits — [`ParamSpace`] (how to sample
//! its param combos) and [`Strategy`] (the pure `simulate`). The sweep and
//! aggregate layers never know which concrete strategy ran: they only see
//! [`TokenOutcome`] rows.

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

/// Compact, `Copy` per-(combo, token) result. Holds no `String` so the hot loop
/// never allocates and the value stays register-friendly; the mint is recovered
/// from the corpus by token index at emit time. Exit reason is a small code, not
/// a string.
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

    /// Stable id stored alongside every combo (e.g. `"tpsl2"`).
    fn id(&self) -> &'static str;

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
    /// **exit** params. Called once per combo. Returns a `Copy` [`TokenOutcome`];
    /// PnL is frictionless (see [`round_trip`]).
    fn resolve_exit(
        &self,
        trades: &[SweepTrade],
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
    fn from_reason_maps_known_and_unknown() {
        assert_eq!(ExitCode::from_reason("TakeProfit"), ExitCode::TakeProfit);
        assert_eq!(ExitCode::from_reason("StopLoss"), ExitCode::StopLoss);
        // Unknown reasons fall back to Open.
        assert_eq!(ExitCode::from_reason("???"), ExitCode::Open);
    }
}
