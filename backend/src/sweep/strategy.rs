//! The strategy surface and the shared fill/cost model.
//!
//! A new strategy implements exactly two traits — [`ParamSpace`] (how to sample
//! its param combos) and [`Strategy`] (the pure `simulate`). The sweep and
//! aggregate layers never know which concrete strategy ran: they only see
//! [`TokenOutcome`] rows.

use crate::models::trade::Trade;

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
/// at `notional_sol` — pure price-to-price, no fees/slippage/latency. A future
/// cost layer would slot in here without touching any `Strategy` impl.
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

/// The pure black-box backtest. The *entire* second half of the surface a new
/// strategy adds — it owns its own entry/exit logic and just returns a
/// [`TokenOutcome`]; the engine never inspects how.
pub trait Strategy: ParamSpace + Send + Sync {
    /// Stable id stored alongside every combo (e.g. `"tpsl2"`).
    fn id(&self) -> &'static str;

    /// Simulate this strategy on one token's full trade history under one param
    /// set. Pure: no IO, no shared mutation, returns a `Copy` value — safe to call
    /// from many `rayon` threads. PnL is frictionless (see [`round_trip`]).
    fn simulate(&self, trades: &[Trade], params: &Self::Params) -> TokenOutcome;

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
    fn from_reason_maps_known_and_unknown() {
        assert_eq!(ExitCode::from_reason("TakeProfit"), ExitCode::TakeProfit);
        assert_eq!(ExitCode::from_reason("StopLoss"), ExitCode::StopLoss);
        // Unknown reasons fall back to Open.
        assert_eq!(ExitCode::from_reason("???"), ExitCode::Open);
    }
}
