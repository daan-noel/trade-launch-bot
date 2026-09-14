//! Simulation **kernel** — the shared metric-aggregation primitives that turn a
//! stream of per-token [`TokenOutcome`]s into one rolled-up [`RunMetrics`] row.
//! The same primitives back every replay path (`lab`'s param sweep, live/paper
//! run rollups), so live / paper / sweep results stay comparable.
//!
//! PnL is priced through the shared [`CostModel`] ([`round_trip_with_costs`] /
//! [`round_trip_multi_leg`]) so a backtest reflects the frictions the live trader
//! pays — including scale-out's per-leg fixed cost. The bounded `QuantileSketch`
//! + streaming [`RunAgg`] are the single home for the sketch / robust-score math:
//! `lab`'s per-combo sweep folds into [`RunAgg`] via its thin `ComboAgg` wrapper,
//! so backtest and live/paper metrics can never drift to a second copy.

use serde::{Deserialize, Serialize};

use crate::config::fee_tuning::close_account_fee_sol;
use crate::config::FeeTuning;

// ── Per-token outcome ─────────────────────────────────────────────────────────

/// Compact exit-reason code: the strategy ladder reasons plus the two non-exit
/// terminals (`Open`, `NoEntry`) the aggregation distinguishes.
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
    /// Analysis-only death-close: the ladder never fired but the token is provably
    /// dead (liquidity gone + gone silent), so the sim closes the bag at the last
    /// meaningful trade instead of leaving it `Open` at a stale price. Live never
    /// produces this (it closes silent tokens via its clock sweep). Counts as a
    /// **closed** loss in the rollup. See [`crate::strategies::death`].
    Dead = 8,
    /// The generic engine's metric-condition exit (`ExitReason::Metrics`): any of
    /// a rule's exit metric conditions became true. Rollups still bucket every
    /// detail label (`stall > 3`, legacy `stall>` / bare `Metrics`) here — the
    /// per-metric detail lives on the persisted string, not on this code.
    Metrics = 9,
    /// Closed by an operator, not by the rule: a Console "Sell ALL", the per-rule
    /// Stop, or Stop All (`ExitReason::Manual`). Also the fallback bucket for a
    /// closed row whose label is missing/unrecognized — see
    /// [`ExitCode::from_closed_reason`]. Live-only; analysis never produces it.
    Manual = 10,
    /// The token graduated off the bonding curve and the bag was closed
    /// (`ExitReason::Migrated`). Live-only.
    Migrated = 11,
}

impl ExitCode {
    /// Map a persisted exit-reason label to a code. Metric detail forms
    /// (`stall > 3`, …) and legacy `"Metrics"` both map to [`ExitCode::Metrics`].
    pub fn from_reason(reason: &str) -> Self {
        match reason {
            "TakeProfit" => ExitCode::TakeProfit,
            "StopLoss" => ExitCode::StopLoss,
            "TrailingStop" => ExitCode::TrailingStop,
            "Stall" => ExitCode::Stall,
            "TimeStop" => ExitCode::TimeStop,
            "LiquidityExit" => ExitCode::LiquidityExit,
            "Dead" => ExitCode::Dead,
            "Manual" => ExitCode::Manual,
            "Migrated" => ExitCode::Migrated,
            "Open" => ExitCode::Open,
            // Matched fingerprint / armed but never filled — distinct from still-Open.
            "NoEntry" => ExitCode::NoEntry,
            r if hunter_engine::event::is_metric_exit_label(r) => ExitCode::Metrics,
            _ => ExitCode::Open,
        }
    }

    /// The bucket for a row already **known** to be closed (a terminal `End`),
    /// from its persisted label. Unlike [`from_reason`](Self::from_reason) this
    /// never answers `Open`/`NoEntry`: an unknown or absent label falls back to
    /// [`ExitCode::Manual`], because [`RunAgg::record`] splits realized from
    /// unrealized on `== Open`, so one mislabeled row would drop its realized PnL
    /// out of `total_pnl_sol`, the win rate, and every holding-time stat while
    /// inflating `n_open`.
    pub fn from_closed_reason(reason: Option<&str>) -> Self {
        match reason.map(Self::from_reason) {
            None | Some(ExitCode::Open) | Some(ExitCode::NoEntry) => ExitCode::Manual,
            Some(code) => code,
        }
    }
}

/// The simulated result of running one strategy over one token's trade history.
#[derive(Clone, Copy, Debug)]
pub struct TokenOutcome {
    /// Whether the strategy took a position under these params.
    pub fired: bool,
    /// Seconds entry→exit (0 when not fired or still open).
    pub holding_secs: i64,
    /// Net round-trip PnL after costs, as % of notional.
    pub pnl_percent: f32,
    /// Net round-trip PnL after costs, in SOL.
    pub pnl_sol: f32,
    pub exit: ExitCode,
}

impl TokenOutcome {
    /// The strategy never entered this token under these params.
    pub fn no_entry() -> Self {
        Self { fired: false, holding_secs: 0, pnl_percent: 0.0, pnl_sol: 0.0, exit: ExitCode::NoEntry }
    }
}

// ── Cost model (the ONE copy; the lab sweep re-exports it) ────────────────────

/// pump.fun's protocol fee, **measured, not assumed** (2026-07-28).
///
/// Dev-buy amounts cluster hard on `gross × 0.987654321` = `gross × 10000/10125`,
/// which is the exact factor a 125 bps fee produces when the recorded
/// `amount_lamports` is the *curve-side* amount: 16,544 of 56,908 dev buys land on
/// that ratio against a round 0.1 SOL, versus 310 on the `0.990099` a 100 bps fee
/// would give. (That `amount_lamports` excludes the fee is itself measured:
/// `|Δreserve_lamports| / amount_lamports` = 1.00000 at p25/median/p75 over 5.6M
/// legs — the ingest never decodes the `fee` IDL fields.)
///
/// This was `100.0` until 2026-07-28, i.e. **0.5 pp per round trip too cheap**, so
/// every backtest run before that date is optimistic by that much. The constant is
/// not persisted per run, so re-run anything whose margin was inside 0.5 pp.
const FEE_BPS_PER_LEG: f64 = 125.0;
/// Execution-cost model the kernel prices every round-trip with. Priced through
/// [`buy_fill`] / [`sell_proceeds`], a round trip reproduces what the wallet
/// moves: on the 34 real round trips of 2026-09-13/14, fed the pool state each
/// landed in, it matches the on-chain balance changes to the lamport.
///
/// Fixed per-leg costs (base fee + priority + tip) come from process-wide
/// [`FeeTuning`] — the same `JITO_MIN_TIP_SOL` / `CU_PRICE_MICRO_LAMPORTS` live
/// applies to the trader. Install via [`FeeTuning::install`] after `dotenvy` in
/// each bin.
///
/// `Serialize` because the frontend's live mark tip has to net a price change
/// between holdings polls, and the ONLY honest way for it to do that is to be
/// handed these numbers (`GET /api/meta/cost-model`) rather than to carry its own
/// copy of a fee that lives in this file and in `.env`.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CostModel {
    /// The venue fee, in bps, on each leg. A buy (`buy_exact_sol_in`) pays it on
    /// top of the SOL that reaches the curve; a sell pays it out of the curve's
    /// output.
    pub fee_bps_per_leg: f64,
    /// SOL a buy transaction costs beyond the order: base signature fee + priority
    /// fee + tip ([`FeeTuning::fixed_buy_sol`]).
    pub fixed_buy_sol: f64,
    /// SOL a sell transaction costs out of what it returns
    /// ([`FeeTuning::fixed_sell_sol`]).
    pub fixed_sell_sol: f64,
    /// The rent-reclaim close transaction a finished position sends, charged once
    /// on the leg that empties the bag ([`close_account_fee_sol`]).
    pub close_fee_sol: f64,
    /// Charge **our own** constant-product price impact from the priced (virtual)
    /// SOL depth each leg lands in: a buy of curve SOL `c` into `V` fills at
    /// `spot × (1 + c/V)`, a sell worth `g` at spot returns `g / (1 + g/V)`. The
    /// exact curve, not a linear haircut.
    ///
    /// Orthogonal to the fill model: a
    /// [`FillModel`](crate::strategies::paper_fill::FillModel) chooses **which
    /// market print we transact against**, whereas impact is **how far our own
    /// order moves the curve**. Both are real and a live trade pays both, so the
    /// two compose without double-counting.
    ///
    /// There is deliberately no flat per-leg slippage knob beside this. One existed
    /// (`slippage_bps`, 100 bps) as a stand-in for this same quantity, which meant it
    /// double-counted against any fill model — and being size-blind, its error changed
    /// sign with buy size (harsher than reality at 0.1 SOL, kinder at 1.0), so it
    /// reordered a grid rather than shifting it. Impact replaces it outright.
    pub price_impact: bool,
}

impl CostModel {
    /// **The model.** Fee + fixed per-leg cost + **real** constant-product price
    /// impact on each leg's own depth, and no flat `slippage_bps`.
    ///
    /// This is the honest pairing with an explicit
    /// [`FillModel`](crate::strategies::paper_fill::FillModel): the fill model
    /// prices *which market print we transact against*, this prices *how far our
    /// own order moves the curve*, and nothing is counted twice. It is also the
    /// only constructor whose cost responds to buy size, which is what makes a
    /// sizing decision measurable rather than assumed.
    ///
    /// Measured on the 2026-07 corpus (median depth ~70 SOL): a 0.1 SOL buy costs
    /// 0.14%/leg and a 1.0 SOL buy 1.42%/leg — against the flat 1.00% the retired
    /// slippage model guessed for both. See
    /// `docs/plans/strategies/execution-costs.md`.
    pub fn pumpfun_with_impact() -> Self {
        Self::pumpfun_with_impact_with(&FeeTuning::current())
    }

