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
/// Execution-cost model the kernel prices every round-trip with, so simulated
/// PnL reflects the frictions the live trader pays. All knobs apply to **both**
/// legs (symmetric entry/exit).
///
/// Fixed per-leg cost (tip + priority) comes from process-wide [`FeeTuning`] —
/// the same `JITO_MIN_TIP_SOL` / `CU_PRICE_MICRO_LAMPORTS` live applies to the
/// trader. Install via [`FeeTuning::install`] after `dotenvy` in each bin.
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    pub fee_bps_per_leg: f64,
    pub fixed_cost_sol_per_leg: f64,
    /// Charge **our own** constant-product price impact, `notional_sol /
    /// reserve_sol` per leg, from the pool depth passed to
    /// [`round_trip_with_costs`].
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
    /// impact (`notional_sol / reserve_sol` per leg), and no flat `slippage_bps`.
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
            fixed_cost_sol_per_leg: tuning.fixed_cost_sol_per_leg(),
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

    /// A frictionless model (no fees/slippage/fixed cost) — pure price-to-price,
    /// for analytic baselines and tests.
    pub fn frictionless() -> Self {
        Self { fee_bps_per_leg: 0.0, fixed_cost_sol_per_leg: 0.0, price_impact: false }
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
    /// SOL-side pool depth at this leg for [`CostModel::price_impact`]. `None`
    /// charges no impact on this leg (same contract as
    /// [`round_trip_with_costs`]'s reserve arg).
    pub reserve_sol: Option<f64>,
}

/// Net PnL of a buy@`entry_price` / sell@`exit_price` round-trip sized at
/// `notional_sol`, net of `costs`. Thin wrapper over [`round_trip_multi_leg`]
/// with a single full-bag exit — the legacy 1-exit shape.
///
/// `reserve_sol` is the **SOL-side pool depth at entry**, reused for the exit
/// leg's impact (slightly over-charges winners when the pool grew — pessimistic
/// on the trades that matter). Pass `None` when unknown.
pub fn round_trip_with_costs(
    entry_price: f64,
    exit_price: f64,
    notional_sol: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
) -> (f64, f64) {
    round_trip_multi_leg(
        entry_price,
        notional_sol,
        reserve_sol,
        &[ExitLeg { sell_bps: 10_000, price: exit_price, reserve_sol }],
        costs,
    )
}

/// Multi-leg sibling of [`round_trip_with_costs`]: one entry + `exits` sell legs.
/// Each leg pays fee bps + fixed-per-leg + impact(`leg_notional / reserve_at_leg`).
/// Fixed cost therefore scales with leg count — the real economic bound on
/// scale-out stage count (~1% of notional per extra exit leg at 0.1 SOL size).
///
/// `exits` must be non-empty and `sum(sell_bps)` should cover the bag being
/// priced (10_000 for a full close; less + a mark leg for mid-ladder open MTM).
/// Returns `(pnl_sol, pnl_percent)` of the entry notional. Empty / invalid
/// inputs → `(0, 0)`.
pub fn round_trip_multi_leg(
    entry_price: f64,
    notional_sol: f64,
    entry_reserve_sol: Option<f64>,
    exits: &[ExitLeg],
    costs: &CostModel,
) -> (f64, f64) {
    if entry_price <= 0.0 || notional_sol <= 0.0 || exits.is_empty() {
        return (0.0, 0.0);
    }
    let fee = costs.fee_bps_per_leg / 10_000.0;
    let entry_impact = leg_impact(costs, notional_sol, entry_reserve_sol);
    let eff_entry = entry_price * (1.0 + entry_impact);
    if eff_entry <= 0.0 {
        return (0.0, 0.0);
    }
    let tokens_total = notional_sol / eff_entry;

    let mut gross_proceeds = 0.0;
    let mut n_exit = 0u32;
    for leg in exits {
        if leg.sell_bps == 0 {
            continue;
        }
        let frac = leg.sell_bps as f64 / 10_000.0;
        let leg_tokens = tokens_total * frac;
        // Impact sized to this leg's share of the entry notional (same B/vsol
        // grain as the single-leg path, just not the full bag).
        let exit_impact = leg_impact(costs, notional_sol * frac, leg.reserve_sol);
        let eff_exit = leg.price * (1.0 - exit_impact).max(0.0);
        gross_proceeds += leg_tokens * eff_exit;
        n_exit += 1;
    }
    if n_exit == 0 {
        return (0.0, 0.0);
    }
    // Fee on entry notional + sum of exit proceeds; fixed cost once per leg
    // (1 entry + N exits).
    let costs_sol = (notional_sol + gross_proceeds) * fee
        + costs.fixed_cost_sol_per_leg * (1.0 + f64::from(n_exit));
    let pnl_sol = gross_proceeds - notional_sol - costs_sol;
    (pnl_sol, pnl_sol / notional_sol * 100.0)
}