    /// Like [`pumpfun_with_impact`](Self::pumpfun_with_impact) but with an explicit
    /// [`FeeTuning`] (tests / one-off repricing without touching process state).
    pub fn pumpfun_with_impact_with(tuning: &FeeTuning) -> Self {
        Self {
            fee_bps_per_leg: FEE_BPS_PER_LEG,
            fixed_buy_sol: tuning.fixed_buy_sol(),
            fixed_sell_sol: tuning.fixed_sell_sol(),
            close_fee_sol: close_account_fee_sol(),
            price_impact: true,
        }
    }

    /// Fee + Jito tip + priority only — **no** size term at all. A deliberate
    /// zero-impact **upper bound**: use it to ask "is there any edge here before
    /// sizing costs?", never to price a run you intend to believe. It is 0.34 pp
    /// too generous on a 0.1 SOL buy and 3.3 pp too generous on a 1.0 SOL buy,
    /// into the measured median 70 SOL pool.
    ///
    /// This is also what [`pumpfun_with_impact`](Self::pumpfun_with_impact)
    /// silently degrades to when the caller supplies no pool depth — see
    /// [`CostModelKind::PumpfunImpact`].
    pub fn pumpfun_fee_only() -> Self {
        Self::pumpfun_fee_only_with(&FeeTuning::current())
    }

    /// Fee-only variant of
    /// [`pumpfun_with_impact_with`](Self::pumpfun_with_impact_with).
    pub fn pumpfun_fee_only_with(tuning: &FeeTuning) -> Self {
        Self { price_impact: false, ..Self::pumpfun_with_impact_with(tuning) }
    }

    /// The venue's own charges only — its fee and the constant-product impact —
    /// with no transaction cost: what a bag sells for whoever sends the sell. Marks
    /// another wallet's bag, whose priority fee and tip are that wallet's choice.
    pub fn venue_only() -> Self {
        Self {
            fixed_buy_sol: 0.0,
            fixed_sell_sol: 0.0,
            close_fee_sol: 0.0,
            ..Self::pumpfun_with_impact()
        }
    }

    /// A frictionless model (no fees/slippage/fixed cost) — pure price-to-price,
    /// for analytic baselines and tests.
    pub fn frictionless() -> Self {
        Self {
            fee_bps_per_leg: 0.0,
            fixed_buy_sol: 0.0,
            fixed_sell_sol: 0.0,
            close_fee_sol: 0.0,
            price_impact: false,
        }
    }

    /// The capital a buy of `notional_sol` takes from the wallet — the order plus
    /// its transaction's fixed cost. The ONE denominator a PnL percent divides by,
    /// so a percent summed over `n` trades divides by `n × capital_sol(notional)`,
    /// never `n × notional`.
    pub fn capital_sol(&self, notional_sol: f64) -> f64 {
        notional_sol + self.fixed_buy_sol
    }

    /// This model for a leg on a pool whose own fee is `venue_fee_bps`: a PumpSwap
    /// pool charges its market-cap tier (read off its swap events), not the curve's
    /// 125 bps. PumpSwap's swap math has the curve's shape — constant product, fee
    /// on top of a buy's pool amount and out of a sell's — so the fee is the only
    /// term that changes. `None` (a curve leg, or a pool no swap has shown yet)
    /// keeps the model as it is.
    pub fn at_venue_fee(self, venue_fee_bps: Option<f64>) -> Self {
        match venue_fee_bps.filter(|f| f.is_finite() && *f >= 0.0) {
            Some(fee_bps_per_leg) => Self { fee_bps_per_leg, ..self },
            None => self,
        }
    }
}

/// Wire-selectable [`CostModel`] — the cost half of a run's **identity** (the fill
/// model is the other half). Two runs priced under different kinds are not
/// comparable, so a request that carries this must persist and display it.
///
/// An omitted value ⇒ [`PumpfunImpact`](Self::PumpfunImpact), the only kind that
/// charges our own size. A **present but unrecognized** value is a hard error, not a
/// fallback: silently substituting a model would report a run as priced under
/// something it was not, and the whole reason a run stores its cost model is that two
/// runs priced differently are not comparable.
///
/// Serde: canonical `snake_case` names, plus short aliases matching the
/// fill-sensitivity analysis doc's column labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostModelKind {
    /// [`CostModel::pumpfun_with_impact`] — fee + fixed + **real** constant-product
    /// impact from pool depth. The default, and the only kind whose cost varies
    /// with `buy_amount_sol`.
    ///
    /// Requires the caller to supply depth to [`round_trip_with_costs`] — without
    /// it no impact is charged and this degrades to [`PumpfunFeeOnly`](Self::PumpfunFeeOnly), silently
    /// and by design (a guessed depth would be a fabricated number).
    ///
    #[default]
    #[serde(alias = "impact")]
    PumpfunImpact,
    /// [`CostModel::pumpfun_fee_only`] — no size term; a zero-impact upper bound.
    #[serde(alias = "fee_only")]
    PumpfunFeeOnly,
}

impl CostModelKind {
    /// The [`CostModel`] this kind selects under process [`FeeTuning::current`].
    pub fn model(self) -> CostModel {
        self.model_with(&FeeTuning::current())
    }

    /// Like [`model`](Self::model) with an explicit [`FeeTuning`].
    pub fn model_with(self, tuning: &FeeTuning) -> CostModel {
        match self {
            CostModelKind::PumpfunImpact => CostModel::pumpfun_with_impact_with(tuning),
            CostModelKind::PumpfunFeeOnly => CostModel::pumpfun_fee_only_with(tuning),
        }
    }
}

/// One exit leg of a (possibly tranched) round-trip. `sell_bps` is of the
/// **initial** bag (same grain as scale-out stages); fractions compose without
/// compounding. Cap is 10_000 (= 100%); a single full close is one leg at 10_000.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitLeg {
    pub sell_bps: u16,
    pub price: f64,
    /// Priced (virtual) SOL depth this leg sells into, for
    /// [`CostModel::price_impact`] — the leg's OWN depth, not the entry's. `None`
    /// charges no impact on this leg.
    pub reserve_sol: Option<f64>,
}

/// The depth impact is charged against, or `None` for no impact. `filter` (not a
/// bare `unwrap_or`) so a zero / negative / NaN depth cannot divide by ~0.
fn impact_depth(costs: &CostModel, reserve_sol: Option<f64>) -> Option<f64> {
    if !costs.price_impact {
        return None;
    }
    reserve_sol.filter(|r| r.is_finite() && *r > 0.0)
}

/// A buy of `notional_sol` into a pool at spot `price` (SOL per raw token) with
/// priced SOL depth `reserve_sol`: returns `(tokens, paid_sol)` — the tokens it
/// receives and the SOL it takes from the wallet.
///
/// `buy_exact_sol_in` spends the notional, fee included: `c = B / (1 + fee)`
/// reaches the curve, which returns `vtok·c / (vsol + c)` tokens =
/// `c / (price × (1 + c/vsol))`. The transaction's fixed cost is paid on top, so
/// `paid = B + fixed_buy` — [`CostModel::capital_sol`].
pub fn buy_fill(notional_sol: f64, price: f64, reserve_sol: Option<f64>, costs: &CostModel) -> (f64, f64) {
    if !(notional_sol > 0.0) || !(price > 0.0) || !price.is_finite() {
        return (0.0, 0.0);
    }
    let fee = costs.fee_bps_per_leg / 10_000.0;
    let curve_sol = notional_sol / (1.0 + fee);
    let tokens = match impact_depth(costs, reserve_sol) {
        Some(v) => curve_sol / (price * (1.0 + curve_sol / v)),
        None => curve_sol / price,
    };
    (tokens, costs.capital_sol(notional_sol))
}

/// SOL a sell of `tokens` into a pool at spot `price` with priced SOL depth
/// `reserve_sol` returns to the wallet. [`sell_value_proceeds`] on the bag's value
/// at spot.
pub fn sell_proceeds(
    tokens: f64,
    price: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
    empties_bag: bool,
) -> f64 {
    sell_value_proceeds(tokens * price, reserve_sol, costs, empties_bag)
}

/// SOL a sell of a bag worth `value_sol` at spot returns to the wallet: the curve
/// pays `g / (1 + g/vsol)`, the venue keeps its fee out of that, and the
/// transaction's fixed cost comes off the rest. The leg that `empties_bag` also
/// pays the rent-reclaim close transaction. A worthless bag still pays its legs.
///
/// The TS mirror is `netProceedsSol` in `frontend/src/shared/lib/liveMark.ts`;
/// `sell_value_proceeds_golden_vectors` pins both to the same literals.
pub fn sell_value_proceeds(
    value_sol: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
    empties_bag: bool,
) -> f64 {
    let fee = costs.fee_bps_per_leg / 10_000.0;
    let value = if value_sol.is_finite() { value_sol.max(0.0) } else { 0.0 };
    let out = match impact_depth(costs, reserve_sol) {
        Some(v) => value / (1.0 + value / v),
        None => value,
    };
    let close = if empties_bag { costs.close_fee_sol } else { 0.0 };
    out * (1.0 - fee) - costs.fixed_sell_sol - close
}

/// Net PnL of a buy@`entry_price` / sell@`exit_price` round-trip sized at
/// `notional_sol`, net of `costs`. Thin wrapper over [`round_trip_multi_leg`]
/// with a single full-bag exit.
///
/// `entry_reserve_sol` / `exit_reserve_sol` are the priced SOL depths the buy and
/// the sell land in. Pass `None` when unknown (no impact charged on that leg).
pub fn round_trip_with_costs(
    entry_price: f64,
    exit_price: f64,
    notional_sol: f64,
    entry_reserve_sol: Option<f64>,
    exit_reserve_sol: Option<f64>,
    costs: &CostModel,
) -> (f64, f64) {
    round_trip_multi_leg(
        entry_price,
        notional_sol,
        entry_reserve_sol,
        &[ExitLeg { sell_bps: 10_000, price: exit_price, reserve_sol: exit_reserve_sol }],
        costs,
    )
}

/// Multi-leg sibling of [`round_trip_with_costs`]: one [`buy_fill`] + `exits`
/// [`sell_proceeds`] legs, each on its own price and depth. Every sell leg pays
/// its own fixed cost, so fixed cost scales with leg count — the real economic
/// bound on scale-out stage count — and the last leg pays the close.
///
/// `exits` must be non-empty and `sum(sell_bps)` should cover the bag being
/// priced (10_000 for a full close; less + a mark leg for mid-ladder open MTM).
/// Returns `(pnl_sol, pnl_percent)`, the percent over [`CostModel::capital_sol`].
/// Empty / invalid inputs → `(0, 0)`.
pub fn round_trip_multi_leg(
    entry_price: f64,
    notional_sol: f64,
    entry_reserve_sol: Option<f64>,
    exits: &[ExitLeg],
    costs: &CostModel,
) -> (f64, f64) {
    if entry_price <= 0.0 || notional_sol <= 0.0 {
        return (0.0, 0.0);
    }
    let Some(last) = exits.iter().rposition(|l| l.sell_bps > 0) else {
        return (0.0, 0.0);
    };
    let (tokens, paid) = buy_fill(notional_sol, entry_price, entry_reserve_sol, costs);
    let got: f64 = exits
        .iter()
        .enumerate()
        .filter(|(_, l)| l.sell_bps > 0)
        .map(|(i, l)| {
            let leg_tokens = tokens * f64::from(l.sell_bps) / 10_000.0;
            sell_proceeds(leg_tokens, l.price, l.reserve_sol, costs, i == last)
        })
        .sum();
    let pnl_sol = got - paid;
    (pnl_sol, pnl_sol / paid * 100.0)
}

/// Net PnL of an **already-filled** bag: what selling `held_amount` tokens at
/// `mark_price` into `reserve_sol` right now would return, minus the
/// `cost_basis_sol` that bag took from the wallet. The sell empties the bag, so it
/// pays the close.
///
/// It is not a round trip with one price swapped: the entry has executed, so its
/// cost is a fact, not a model — pass what the fill actually paid (the position's
/// wallet-exact `entry_sol`, pro-rata to the bag still held). [`modeled_cost_basis`]
/// is the fallback for a bag whose only record is a curve-side price.
pub fn mark_open_bag(
    cost_basis_sol: f64,
    mark_price: f64,
    held_amount: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
) -> f64 {
    if !cost_basis_sol.is_finite()
        || !held_amount.is_finite()
        || held_amount <= 0.0
        || !mark_price.is_finite()
        || mark_price < 0.0
    {
        return 0.0;
    }
    sell_proceeds(held_amount, mark_price, reserve_sol, costs, true) - cost_basis_sol
}

/// What a curve buy that filled `held_amount` tokens at the curve-side execution
/// price `entry_price` took from the wallet: `c × (1 + fee) + fixed_buy`, where
/// `c = entry_price × held_amount` is the SOL that reached the curve. Exact for a
/// bot curve buy at the floor tip — the inverse of [`buy_fill`] — and the basis for
/// a bag with no wallet-flow record.
pub fn modeled_cost_basis(entry_price: f64, held_amount: f64, costs: &CostModel) -> f64 {
    if !(entry_price > 0.0) || !(held_amount > 0.0) {
        return 0.0;
    }
    let fee = costs.fee_bps_per_leg / 10_000.0;
    entry_price * held_amount * (1.0 + fee) + costs.fixed_buy_sol
}

/// Round a PnL figure through `f32` precision and back. The sweep's
/// [`TokenOutcome`] stores `pnl_sol`/`pnl_percent` as `f32` (register-friendly,
/// no per-outcome allocation across millions of `(combo × token)` rows); a
/// single-rule simulate keeps `f64` end-to-end. Left unrounded, the two paths'
/// headline numbers drift by float noise even when every decision and cost input
/// is identical. Simulate calls this on both `round_trip_with_costs` outputs
/// before display/summation so it quantizes exactly like the sweep does.
pub fn quantize_f32(x: f64) -> f64 {
    x as f32 as f64
}

/// **Canonical "return %"** — the single definition of realized return shared by
/// the live rules table, the lab rules table, the positions-summary panel, and the
/// sweep. Capital-weighted: net PnL as a percent of the total SOL *deployed* across
/// the closed positions, i.e. `Σ pnl_sol / Σ entry_sol × 100`.
///
/// Because the denominator is total capital (always ≥ 0), the sign of this figure
/// **can never disagree** with the sign of the summed SOL PnL — the two headline
/// columns move together by construction. This replaces the old
/// `mean(per-trade price %)`, which mixed an equal-weighted mean of size-independent
/// price ratios with a size-weighted SOL sum and so could show `+%`/`−◎` (or the
/// reverse) on the same rule. Under a fixed per-trade notional (the sweep) it
/// reduces exactly to the mean of per-trade percents, so backtest numbers are
/// unchanged. Returns `0.0` when no capital was deployed.
pub fn weighted_return_pct(sum_pnl_sol: f64, sum_capital_sol: f64) -> f64 {
    if sum_capital_sol > 0.0 {
        sum_pnl_sol / sum_capital_sol * 100.0
    } else {
        0.0
    }
}

// ── Run metrics ────────────────────────────────────────────────────────────────

/// Rolled-up metrics for one run across a token corpus. Field-for-field the
/// `strategy_run_metrics` columns (plus the sweep's `score`, ignored when
/// persisting a live/paper run).
///
/// **Also the wire shape.** Serialized straight to the frontend by every surface
/// that reports a run's outcome — single-rule simulate, grouped sweep, and a
/// live/paper run — so all three send the same field names and the UI can render
/// them through one component instead of three ad-hoc shapes (parity plan B4).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunMetrics {
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    /// Realized-only (`wins / n_closed`) — a still-`Open` mark is not a win/loss yet.
    pub win_rate: f64,
    /// Realized-only: the sum of closed positions' PnL, never a still-`Open` mark.
    pub total_pnl_sol: f64,
    /// **Unrealized** counterpart to `total_pnl_sol`: the sum of still-`Open`
    /// positions' mark-to-last-price PnL. Reported alongside the realized total
    /// (never folded into it) so a run whose losers are all still open can't read
    /// as profitable — `total_pnl_sol + open_pnl_sol` is the mark-to-market total.
    /// Every other field on this struct stays realized-only (parity plan C2).
    pub open_pnl_sol: f64,
    pub expectancy_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    pub std_pnl_pct: f64,
    pub profit_factor: Option<f64>,
    /// Mean per-trade pnl% over **all fired** positions (still-open marks
    /// included). The profitability term in [`checklist_score`].
    pub mtm_pnl_pct: f64,
    /// Checklist rank (see [`checklist_score`]): MTM% × fire-rate × open-drag
    /// × win-rate. `None` when nothing fired. Grouped sweep rewrites this with
    /// the group's matched-token count after finalize.
    pub score: Option<f64>,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    pub n_exit_take_profit: u32,
    pub n_exit_stop_loss: u32,
    pub n_exit_trailing: u32,
    pub n_exit_stall: u32,
    pub n_exit_time: u32,
    pub n_exit_liquidity: u32,
    /// Analysis-only death-closes (`ExitCode::Dead`): positions closed at the last
    /// meaningful trade because the token died silent. 0 in live rollups. Counts as
    /// closed (loss), so it lifts `n_closed` and lowers `n_open`.
    pub n_exit_dead: u32,
    /// Generic-engine metric-condition exits (`ExitCode::Metrics`). 0 for the
    /// legacy strategies (which use the granular ladder codes above).
    /// Equals `n_exit_metrics_win + n_exit_metrics_loss`.
    pub n_exit_metrics: u32,
    /// Metric exits with positive realized SOL. `#[serde(default)]` so older
    /// sweep rows that only stored the total still deserialize.
    #[serde(default)]
    pub n_exit_metrics_win: u32,
    /// Metric exits that are not wins (loss or break-even).
    #[serde(default)]
    pub n_exit_metrics_loss: u32,
    /// Operator-forced closes (`ExitCode::Manual`) — Console sell, per-rule Stop,
    /// Stop All, plus any closed row with an unrecognized label. 0 in a sweep /
    /// simulate rollup (analysis has no operator). `#[serde(default)]` so rows
    /// stored before this field existed still deserialize.
    #[serde(default)]
    pub n_exit_manual: u32,
    /// Closes taken because the token graduated off the curve
    /// (`ExitCode::Migrated`). 0 in a sweep / simulate rollup.
    #[serde(default)]
    pub n_exit_migrated: u32,
    pub n_exit_open: u32,
}

// ── Streaming aggregate (ported from lab sweep::aggregate) ─────────────────────

/// Floor under win-rate in [`checklist_score`] so an all-open book (WR = 0)
/// still gets a tiny multiplier instead of zeroing the whole rank.
const SCORE_WIN_RATE_FLOOR: f64 = 0.01;
/// Weight on open-share in [`checklist_score`]: `× (1 − w · n_open/n_fired)`.
const SCORE_OPEN_DRAG: f64 = 0.5;