/// Our own footprint in the pool for one leg. `filter` (not a bare
/// `unwrap_or`) so a zero / negative depth cannot divide by ~0.
fn leg_impact(costs: &CostModel, size_sol: f64, reserve_sol: Option<f64>) -> f64 {
    if !costs.price_impact || size_sol <= 0.0 {
        return 0.0;
    }
    reserve_sol.filter(|r| *r > 0.0).map_or(0.0, |r| size_sol / r)
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

// ── Wallet trade reconstruction (Trader Analysis) ──────────────────────────────

/// Avg-cost PnL reconstruction for one wallet's aggregate buy/sell activity on
/// one mint within a window — the Trader Analysis page's per-token stats. NOT a
/// FIFO episode ledger (that needs per-trade ordering, which this aggregate
/// doesn't carry); the closed portion's cost basis is `avg_buy_price ×
/// matched_tokens`, where `matched_tokens = min(buy_tokens, sell_tokens)`.
///
/// When the wallet sold more than it bought *in this window* (its opening buy
/// predates the look-back window), the unmatched sell proceeds are apportioned
/// by the matched fraction rather than guessed at, and `partial_data` flags the
/// row so the UI can say so instead of presenting a precise-looking number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalletMintPnl {
    /// `buy_sol / buy_token_amount` — `None` when the wallet never bought in the
    /// window (a mint it only received/sold, e.g. an airdrop).
    pub avg_buy_price: Option<f64>,
    /// `sell_sol / sell_token_amount` — `None` when the wallet never sold.
    pub avg_sell_price: Option<f64>,
    /// `buy_token_amount − sell_token_amount`. Positive = still holding a bag on
    /// this mint; negative means the wallet sold more than it bought in the
    /// window (see `partial_data`), never an accounting error.
    pub net_token_amount: i64,
    /// Realized PnL on the matched (closed) portion, gross of the pump.fun fee —
    /// `proceeds_of_matched − cost_basis_of_matched`.
    pub realized_pnl_sol: f64,
    /// Same as `realized_pnl_sol`, net of the measured pump.fun protocol fee
    /// (`kernel::FEE_BPS_PER_LEG`, both legs) — no tip/priority-fee charge, since
    /// those are OUR execution cost, not observable on someone else's trades.
    pub realized_pnl_sol_net_of_fee: f64,
    /// `realized_pnl_sol / cost_basis_of_matched × 100` — `None` when the matched
    /// cost basis is zero (nothing to divide by: no buys, or the buy was free).
    pub realized_pnl_pct: Option<f64>,
    /// Mark-to-market PnL on the still-open bag (`net_token_amount > 0`), using
    /// the token's current spot price — `None` when there's no open bag, or the
    /// current price is unknown. Gross of fee (an unrealized mark, not an actual
    /// exit — charging a hypothetical exit fee here would overstate the haircut
    /// for a bag that might still be held when fees/slippage move).
    pub unrealized_pnl_sol: Option<f64>,
    /// `realized_pnl_sol + unrealized_pnl_sol.unwrap_or(0.0)` — the single
    /// mark-to-market ranking number (Trader Analysis' "ranked by PnL" chart
    /// sorts on this, not on `realized_pnl_sol` alone, so a wallet's still-open
    /// runner isn't invisible to the ranking).
    pub total_pnl_sol: f64,
    /// `net_token_amount > 0` — still holding some of this mint.
    pub is_open: bool,
    /// The wallet sold more than it bought within the window — the cost basis on
    /// the unmatched portion is unknown (a pre-window position), so every PnL
    /// figure above is a partial-window estimate, not the wallet's true realized
    /// result on this mint.
    pub partial_data: bool,
}