/// Streaming accumulator across every token a run fires on. Every PnL/holding/
/// win-rate stat is **realized-only** (closed positions — includes the
/// analysis-only death-close, excludes a still-`Open` mark-to-last-price): an
/// unrealized mark isn't a trade outcome yet, so folding it into "win rate" or
/// "total PnL" mixed marks-to-market with realized returns and made a sweep's
/// headline numbers depend on exactly when the corpus window happened to end
/// (parity plan C2). `n_fired`/`n_open` still count every position taken,
/// `Open` still included, so the UI can show "X open" alongside the realized
/// figures. O(1) per run — interior quantiles via a fixed [`QuantileSketch`].
///
/// Public so the analysis path can fold into the **same** accumulator the live /
/// paper kernel uses: `lab`'s per-combo sweep wraps one of these per combo (its
/// `ComboAgg`) so backtest metrics are byte-identical to a live run's, with no
/// second copy of the sketch / score math to drift.
#[derive(Clone)]
pub struct RunAgg {
    fired: u64,
    open: u64,
    wins: u64,
    pnl_sol_sum: f64,
    /// Unrealized mark-to-last-price sum over the still-`Open` positions. Kept
    /// strictly apart from `pnl_sol_sum` so no realized figure can absorb it.
    open_pnl_sol_sum: f64,
    gross_win_sol: f64,
    gross_loss_sol: f64,
    pnl_min: f32,
    pnl_max: f32,
    pnl_sketch: QuantileSketch,
    closed_pct_sum: f64,
    closed_pct_sum_sq: f64,
    /// Σ pnl% over **all fired** (open marks included) — feeds `mtm_pnl_pct`.
    fired_pct_sum: f64,
    holding_sum: i64,
    holding_sketch: QuantileSketch,
    exit_counts: [u32; N_EXIT_BUCKETS],
    /// `ExitCode::Metrics` with `pnl_sol > 0`.
    metrics_win: u32,
    /// `ExitCode::Metrics` that are not wins (`pnl_sol <= 0`).
    metrics_loss: u32,
}

impl Default for RunAgg {
    fn default() -> Self {
        Self {
            fired: 0,
            open: 0,
            wins: 0,
            pnl_sol_sum: 0.0,
            open_pnl_sol_sum: 0.0,
            gross_win_sol: 0.0,
            gross_loss_sol: 0.0,
            pnl_min: f32::INFINITY,
            pnl_max: f32::NEG_INFINITY,
            pnl_sketch: QuantileSketch::default(),
            closed_pct_sum: 0.0,
            closed_pct_sum_sq: 0.0,
            fired_pct_sum: 0.0,
            holding_sum: 0,
            holding_sketch: QuantileSketch::default(),
            exit_counts: [0; N_EXIT_BUCKETS],
            metrics_win: 0,
            metrics_loss: 0,
        }
    }
}

impl RunAgg {
    /// Fold one token's outcome into the accumulator. No-entry rows are ignored.
    /// A still-`Open` outcome counts toward `n_fired`/`n_open`/its exit-count
    /// slot, and its mark-to-last-price PnL accumulates into the separate
    /// `open_pnl_sol_sum` — because it is unrealized it never touches the
    /// realized PnL sum, win/loss counters, quantile sketch, or holding-time
    /// stats (parity plan C2).
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        let p = o.pnl_percent as f64;
        self.fired_pct_sum += p;
        if o.exit == ExitCode::Open {
            self.open += 1;
            self.open_pnl_sol_sum += o.pnl_sol as f64;
        } else {
            self.pnl_sol_sum += o.pnl_sol as f64;
            self.pnl_min = self.pnl_min.min(o.pnl_percent);
            self.pnl_max = self.pnl_max.max(o.pnl_percent);
            self.pnl_sketch.record(p);
            if o.pnl_sol > 0.0 {
                self.wins += 1;
                self.gross_win_sol += o.pnl_sol as f64;
            } else if o.pnl_sol < 0.0 {
                self.gross_loss_sol += -(o.pnl_sol as f64);
            }
            self.holding_sum += o.holding_secs;
            self.holding_sketch.record(o.holding_secs as f64);
            self.closed_pct_sum += p;
            self.closed_pct_sum_sq += p * p;
            if o.exit == ExitCode::Metrics {
                if o.pnl_sol > 0.0 {
                    self.metrics_win += 1;
                } else {
                    self.metrics_loss += 1;
                }
            }
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse the accumulator to the final rolled-up [`RunMetrics`]. Every
    /// PnL/win-rate/holding figure is realized-only (denominator `n_closed`,
    /// never `n_fired`) — see [`RunAgg`]'s doc. Score uses MTM% (opens included)
    /// with `matched = n_fired` (fire-rate = 1); grouped sweep rewrites score
    /// with the group's token count via [`checklist_score`].
    pub fn finalize(self) -> RunMetrics {
        let n_closed = self.fired - self.open;
        let n = n_closed as f64;
        let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if n_closed == 0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            (
                self.pnl_sketch.quantile(0.5),
                self.pnl_sketch.quantile(0.9),
                self.pnl_max as f64,
                self.pnl_min as f64,
            )
        };
        let mean_pnl_pct = if n_closed == 0 { 0.0 } else { self.closed_pct_sum / n };
        let mtm_pnl_pct = if self.fired == 0 {
            0.0
        } else {
            self.fired_pct_sum / self.fired as f64
        };
        let (avg_holding_secs, median_holding_secs) = if n_closed == 0 {
            (0.0, 0.0)
        } else {
            (self.holding_sum as f64 / n, self.holding_sketch.quantile(0.5))
        };
        let profit_factor = if self.gross_loss_sol > 0.0 {
            Some(self.gross_win_sol / self.gross_loss_sol)
        } else {
            None
        };
        let expectancy_sol = if n_closed == 0 { 0.0 } else { self.pnl_sol_sum / n };
        let std_pnl_pct = sample_std_pct(n_closed, self.closed_pct_sum, self.closed_pct_sum_sq);
        let win_rate = if n_closed == 0 { 0.0 } else { self.wins as f64 / n };
        let score = checklist_score(self.fired, self.open, self.fired, mtm_pnl_pct, win_rate);
        RunMetrics {
            n_fired: self.fired,
            n_open: self.open,
            n_closed,
            win_rate,
            total_pnl_sol: self.pnl_sol_sum,
            open_pnl_sol: self.open_pnl_sol_sum,
            expectancy_sol,
            mean_pnl_pct,
            median_pnl_pct,
            p90_pnl_pct,
            best_pnl_pct,
            worst_pnl_pct,
            std_pnl_pct,
            profit_factor,
            mtm_pnl_pct,
            score,
            avg_holding_secs,
            median_holding_secs,
            n_exit_take_profit: self.exit_counts[0],
            n_exit_stop_loss: self.exit_counts[1],
            n_exit_trailing: self.exit_counts[2],
            n_exit_stall: self.exit_counts[3],
            n_exit_time: self.exit_counts[4],
            n_exit_liquidity: self.exit_counts[5],
            n_exit_open: self.exit_counts[6],
            n_exit_dead: self.exit_counts[7],
            n_exit_metrics: self.exit_counts[8],
            n_exit_metrics_win: self.metrics_win,
            n_exit_metrics_loss: self.metrics_loss,
            n_exit_manual: self.exit_counts[9],
            n_exit_migrated: self.exit_counts[10],
        }
    }
}

/// Exact-quantile counterpart to [`RunAgg`] for a **bounded** set of outcomes —
/// e.g. one sweep combo's per-token rows when re-simulated standalone (the
/// grouped-sweep drill-in), never the full combos × tokens sweep (unbounded;
/// that's exactly why [`RunAgg`] streams through a fixed-size sketch instead of
/// holding every value). Same realized-only semantics as `RunAgg::record`/
/// `finalize` (a still-`Open` mark contributes to `n_fired`/`n_open` only, never
/// to a PnL/win-rate/holding figure), but `median_pnl_pct`/`p90_pnl_pct`/
/// `median_holding_secs` are exact nearest-rank percentiles over the collected
/// values instead of the sketch's ~15% relative error — so a drill-in's summary
/// can be compared directly against a single-rule simulate's own small-N exact
/// aggregate (parity plan D1).
pub fn exact_run_metrics<'a>(outcomes: impl Iterator<Item = &'a TokenOutcome>) -> RunMetrics {
    let mut fired = 0u64;
    let mut open = 0u64;
    let mut wins = 0u64;
    let mut pnl_sol_sum = 0.0f64;
    let mut open_pnl_sol_sum = 0.0f64;
    let mut gross_win_sol = 0.0f64;
    let mut gross_loss_sol = 0.0f64;
    let mut closed_pct: Vec<f64> = Vec::new();
    let mut closed_holding: Vec<i64> = Vec::new();
    let mut fired_pct_sum = 0.0f64;
    let mut exit_counts = [0u32; N_EXIT_BUCKETS];
    let mut metrics_win = 0u32;
    let mut metrics_loss = 0u32;

    for o in outcomes {
        if !o.fired {
            continue;
        }
        fired += 1;
        let pnl_pct = o.pnl_percent as f64;
        fired_pct_sum += pnl_pct;
        if o.exit == ExitCode::Open {
            open += 1;
            open_pnl_sol_sum += o.pnl_sol as f64;
        } else {
            let pnl_sol = o.pnl_sol as f64;
            pnl_sol_sum += pnl_sol;
            if pnl_sol > 0.0 {
                wins += 1;
                gross_win_sol += pnl_sol;
            } else if pnl_sol < 0.0 {
                gross_loss_sol += -pnl_sol;
            }
            closed_pct.push(pnl_pct);
            closed_holding.push(o.holding_secs);
            if o.exit == ExitCode::Metrics {
                if o.pnl_sol > 0.0 {
                    metrics_win += 1;
                } else {
                    metrics_loss += 1;
                }
            }
        }
        exit_counts[exit_index(o.exit)] += 1;
    }

    let n_closed = closed_pct.len() as u64;
    let n = n_closed as f64;
    closed_pct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if closed_pct.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (
            exact_quantile_f64(&closed_pct, 0.5),
            exact_quantile_f64(&closed_pct, 0.9),
            *closed_pct.last().expect("non-empty"),
            closed_pct[0],
        )
    };
    let closed_pct_sum: f64 = closed_pct.iter().sum();
    let closed_pct_sum_sq: f64 = closed_pct.iter().map(|p| p * p).sum();
    let mean_pnl_pct = if n_closed == 0 { 0.0 } else { closed_pct_sum / n };
    let mtm_pnl_pct = if fired == 0 { 0.0 } else { fired_pct_sum / fired as f64 };
    closed_holding.sort_unstable();
    let (avg_holding_secs, median_holding_secs) = if closed_holding.is_empty() {
        (0.0, 0.0)
    } else {
        (closed_holding.iter().sum::<i64>() as f64 / n, exact_quantile_i64(&closed_holding, 0.5))
    };
    let profit_factor =
        if gross_loss_sol > 0.0 { Some(gross_win_sol / gross_loss_sol) } else { None };
    let expectancy_sol = if n_closed == 0 { 0.0 } else { pnl_sol_sum / n };
    let std_pnl_pct = sample_std_pct(n_closed, closed_pct_sum, closed_pct_sum_sq);
    let win_rate = if n_closed == 0 { 0.0 } else { wins as f64 / n };
    let score = checklist_score(fired, open, fired, mtm_pnl_pct, win_rate);

    RunMetrics {
        n_fired: fired,
        n_open: open,
        n_closed,
        win_rate,
        total_pnl_sol: pnl_sol_sum,
        open_pnl_sol: open_pnl_sol_sum,
        expectancy_sol,
        mean_pnl_pct,
        median_pnl_pct,
        p90_pnl_pct,
        best_pnl_pct,
        worst_pnl_pct,
        std_pnl_pct,
        profit_factor,
        mtm_pnl_pct,
        score,
        avg_holding_secs,
        median_holding_secs,
        n_exit_take_profit: exit_counts[0],
        n_exit_stop_loss: exit_counts[1],
        n_exit_trailing: exit_counts[2],
        n_exit_stall: exit_counts[3],
        n_exit_time: exit_counts[4],
        n_exit_liquidity: exit_counts[5],
        n_exit_open: exit_counts[6],
        n_exit_dead: exit_counts[7],
        n_exit_metrics: exit_counts[8],
        n_exit_metrics_win: metrics_win,
        n_exit_metrics_loss: metrics_loss,
        n_exit_manual: exit_counts[9],
        n_exit_migrated: exit_counts[10],
    }
}

/// A run reported **twice over the same outcomes** — the shape every surface that
/// summarizes a run (single-rule simulate, grouped sweep, live/paper) sends to the
/// frontend, so one component renders all three (parity plan B4/F1-F3).
///
/// Reporting both is the point. A still-`Open` position has a mark-to-last-price
/// PnL but no realized outcome, so [`realized`](Self::realized) measures closed
/// trades only — which, read alone, flatters a rule that simply never closed its
/// losers: they never entered the sum. [`mtm`](Self::mtm) values every fired
/// position, open bags included. Neither is "the" answer — realized is what
/// actually happened, MTM is what the run is currently worth, and the **gap
/// between them is the signal**: it says how much of the headline is still
/// unsettled.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunSummary {
    /// Closed positions only. Identical to [`exact_run_metrics`]'s output.
    pub realized: RunMetrics,
    /// Every fired position, open ones valued at their last price.
    ///
    /// Only the PnL / win-rate / central-tendency fields are meaningful here; the
    /// `n_exit_*` counts are forced to zero because an open position has no exit
    /// reason to bucket — read them off [`realized`](Self::realized).
    pub mtm: RunMetrics,
}

/// Build the two-band [`RunSummary`] from one pass' worth of outcomes.
///
/// The MTM band is produced by re-running the **same** [`exact_run_metrics`] with
/// the open positions reclassified as closed, rather than by a second hand-rolled
/// copy of the arithmetic — so the two bands can never drift apart and compare
/// tile-for-tile down the column.
pub fn run_summary<'a>(outcomes: impl Iterator<Item = &'a TokenOutcome>) -> RunSummary {
    let all: Vec<TokenOutcome> = outcomes.copied().collect();
    let realized = exact_run_metrics(all.iter());

    // Reclassify Open → a closed bucket so the same aggregator counts its mark as a
    // settled outcome. `TakeProfit` is an arbitrary stand-in purely to get past the
    // `== Open` test; the resulting exit counts are meaningless and zeroed below.
    let marked: Vec<TokenOutcome> = all
        .iter()
        .map(|o| TokenOutcome {
            exit: if o.exit == ExitCode::Open { ExitCode::TakeProfit } else { o.exit },
            ..*o
        })
        .collect();
    let mut mtm = exact_run_metrics(marked.iter());

    // An open position contributes no exit reason — don't let the stand-in above
    // masquerade as a real take-profit.
    mtm.n_exit_take_profit = 0;
    mtm.n_exit_stop_loss = 0;
    mtm.n_exit_trailing = 0;
    mtm.n_exit_stall = 0;
    mtm.n_exit_time = 0;
    mtm.n_exit_liquidity = 0;
    mtm.n_exit_dead = 0;
    mtm.n_exit_metrics = 0;
    mtm.n_exit_metrics_win = 0;
    mtm.n_exit_metrics_loss = 0;
    mtm.n_exit_manual = 0;
    mtm.n_exit_migrated = 0;
    mtm.n_exit_open = 0;
    // The open cohort is what MTM folded in; keep the counts describing the run.
    mtm.n_open = realized.n_open;
    mtm.open_pnl_sol = realized.open_pnl_sol;

    RunSummary { realized, mtm }
}