/// Compute [`WalletMintPnl`] from one wallet's raw aggregate trade sums on one
/// mint. `buy_sol`/`sell_sol` are the recorded curve-side amounts (pre-fee, see
/// [`WalletMintPnl::realized_pnl_sol_net_of_fee`]'s doc); `current_price` is the
/// token's current spot price (human SOL per raw token unit — the same
/// convention `avg_buy_price`/`avg_sell_price` use), or `None` if unknown.
pub fn wallet_mint_pnl(
    buy_sol: f64,
    sell_sol: f64,
    buy_token_amount: i64,
    sell_token_amount: i64,
    current_price: Option<f64>,
) -> WalletMintPnl {
    let avg_buy_price = (buy_token_amount > 0).then(|| buy_sol / buy_token_amount as f64);
    let avg_sell_price = (sell_token_amount > 0).then(|| sell_sol / sell_token_amount as f64);

    let matched_tokens = buy_token_amount.min(sell_token_amount).max(0) as f64;
    let cost_basis_matched = avg_buy_price.unwrap_or(0.0) * matched_tokens;
    // Apportion sell proceeds by the matched fraction — a no-op (fraction = 1)
    // in the common case `sell_token_amount <= buy_token_amount`.
    let proceeds_matched = if sell_token_amount > 0 {
        sell_sol * (matched_tokens / sell_token_amount as f64)
    } else {
        0.0
    };

    let realized_pnl_sol = proceeds_matched - cost_basis_matched;
    let fee = FEE_BPS_PER_LEG / 10_000.0;
    let realized_pnl_sol_net_of_fee =
        proceeds_matched * (1.0 - fee) - cost_basis_matched * (1.0 + fee);
    let realized_pnl_pct =
        (cost_basis_matched > 0.0).then(|| realized_pnl_sol / cost_basis_matched * 100.0);

    let net_token_amount = buy_token_amount - sell_token_amount;
    let unrealized_pnl_sol = if net_token_amount > 0 {
        match (avg_buy_price, current_price) {
            (Some(abp), Some(cp)) => Some(net_token_amount as f64 * (cp - abp)),
            _ => None,
        }
    } else {
        None
    };
    let total_pnl_sol = realized_pnl_sol + unrealized_pnl_sol.unwrap_or(0.0);

    WalletMintPnl {
        avg_buy_price,
        avg_sell_price,
        net_token_amount,
        realized_pnl_sol,
        realized_pnl_sol_net_of_fee,
        realized_pnl_pct,
        unrealized_pnl_sol,
        total_pnl_sol,
        is_open: net_token_amount > 0,
        partial_data: sell_token_amount > buy_token_amount,
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

/// Manual-checklist rank used by the grouped sweep:
/// `mtm_pct × (n_fired/matched) × (1 − 0.5·n_open/n_fired) × max(win_rate, ε)`.
///
/// - `mtm_pct` — mean pnl% over all fired (still-open marks included)
/// - fire-rate — coverage of the matched group (capped at 1)
/// - open-drag — soft penalty for unsettled bags
/// - win-rate — closed-only; floored so all-open books don't zero the score
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
    Some(mtm_pnl_pct * fire_rate * (1.0 - SCORE_OPEN_DRAG * open_drag) * wr)
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
#[derive(Clone)]
struct QuantileSketch {
    neg: [u16; SKETCH_N],
    pos: [u16; SKETCH_N],
    zero: u16,
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

    // ── round-trip pricing ──────────────────────────────────────────────────

    #[test]
    fn frictionless_round_trip_is_pure_price_delta() {
        // 2× exit, 1 SOL notional, no costs → +1 SOL, +100%.
        let (sol, pct) = round_trip_with_costs(1.0, 2.0, 1.0, None, &CostModel::frictionless());
        assert!((sol - 1.0).abs() < 1e-12);
        assert!((pct - 100.0).abs() < 1e-12);
    }

    #[test]
    fn costs_reduce_pnl_below_frictionless() {
        let costs = CostModel::pumpfun_with_impact();
        let friction = round_trip_with_costs(1.0, 2.0, 1.0, Some(70.0), &costs).0;
        let free = round_trip_with_costs(1.0, 2.0, 1.0, None, &CostModel::frictionless()).0;
        assert!(friction < free, "costs must drag PnL down");
    }

    // ── multi-leg round-trip (scale-out) ─────────────────────────────────────

    #[test]
    fn multi_leg_single_full_exit_matches_round_trip() {
        let m = CostModel::pumpfun_with_impact();
        let depth = Some(70.0);
        let single = round_trip_with_costs(1.0, 1.25, 0.1, depth, &m);
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
        // pays one extra fixed_cost_sol_per_leg — the economic bound on stages.
        let tip = 0.001;
        let m = CostModel {
            fee_bps_per_leg: 0.0,
            fixed_cost_sol_per_leg: tip,
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
    fn price_impact_is_notional_over_depth_and_scales_with_size() {
        // Same trade, same pool, three sizes. Impact is B/vsol per leg, so the
        // haircut must grow with size — the whole point the retired flat-slippage
        // model missed.
        let m = CostModel::pumpfun_with_impact();
        let depth = Some(70.0);
        let small = round_trip_with_costs(1.0, 1.10, 0.1, depth, &m).1;
        let mid = round_trip_with_costs(1.0, 1.10, 0.27, depth, &m).1;
        let big = round_trip_with_costs(1.0, 1.10, 1.0, depth, &m).1;
        assert!(big < mid && mid < small, "bigger order must cost more: {small} {mid} {big}");

        // And the entry leg's markup is exactly B/vsol: 1 SOL into 70 SOL = 1.43%.
        let free = CostModel { fee_bps_per_leg: 0.0, fixed_cost_sol_per_leg: 0.0, ..m };
        let (_, pct) = round_trip_with_costs(1.0, 1.0, 1.0, Some(70.0), &free);
        // entry paid ×(1+1/70), exit received ×(1−1/70) ⇒ ≈ −2×1/70.
        assert!((pct - (-100.0 * (2.0 / 70.0))).abs() < 0.05, "got {pct}");
    }

    #[test]
    fn price_impact_is_inert_without_depth_or_without_the_flag() {
        // Depth unknown ⇒ degrades to fee-only, never a silent divide-by-zero.
        let m = CostModel::pumpfun_with_impact();
        let none = round_trip_with_costs(1.0, 1.10, 1.0, None, &m).1;
        let zero = round_trip_with_costs(1.0, 1.10, 1.0, Some(0.0), &m).1;
        let neg = round_trip_with_costs(1.0, 1.10, 1.0, Some(-5.0), &m).1;
        assert!((none - zero).abs() < 1e-12 && (none - neg).abs() < 1e-12);
        assert!(none.is_finite());

        // The size-blind kind ignores depth entirely, so a caller who happens to
        // have depth in hand cannot change what it charges.
        let blind = CostModel::pumpfun_fee_only();
        let a = round_trip_with_costs(1.0, 1.10, 1.0, None, &blind).1;
        let b = round_trip_with_costs(1.0, 1.10, 1.0, Some(70.0), &blind).1;
        assert!((a - b).abs() < 1e-12, "size-blind model must be depth-blind");
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
        let a = round_trip_with_costs(1.0, 1.5, 1.0, None, &CostModelKind::PumpfunImpact.model());
        let b = round_trip_with_costs(1.0, 1.5, 1.0, None, &CostModelKind::PumpfunFeeOnly.model());
        assert!((a.0 - b.0).abs() < 1e-12, "a size-blind kind must be the fee-only kind");

        // …and with depth, the ONLY thing that separates them is our own footprint.
        let with = round_trip_with_costs(1.0, 1.5, 1.0, Some(70.0), &CostModelKind::PumpfunImpact.model());
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
        assert!((c.fixed_cost_sol_per_leg - cheap.fixed_cost_sol_per_leg()).abs() < 1e-15);
        assert!(d.fixed_cost_sol_per_leg > c.fixed_cost_sol_per_leg);
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

    // ── wallet_mint_pnl (Trader Analysis) ───────────────────────────────────

    #[test]
    fn wallet_pnl_fully_closed_round_trip() {
        // Bought 100 tokens for 1 SOL (0.01/token), sold all 100 for 1.5 SOL.
        let p = wallet_mint_pnl(1.0, 1.5, 100, 100, None);
        assert_eq!(p.avg_buy_price, Some(0.01));
        assert_eq!(p.avg_sell_price, Some(0.015));
        assert_eq!(p.net_token_amount, 0);
        assert!(!p.is_open);
        assert!(!p.partial_data);
        assert!((p.realized_pnl_sol - 0.5).abs() < 1e-12);
        assert!((p.realized_pnl_pct.unwrap() - 50.0).abs() < 1e-9);
        // Net of the 125bps/leg fee: 1.5*(1-0.0125) - 1.0*(1+0.0125) = 1.48125 - 1.0125.
        assert!((p.realized_pnl_sol_net_of_fee - (1.5 * 0.9875 - 1.0 * 1.0125)).abs() < 1e-9);
        assert!(p.realized_pnl_sol_net_of_fee < p.realized_pnl_sol, "fee must cost something");
        assert_eq!(p.unrealized_pnl_sol, None);
        assert!((p.total_pnl_sol - p.realized_pnl_sol).abs() < 1e-12);
    }

    #[test]
    fn wallet_pnl_still_open_marks_to_market() {
        // Bought 100 for 1 SOL, sold none; current price is 2x the entry.
        let p = wallet_mint_pnl(1.0, 0.0, 100, 0, Some(0.02));
        assert_eq!(p.net_token_amount, 100);
        assert!(p.is_open);
        assert!(!p.partial_data);
        assert_eq!(p.avg_sell_price, None);
        // Nothing matched yet, so realized is flat.
        assert!((p.realized_pnl_sol - 0.0).abs() < 1e-12);
        assert_eq!(p.realized_pnl_pct, None);
        // Unrealized: 100 * (0.02 - 0.01) = 1.0 SOL.
        assert!((p.unrealized_pnl_sol.unwrap() - 1.0).abs() < 1e-12);
        assert!((p.total_pnl_sol - 1.0).abs() < 1e-12);
    }

    #[test]
    fn wallet_pnl_open_without_current_price_has_no_unrealized() {
        let p = wallet_mint_pnl(1.0, 0.0, 100, 0, None);
        assert!(p.is_open);
        assert_eq!(p.unrealized_pnl_sol, None, "unknown price must not fabricate a mark");
        assert!((p.total_pnl_sol - 0.0).abs() < 1e-12, "total falls back to realized-only");
    }

    #[test]
    fn wallet_pnl_partially_closed_matches_only_the_sold_tokens() {
        // Bought 100 for 1 SOL, sold only 40 for 0.8 SOL (2x). The other 60 stay
        // an open bag, not folded into the realized figure.
        let p = wallet_mint_pnl(1.0, 0.8, 100, 40, Some(0.02));
        assert_eq!(p.net_token_amount, 60);
        assert!(p.is_open);
        assert!(!p.partial_data);
        // Cost basis of the 40 matched tokens = 40 * 0.01 = 0.4 SOL.
        assert!((p.realized_pnl_sol - (0.8 - 0.4)).abs() < 1e-12);
        // Unrealized on the remaining 60: 60 * (0.02 - 0.01) = 0.6 SOL.
        assert!((p.unrealized_pnl_sol.unwrap() - 0.6).abs() < 1e-12);
        assert!((p.total_pnl_sol - (0.4 + 0.6)).abs() < 1e-12);
    }

    #[test]
    fn wallet_pnl_oversold_vs_window_flags_partial_data() {
        // Sold 100 tokens but the window only saw a 60-token buy — the other 40
        // tokens' cost basis predates the window. Only the matched 60 count.
        let p = wallet_mint_pnl(0.6, 1.5, 60, 100, None);
        assert!(p.partial_data, "selling more than bought in-window must be flagged");
        assert_eq!(p.net_token_amount, -40);
        assert!(!p.is_open, "a negative net amount is not an open bag");
        // Proceeds apportioned to the matched 60/100 of the sale: 1.5 * 0.6 = 0.9.
        // Cost basis of the matched 60 = 60 * 0.01 = 0.6.
        assert!((p.realized_pnl_sol - (0.9 - 0.6)).abs() < 1e-9);
        assert_eq!(p.unrealized_pnl_sol, None, "no open bag to mark");
    }

    #[test]
    fn wallet_pnl_no_buys_in_window_has_no_avg_buy_price_or_realized_pct() {
        // A mint only sold in the window (opening buy predates `since`).
        let p = wallet_mint_pnl(0.0, 1.0, 0, 50, Some(0.01));
        assert_eq!(p.avg_buy_price, None);
        assert!(p.partial_data);
        assert_eq!(p.realized_pnl_pct, None, "zero cost basis must not divide by zero");
        assert!((p.realized_pnl_sol - 0.0).abs() < 1e-12, "no matched cost basis to net against");
    }

    #[test]
    fn wallet_pnl_no_trades_is_all_zero_and_closed() {
        let p = wallet_mint_pnl(0.0, 0.0, 0, 0, None);
        assert!(!p.is_open);
        assert!(!p.partial_data);
        assert_eq!(p.avg_buy_price, None);
        assert_eq!(p.avg_sell_price, None);
        assert_eq!(p.realized_pnl_pct, None);
        assert!((p.total_pnl_sol - 0.0).abs() < 1e-12);
    }
}