/// Nearest-rank percentile `q` (`0.0..=1.0`) over an ascending-sorted, non-empty
/// slice. `q=0.5`/`q=0.9` are the median/p90 [`exact_run_metrics`] needs.
///
/// Public as the ONE nearest-rank percentile in the workspace — `lab`'s discovery
/// candidate generator derives its metric percentile ladder through it, so the
/// anchors it publishes and the medians a run reports are the same statistic.
pub fn exact_quantile_f64(sorted: &[f64], q: f64) -> f64 {
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// [`exact_quantile_f64`]'s `i64` counterpart (holding-time seconds).
fn exact_quantile_i64(sorted: &[i64], q: f64) -> f64 {
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

/// Sample stddev of closed per-trade pnl% — display column only.
fn sample_std_pct(n_closed: u64, sum: f64, sum_sq: f64) -> f64 {
    if n_closed < 2 {
        return 0.0;
    }
    let n = n_closed as f64;
    let mean = sum / n;
    let var = ((sum_sq - n * mean * mean) / (n - 1.0)).max(0.0);
    var.sqrt()
}

/// Manual-checklist rank used by the grouped sweep: `mtm_pct × q` for a gain and
/// `mtm_pct ÷ q` for a loss, where the quality factor
/// `q = (n_fired/matched) × (1 − 0.5·n_open/n_fired) × max(win_rate, ε)` ∈ (0, 1].
///
/// - `mtm_pct` — mean pnl% over all fired (still-open marks included)
/// - fire-rate — coverage of the matched group (capped at 1)
/// - open-drag — soft penalty for unsettled bags
/// - win-rate — closed-only; floored so all-open books don't zero the score
///
/// Lower quality always ranks lower: it shrinks a gain toward zero and deepens a
/// loss. Multiplying a loss by `q` instead would pull it toward zero, so a combo
/// that fires rarely and never wins would outrank a small, well-covered loss.
///
/// `None` when nothing fired or `matched == 0`. Public so the sweep can rewrite
/// a combo's score with the group's true matched-token count after finalize.
pub fn checklist_score(
    n_fired: u64,
    n_open: u64,
    matched: u64,
    mtm_pnl_pct: f64,
    win_rate: f64,
) -> Option<f64> {
    if n_fired == 0 || matched == 0 {
        return None;
    }
    let fire_rate = (n_fired as f64 / matched as f64).min(1.0);
    let open_drag = (n_open as f64 / n_fired as f64).min(1.0);
    let wr = win_rate.max(SCORE_WIN_RATE_FLOOR);
    let quality = fire_rate * (1.0 - SCORE_OPEN_DRAG * open_drag) * wr;
    Some(if mtm_pnl_pct >= 0.0 { mtm_pnl_pct * quality } else { mtm_pnl_pct / quality })
}

/// Width of the `exit_counts` histogram — one slot per [`exit_index`] value.
/// Bump together with the match below when adding an [`ExitCode`].
const N_EXIT_BUCKETS: usize = 11;

fn exit_index(e: ExitCode) -> usize {
    match e {
        ExitCode::TakeProfit => 0,
        ExitCode::StopLoss => 1,
        ExitCode::TrailingStop => 2,
        ExitCode::Stall => 3,
        ExitCode::TimeStop => 4,
        ExitCode::LiquidityExit => 5,
        ExitCode::Open | ExitCode::NoEntry => 6,
        ExitCode::Dead => 7,
        ExitCode::Metrics => 8,
        ExitCode::Manual => 9,
        ExitCode::Migrated => 10,
    }
}

// ── Quantile sketch (ported from lab sweep::aggregate) ─────────────────────────

const SKETCH_N: usize = 64;
const SKETCH_BIAS: f64 = 22.65;
const SKETCH_INV_LN_GAMMA: f64 = 3.27885;

/// Fixed-memory, order-independent quantile sketch (DDSketch-style log buckets).
/// Median/p90 carry ~15% relative error; best/worst/mean/total stay exact.
/// Counters are `u32`: a `u16` bucket saturated at 65_535, under-counting the one
/// bucket a dead-heavy 100k-token sweep piles its -99 % closes into and moving
/// the reported median/p90 to another bucket entirely.
#[derive(Clone)]
struct QuantileSketch {
    neg: [u32; SKETCH_N],
    pos: [u32; SKETCH_N],
    zero: u32,
}

impl Default for QuantileSketch {
    fn default() -> Self {
        Self { neg: [0; SKETCH_N], pos: [0; SKETCH_N], zero: 0 }
    }
}

fn sketch_bucket(mag: f64) -> usize {
    let idx = (mag.ln() * SKETCH_INV_LN_GAMMA + SKETCH_BIAS).floor();
    idx.clamp(0.0, (SKETCH_N - 1) as f64) as usize
}

fn sketch_value_at(i: usize) -> f64 {
    ((i as f64 - SKETCH_BIAS + 0.5) / SKETCH_INV_LN_GAMMA).exp()
}

impl QuantileSketch {
    fn record(&mut self, v: f64) {
        if v > 0.0 {
            let b = &mut self.pos[sketch_bucket(v)];
            *b = b.saturating_add(1);
        } else if v < 0.0 {
            let b = &mut self.neg[sketch_bucket(-v)];
            *b = b.saturating_add(1);
        } else {
            self.zero = self.zero.saturating_add(1);
        }
    }

    fn count(&self) -> u64 {
        let neg: u64 = self.neg.iter().map(|&c| c as u64).sum();
        let pos: u64 = self.pos.iter().map(|&c| c as u64).sum();
        neg + pos + self.zero as u64
    }

    fn quantile(&self, q: f64) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let target = ((q * total as f64) as u64).min(total - 1);
        let mut cum = 0u64;
        for i in (0..SKETCH_N).rev() {
            cum += self.neg[i] as u64;
            if cum > target {
                return -sketch_value_at(i);
            }
        }
        cum += self.zero as u64;
        if cum > target {
            return 0.0;
        }
        for i in 0..SKETCH_N {
            cum += self.pos[i] as u64;
            if cum > target {
                return sketch_value_at(i);
            }
        }
        sketch_value_at(SKETCH_N - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 90_500 closes at -99 % and 9_500 at +5 %: the p90 is the loss bucket. A
    /// saturating 16-bit counter capped the loss bucket at 65_535 and read +5 %.
    #[test]
    fn sketch_quantiles_survive_a_bucket_past_u16() {
        let mut s = QuantileSketch::default();
        for _ in 0..90_500 {
            s.record(-99.0);
        }
        for _ in 0..9_500 {
            s.record(5.0);
        }
        assert_eq!(s.count(), 100_000);
        assert!(s.quantile(0.9) < 0.0, "p90 {}", s.quantile(0.9));
    }

    // ── round-trip pricing ──────────────────────────────────────────────────

    #[test]
    fn frictionless_round_trip_is_pure_price_delta() {
        // 2× exit, 1 SOL notional, no costs → +1 SOL, +100%.
        let (sol, pct) = round_trip_with_costs(1.0, 2.0, 1.0, None, None, &CostModel::frictionless());
        assert!((sol - 1.0).abs() < 1e-12);
        assert!((pct - 100.0).abs() < 1e-12);
    }

    #[test]
    fn costs_reduce_pnl_below_frictionless() {
        let costs = CostModel::pumpfun_with_impact();
        let friction = round_trip_with_costs(1.0, 2.0, 1.0, Some(70.0), Some(70.0), &costs).0;
        let free = round_trip_with_costs(1.0, 2.0, 1.0, None, None, &CostModel::frictionless()).0;
        assert!(friction < free, "costs must drag PnL down");
    }

    /// One real round trip of 2026-09-13 (position 785bedf3), priced from the pool
    /// state it landed in, against what the wallet moved on chain: -50 227 000
    /// lamports on the buy, +42 328 180 on the sell. The kernel reproduces both.
    #[test]
    fn a_real_round_trip_reproduces_the_wallet() {
        let costs = CostModel {
            fee_bps_per_leg: 125.0,
            fixed_buy_sol: 0.000_227,
            fixed_sell_sol: 0.000_225,
            close_fee_sol: 0.0,
            price_impact: true,
        };
        // Pre-buy pool: our leg's post-trade reserves minus / plus our own leg.
        let (buy_amt, buy_tok) = (0.049_382_715, 551_616_111_320.0);
        let v0 = 53.706_796_751 - buy_amt;
        let p0 = v0 / (599_365_479_873_277.0 + buy_tok);
        let (tokens, paid) = buy_fill(0.05, p0, Some(v0), &costs);
        // The program floors to whole lamports and raw units: 1 lamport of the fee
        // split is 2e-8 of this order.
        assert!((tokens / buy_tok - 1.0).abs() < 1e-7, "tokens {tokens} vs {buy_tok}");
        assert!((paid - 0.050_227).abs() < 1e-12);
        // Pre-sell pool, same reconstruction.
        let sell_amt = 0.043_091_829;
        let v1 = 50.124_826_570 + sell_amt;
        let p1 = v1 / (642_196_736_136_456.0 - buy_tok);
        let got = sell_proceeds(buy_tok, p1, Some(v1), &costs, false);
        assert!((got - 0.042_328_180).abs() < 2e-9, "got {got}");
    }

    // ── multi-leg round-trip (scale-out) ─────────────────────────────────────

    #[test]
    fn multi_leg_single_full_exit_matches_round_trip() {
        let m = CostModel::pumpfun_with_impact();
        let depth = Some(70.0);
        let single = round_trip_with_costs(1.0, 1.25, 0.1, depth, depth, &m);
        let multi = round_trip_multi_leg(
            1.0,
            0.1,
            depth,
            &[ExitLeg { sell_bps: 10_000, price: 1.25, reserve_sol: depth }],
            &m,
        );
        assert!((single.0 - multi.0).abs() < 1e-12, "sol {} vs {}", single.0, multi.0);
        assert!((single.1 - multi.1).abs() < 1e-12, "pct {} vs {}", single.1, multi.1);
    }

    #[test]
    fn multi_leg_fixed_cost_scales_with_exit_count() {
        // Same prices / full coverage: one 100% exit vs two 50% exits at the same
        // price. Frictionless PnL is identical; with a fixed tip the 2-leg path
        // pays one extra fixed_sell_sol — the economic bound on stages — and the
        // close is paid once either way.
        let tip = 0.001;
        let m = CostModel {
            fee_bps_per_leg: 0.0,
            fixed_buy_sol: tip,
            fixed_sell_sol: tip,
            close_fee_sol: 0.000_005,
            price_impact: false,
        };
        let one = round_trip_multi_leg(
            1.0,
            0.1,
            None,
            &[ExitLeg { sell_bps: 10_000, price: 1.10, reserve_sol: None }],
            &m,
        )
        .0;
        let two = round_trip_multi_leg(
            1.0,
            0.1,
            None,
            &[
                ExitLeg { sell_bps: 5_000, price: 1.10, reserve_sol: None },
                ExitLeg { sell_bps: 5_000, price: 1.10, reserve_sol: None },
            ],
            &m,
        )
        .0;
        assert!(
            (one - two - tip).abs() < 1e-12,
            "2-leg must cost exactly one extra tip: one={one} two={two} tip={tip}"
        );
    }

    #[test]
    fn multi_leg_prices_tranches_at_their_own_fills() {
        // Frictionless: bank 70% at +50%, stub 30% at flat → net +35% of notional.
        let m = CostModel::frictionless();
        let (sol, pct) = round_trip_multi_leg(
            1.0,
            1.0,
            None,
            &[
                ExitLeg { sell_bps: 7_000, price: 1.50, reserve_sol: None },
                ExitLeg { sell_bps: 3_000, price: 1.00, reserve_sol: None },
            ],
            &m,
        );
        assert!((sol - 0.35).abs() < 1e-12, "got sol={sol}");
        assert!((pct - 35.0).abs() < 1e-12, "got pct={pct}");
    }

    // ── price impact (§2g) ──────────────────────────────────────────────────

    #[test]
    fn price_impact_is_the_exact_curve_and_scales_with_size() {
        // Same trade, same pool, three sizes. The haircut must grow with size —
        // the whole point the retired flat-slippage model missed.
        let m = CostModel::pumpfun_with_impact();
        let depth = Some(70.0);
        let small = round_trip_with_costs(1.0, 1.10, 0.1, depth, depth, &m).1;
        let mid = round_trip_with_costs(1.0, 1.10, 0.27, depth, depth, &m).1;
        let big = round_trip_with_costs(1.0, 1.10, 1.0, depth, depth, &m).1;
        assert!(big < mid && mid < small, "bigger order must cost more: {small} {mid} {big}");

        // Buy 1 SOL into 70 and sell straight back into the pool it left: the curve
        // returns 1 / (1 + 2/70) exactly, so the round trip loses (2/70)/(1 + 2/70).
        let free = CostModel {
            fee_bps_per_leg: 0.0,
            fixed_buy_sol: 0.0,
            fixed_sell_sol: 0.0,
            close_fee_sol: 0.0,
            ..m
        };
        let (_, pct) = round_trip_with_costs(1.0, 1.0, 1.0, Some(70.0), Some(70.0), &free);
        let want = -100.0 * (2.0 / 70.0) / (1.0 + 2.0 / 70.0);
        assert!((pct - want).abs() < 1e-9, "got {pct}, want {want}");
    }

    #[test]
    fn price_impact_is_inert_without_depth_or_without_the_flag() {
        // Depth unknown ⇒ degrades to fee-only, never a silent divide-by-zero.
        let m = CostModel::pumpfun_with_impact();
        let none = round_trip_with_costs(1.0, 1.10, 1.0, None, None, &m).1;
        let zero = round_trip_with_costs(1.0, 1.10, 1.0, Some(0.0), Some(0.0), &m).1;
        let neg = round_trip_with_costs(1.0, 1.10, 1.0, Some(-5.0), Some(f64::NAN), &m).1;
        assert!((none - zero).abs() < 1e-12 && (none - neg).abs() < 1e-12);
        assert!(none.is_finite());

        // The size-blind kind ignores depth entirely, so a caller who happens to
        // have depth in hand cannot change what it charges.
        let blind = CostModel::pumpfun_fee_only();
        let a = round_trip_with_costs(1.0, 1.10, 1.0, None, None, &blind).1;
        let b = round_trip_with_costs(1.0, 1.10, 1.0, Some(70.0), Some(70.0), &blind).1;
        assert!((a - b).abs() < 1e-12, "size-blind model must be depth-blind");
    }

    /// Each exit leg prices on its OWN depth: the same sell into a pool that
    /// drained returns less than into the pool the entry saw.
    #[test]
    fn the_exit_prices_on_the_exit_depth() {
        let m = CostModel::pumpfun_with_impact();
        let same = round_trip_with_costs(1.0, 1.0, 1.0, Some(70.0), Some(70.0), &m).0;
        let drained = round_trip_with_costs(1.0, 1.0, 1.0, Some(70.0), Some(35.0), &m).0;
        assert!(drained < same, "drained {drained} vs same {same}");
    }

    /// The percent divides by what the wallet paid — the order plus its fixed
    /// cost — never by the bare notional.
    #[test]
    fn the_percent_is_over_the_capital_paid() {
        let m = CostModel::pumpfun_with_impact();
        let (sol, pct) = round_trip_with_costs(1.0, 1.3, 0.05, Some(60.0), Some(60.0), &m);
        assert!((pct - sol / m.capital_sol(0.05) * 100.0).abs() < 1e-12);
        assert!(m.capital_sol(0.05) > 0.05);
    }

    #[test]
    fn impact_model_charges_impact_and_the_same_fee() {
        let m = CostModel::pumpfun_with_impact();
        assert!(m.price_impact);
        assert_eq!(m.fee_bps_per_leg, CostModel::pumpfun_fee_only().fee_bps_per_leg);
    }

    #[test]
    fn cost_model_kind_serde_names_and_default() {
        use serde_json::json;
        for (name, want) in [
            ("pumpfun_impact", CostModelKind::PumpfunImpact),
            ("pumpfun_fee_only", CostModelKind::PumpfunFeeOnly),
            // Short aliases, so a request can name the model the way the analysis does.
            ("impact", CostModelKind::PumpfunImpact),
            ("fee_only", CostModelKind::PumpfunFeeOnly),
        ] {
            let got: CostModelKind = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(got, want, "'{name}'");
        }
        // The retired flat-slippage names are GONE, not aliased. A payload naming one
        // fails loudly: quietly repricing it would report a run as computed under a
        // model it never saw, and no stored record names one any more.
        for retired in ["pumpfun_default", "default", "pumpfun_legacy_slippage"] {
            assert!(
                serde_json::from_value::<CostModelKind>(json!(retired)).is_err(),
                "'{retired}' must not decode"
            );
        }
        assert_eq!(
            serde_json::to_value(CostModelKind::PumpfunFeeOnly).unwrap(),
            json!("pumpfun_fee_only")
        );
        // An omitted field takes the size-aware model — the one thing a cost model
        // has to get right that a caller cannot supply by forgetting to.
        assert_eq!(CostModelKind::default(), CostModelKind::PumpfunImpact);
    }

    /// No kind charges a flat per-leg slippage any more, because a `FillModel`
    /// already prices exactly that. This is a structural claim, so assert it over
    /// every kind rather than trusting the constructors one at a time.
    #[test]
    fn no_kind_double_counts_what_the_fill_model_prices() {
        // Depth withheld ⇒ impact is inert ⇒ every kind must collapse to the same
        // number. If any kind still carried a flat slippage term, it would not.
        let impact = CostModelKind::PumpfunImpact.model();
        let fee_only = CostModelKind::PumpfunFeeOnly.model();
        let a = round_trip_with_costs(1.0, 1.5, 1.0, None, None, &impact);
        let b = round_trip_with_costs(1.0, 1.5, 1.0, None, None, &fee_only);
        assert!((a.0 - b.0).abs() < 1e-12, "a size-blind kind must be the fee-only kind");

        // …and with depth, the ONLY thing that separates them is our own footprint.
        let with = round_trip_with_costs(1.0, 1.5, 1.0, Some(70.0), Some(70.0), &impact);
        assert!(with.0 < a.0, "impact must cost something once depth is known");
    }

    #[test]
    fn cost_model_fixed_cost_tracks_fee_tuning_tip() {
        let cheap = FeeTuning {
            jito_min_tip_sol: 0.0001,
            ..FeeTuning::defaults()
        };
        let dear = FeeTuning {
            jito_min_tip_sol: 0.001,
            ..FeeTuning::defaults()
        };
        let c = CostModel::pumpfun_with_impact_with(&cheap);
        let d = CostModel::pumpfun_with_impact_with(&dear);
        assert!((c.fixed_buy_sol - cheap.fixed_buy_sol()).abs() < 1e-15);
        assert!((c.fixed_sell_sol - cheap.fixed_sell_sol()).abs() < 1e-15);
        assert!(d.fixed_buy_sol > c.fixed_buy_sol && d.fixed_sell_sol > c.fixed_sell_sol);
    }

    // ── exact_run_metrics (parity plan D1) ──────────────────────────────────

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        TokenOutcome { fired: true, holding_secs: holding, pnl_percent: pnl_pct, pnl_sol, exit }
    }

    #[test]
    fn exact_metrics_matches_streaming_agg_on_the_same_outcomes() {
        // exact_run_metrics must agree with RunAgg (the streaming/sketch path) on
        // every field RunAgg computes exactly already — the only thing that should
        // ever differ is that median/p90/median_holding_secs stop being approximate.
        let rows = vec![
            outcome(2.0, 100.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 20),
            outcome(5.0, 999.0, ExitCode::Open, 0),
            TokenOutcome::no_entry(),
        ];
        let mut agg = RunAgg::default();
        for o in &rows {
            agg.record(o);
        }
        let streaming = agg.finalize();
        let exact = exact_run_metrics(rows.iter());
        assert_eq!(exact.n_fired, streaming.n_fired);
        assert_eq!(exact.n_open, streaming.n_open);
        assert_eq!(exact.n_closed, streaming.n_closed);
        assert!((exact.win_rate - streaming.win_rate).abs() < 1e-9);
        assert!((exact.total_pnl_sol - streaming.total_pnl_sol).abs() < 1e-9);
        assert!((exact.mean_pnl_pct - streaming.mean_pnl_pct).abs() < 1e-9);
        assert_eq!(exact.profit_factor, streaming.profit_factor);
        assert!((exact.score.unwrap() - streaming.score.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn exact_median_and_p90_have_no_sketch_error() {
        // 1..=1000 → exact median is 500 or 501 (nearest-rank on 1000 values picks
        // one deterministically), exact p90 is exactly 900 — no ~15% band needed.
        let rows: Vec<TokenOutcome> =
            (1..=1000).map(|v| outcome(0.1, v as f32, ExitCode::TakeProfit, v)).collect();
        let m = exact_run_metrics(rows.iter());
        assert!((500.0..=501.0).contains(&m.median_pnl_pct), "median {}", m.median_pnl_pct);
        assert_eq!(m.p90_pnl_pct, 900.0);
    }

    #[test]
    fn exact_metrics_excludes_open_from_headline_figures() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
            outcome(1_000.0, 5_000.0, ExitCode::Open, 0),
        ];
        let m = exact_run_metrics(rows.iter());
        assert_eq!(m.n_fired, 3);
        assert_eq!(m.n_open, 1);
        assert!((m.total_pnl_sol - 0.0).abs() < 1e-9);
        assert_eq!(m.best_pnl_pct, 50.0);
        assert_eq!(m.worst_pnl_pct, -50.0);
    }

    // ── two-band run summary (parity plan B4) ───────────────────────────────

    #[test]
    fn run_summary_bands_split_realized_from_mark_to_market() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
            outcome(-4.0, -80.0, ExitCode::Open, 0), // a big unrealized LOSER
        ];
        let s = run_summary(rows.iter());

        // Realized reads flat — the loser never closed.
        assert!((s.realized.total_pnl_sol - 0.0).abs() < 1e-9);
        assert_eq!(s.realized.n_closed, 2);
        // MTM tells the truth about what the run is currently worth.
        assert!((s.mtm.total_pnl_sol - -4.0).abs() < 1e-9);
        assert_eq!(s.mtm.n_closed, 3, "MTM settles every fired position");
        assert!((s.mtm.worst_pnl_pct - -80.0).abs() < 1e-9, "the open loser is the MTM worst");
        // Both bands agree on how much is unsettled.
        assert_eq!(s.realized.n_open, 1);
        assert_eq!(s.mtm.n_open, 1);
        assert!((s.mtm.open_pnl_sol - -4.0).abs() < 1e-9);
    }

    #[test]
    fn run_summary_bands_are_identical_when_nothing_is_open() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
        ];
        let s = run_summary(rows.iter());
        assert!((s.realized.total_pnl_sol - s.mtm.total_pnl_sol).abs() < 1e-9);
        assert!((s.realized.win_rate - s.mtm.win_rate).abs() < 1e-9);
        assert!((s.realized.median_pnl_pct - s.mtm.median_pnl_pct).abs() < 1e-9);
    }

    #[test]
    fn mtm_band_reports_no_exit_reasons() {
        // The Open→TakeProfit reclassification must never surface as a real exit.
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(2.0, 90.0, ExitCode::Open, 0),
        ];
        let s = run_summary(rows.iter());
        assert_eq!(s.realized.n_exit_take_profit, 1);
        assert_eq!(s.mtm.n_exit_take_profit, 0, "stand-in must not read as a take-profit");
    }

    #[test]
    fn exact_metrics_over_no_outcomes_is_all_zero() {
        let m = exact_run_metrics(std::iter::empty());
        assert_eq!(m.n_fired, 0);
        assert_eq!(m.score, None);
        assert_eq!(m.profit_factor, None);
    }

    // ── checklist_score ─────────────────────────────────────────────────────

    #[test]
    fn score_is_mtm_pct_when_fully_closed_and_all_wins() {
        // fire_rate=1, open_drag=0, win_rate=1 → score == mtm_pnl_pct.
        let rows = vec![
            outcome(0.5, 50.0, ExitCode::TakeProfit, 5),
            outcome(0.5, 50.0, ExitCode::TakeProfit, 5),
        ];
        let m = exact_run_metrics(rows.iter());
        assert!((m.mtm_pnl_pct - 50.0).abs() < 1e-9);
        assert_eq!(m.score, Some(50.0));
    }

    #[test]
    fn score_includes_open_marks_in_mtm_and_penalises_open_share() {
        let rows = vec![
            outcome(0.1, 10.0, ExitCode::TakeProfit, 5),
            outcome(0.1, 10.0, ExitCode::TakeProfit, 5),
            outcome(5.0, 90.0, ExitCode::Open, 0),
        ];
        let m = exact_run_metrics(rows.iter());
        // MTM mean = (10+10+90)/3 = 36.666…
        assert!((m.mtm_pnl_pct - 110.0 / 3.0).abs() < 1e-9);
        // × 1 × (1 − 0.5·1/3) × 1.0 = × (5/6)
        let expected = m.mtm_pnl_pct * (1.0 - 0.5 / 3.0);
        assert!((m.score.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn score_none_when_nothing_fired() {
        assert_eq!(exact_run_metrics(std::iter::empty()).score, None);
        assert_eq!(
            checklist_score(0, 0, 10, 50.0, 1.0),
            None,
            "unfired combo has no score"
        );
    }

    #[test]
    fn checklist_score_scales_with_fire_rate() {
        // Same book, half coverage → half score.
        let full = checklist_score(10, 0, 10, 40.0, 1.0).unwrap();
        let half = checklist_score(5, 0, 10, 40.0, 1.0).unwrap();
        assert!((full - 40.0).abs() < 1e-9);
        assert!((half - 20.0).abs() < 1e-9);
    }

    /// Among losers, the small well-covered loss ranks above a big loss that fired
    /// on a tenth of the group and never won - lower quality deepens a loss.
    #[test]
    fn checklist_score_never_rewards_a_loser_for_low_quality() {
        let small = checklist_score(100, 0, 100, -2.0, 0.45).unwrap();
        let big_rare = checklist_score(10, 5, 100, -20.0, 0.0).unwrap();
        assert!(small > big_rare, "{small} vs {big_rare}");
        // Same loss, less coverage: ranks lower, not higher.
        let full = checklist_score(10, 0, 10, -4.0, 1.0).unwrap();
        let half = checklist_score(5, 0, 10, -4.0, 1.0).unwrap();
        assert!(full > half, "{full} vs {half}");
    }

    /// Golden vectors for [`sell_value_proceeds`], shared with the frontend mirror.
    ///
    /// The browser has to net a mark between holdings polls (`walletMarksLive`),
    /// so `netProceedsSol` in `lib/liveMark.ts` re-implements this arithmetic in
    /// TS. These four cases are asserted on BOTH sides against the same literals
    /// — `netProceedsMatchesRust` in `liveMark.test.ts` — so a change to either
    /// implementation fails the other's test instead of quietly giving two
    /// answers to "what is this bag worth". Constants are explicit rather than
    /// `FeeTuning::current()` so the vectors do not move with `.env`.
    #[test]
    fn sell_value_proceeds_golden_vectors() {
        let costs = CostModel {
            fee_bps_per_leg: 125.0,
            fixed_buy_sol: 0.00025,
            fixed_sell_sol: 0.00025,
            close_fee_sol: 0.000_005,
            price_impact: true,
        };
        // (value at spot, reserve) -> net proceeds of the sell that empties the bag
        let cases: [(f64, Option<f64>, f64); 4] = [
            // No depth: fee, the leg's fixed cost and the close only.
            (0.05, None, 0.049120000000000004),
            // Into a 70 SOL pool: the curve returns 0.1 / (1 + 0.1/70).
            (0.1, Some(70.0), 0.098354129814550648),
            (0.030784, None, 0.0301442),
            // A bag ten times the pool: the curve still pays, just far less.
            (30.0, Some(3.0), 2.6929268181818182),
        ];
        for (value, reserve, want) in cases {
            let got = sell_value_proceeds(value, reserve, &costs, true);
            assert!((got - want).abs() < 1e-12, "value {value}: {got} != {want}");
        }
    }

    /// A PumpSwap leg swaps in only the pool's fee; a curve leg (`None`) and a
    /// garbage fee keep the model as it is.
    #[test]
    fn at_venue_fee_swaps_only_the_fee() {
        let curve = CostModel::pumpfun_with_impact();
        let amm = curve.at_venue_fee(Some(95.0));
        assert_eq!(amm.fee_bps_per_leg, 95.0);
        assert_eq!(amm.fixed_sell_sol, curve.fixed_sell_sol);
        assert_eq!(amm.close_fee_sol, curve.close_fee_sol);
        assert!(amm.price_impact);
        // A sell at 95 bps into a 70 SOL pool: g/(1+g/V)·(1−0.0095) − fixed − close.
        let got = sell_value_proceeds(0.1, Some(70.0), &amm, true);
        let want = 0.1 / (1.0 + 0.1 / 70.0) * (1.0 - 0.0095) - curve.fixed_sell_sol - curve.close_fee_sol;
        assert!((got - want).abs() < 1e-15, "{got} vs {want}");
        for keep in [None, Some(f64::NAN), Some(-1.0)] {
            assert_eq!(curve.at_venue_fee(keep).fee_bps_per_leg, curve.fee_bps_per_leg);
        }
    }

    /// The entry fill is sunk: marking at the price it filled at is a LOSS of the
    /// exit leg's costs, never break-even, and the entry's impact is not charged
    /// again — it is inside the basis the fill actually paid.
    #[test]
    fn mark_open_bag_does_not_recharge_entry_impact() {
        let costs = CostModel::pumpfun_with_impact();
        let (tokens, paid) = buy_fill(1.0, 1.0, Some(10.0), &costs);
        let open = mark_open_bag(paid, 1.0, tokens, Some(10.0), &costs);
        let (round_trip, _) = round_trip_with_costs(1.0, 1.0, 1.0, Some(10.0), Some(10.0), &costs);
        assert!(open < 0.0);
        assert!(open > round_trip, "open mark {open} must not pay entry impact twice ({round_trip})");
    }

    /// Marking a fresh fill at a later price IS the round trip at that price: the
    /// open mark and the closed trade are one formula.
    #[test]
    fn mark_open_bag_equals_the_round_trip_it_would_close() {
        let costs = CostModel::pumpfun_with_impact();
        let (tokens, paid) = buy_fill(0.05, 1e-7, Some(55.0), &costs);
        let open = mark_open_bag(paid, 1.3e-7, tokens, Some(62.0), &costs);
        let (closed, _) = round_trip_with_costs(1e-7, 1.3e-7, 0.05, Some(55.0), Some(62.0), &costs);
        assert!((open - closed).abs() < 1e-15, "open {open} closed {closed}");
    }

    /// [`modeled_cost_basis`] inverts [`buy_fill`]: the curve-side price of an
    /// executed buy, times its tokens, grossed up by the fee plus the fixed leg,
    /// is what the order took from the wallet.
    #[test]
    fn modeled_cost_basis_inverts_buy_fill() {
        let costs = CostModel::pumpfun_with_impact();
        let (tokens, paid) = buy_fill(0.05, 1e-7, Some(55.0), &costs);
        let curve_sol = 0.05 / (1.0 + costs.fee_bps_per_leg / 10_000.0);
        let exec_price = curve_sol / tokens;
        assert!((modeled_cost_basis(exec_price, tokens, &costs) - paid).abs() < 1e-15);
    }
}
