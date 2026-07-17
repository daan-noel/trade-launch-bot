//! The swing1 [`Strategy`] impl. Like tpsl2 it wraps the **same** pure entry/exit
//! fns the live and DB-backtest paths use — `swing_1::entry::find_phase_entry`
//! and `swing_1::exit::find_trade_driven_exit_with_slot` — so the sweep resolves
//! byte-identical entry/exit *decisions* to live trading. PnL is the frictionless
//! `round_trip` of the decision prices.
//!
//! swing1 has **no** token-creation gate (the dev-fingerprint filter is applied
//! upstream), and no per-token precompute — the swing scan is the only per-token
//! work and it depends on the swing/kill/volume **entry** axes, so it runs once
//! per distinct [`Swing1EntryKey`] in `resolve_entry` (cached across that key's
//! exit sub-grid by the engine).
//!
//! Axis layout (mirrors the [`Swing1Rule`] fields):
//!   • exit ladder (exit-only): take_profit, stop_loss, trailing, stall, time, liq
//!   • swing detection (entry): reversal thresholds + min leg trades
//!   • kill-low profile (entry): depth/duration/net-flow
//!   • volume-low profile + transition floor (entry)
//!   • entry confirmation (entry): pullback / higher-low secs / max age + guards
//!   • symmetric next-kill exit (exit-only): depth/duration
//!
//! The next-kill exit axes are **exit-only** (they flee, they don't move the
//! entry), so they stay out of [`Swing1EntryKey`].

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::models::Swing1Rule;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::{
    index_of, lhs_index_plan, neighbor_indices, round_trip_with_costs, CostModel, ExitCode,
    ParamSpace, Strategy, SweepMethod, TokenOutcome,
};
use trading_core::strategies::rules::swing1_shape_sane;
use trading_core::strategies::swing_1::{entry, exit};

/// Drop combos that violate the swing1 shape-sanity invariants — the **same**
/// [`swing1_shape_sane`] predicate rule validation uses (SSOT), so the sweep can
/// never surface a "winner" you'd fail to save as a rule. Such a combo also has a
/// degenerate phase classification (overlapping kill/volume depth or duration
/// bands, so a flush and accumulation are indistinguishable), so its PnL is
/// meaningless regardless of saveability. Logs the pruned count (no silent caps).
fn prune_shape_invalid(combos: Vec<Swing1Combo>) -> Vec<Swing1Combo> {
    let before = combos.len();
    let kept: Vec<_> = combos
        .into_iter()
        .filter(|c| {
            swing1_shape_sane(
                c.raw.kill_depth_min_pct,
                c.raw.vol_depth_max_pct,
                c.raw.kill_max_duration_ms,
                c.raw.vol_min_duration_ms,
            )
            .is_ok()
        })
        .collect();
    if kept.len() < before {
        tracing::info!(
            pruned = before - kept.len(),
            kept = kept.len(),
            "swing1 sweep: dropped shape-invalid combos (kill/volume depth or duration bands overlap)"
        );
    }
    kept
}

/// The full swing1 rule param set, every knob swept. `take_profit`/`stop_loss` are
/// always-on; every other knob is `Option` and a `None` (the default axis when the
/// page leaves it blank, or an explicit `off`) means "unbounded / inert" — the gate
/// is disabled and the live pure fns ignore it (same as `0`). `buy_amount_sol` and the
/// non-param rule fields stay inherited from the base rule template.
#[derive(Clone, Copy, Debug)]
pub struct Swing1Params {
    // ── Exit ladder (exit-only) ───────────────────────────────────────────────
    pub take_profit: f64,
    pub stop_loss: f64,
    pub trailing_stop_pct: Option<f64>,
    pub time_stop_secs: Option<u64>,
    pub stall_secs: Option<u64>,
    pub liquidity_drop_pct: Option<f64>,
    // ── Swing detection (entry) ───────────────────────────────────────────────
    pub swing_high_to_low_sol: Option<f64>,
    pub swing_high_to_low_pct: Option<f64>,
    pub swing_low_to_high_sol: Option<f64>,
    pub swing_low_to_high_pct: Option<f64>,
    pub swing_min_leg_trades: Option<u32>,
    pub dust_frac: Option<f64>,
    // ── Kill-low profile (entry) ──────────────────────────────────────────────
    pub kill_depth_min_pct: Option<f64>,
    pub kill_max_duration_ms: Option<i64>,
    pub kill_min_net_flow_per_sec: Option<f64>,
    // ── Volume-low profile + transition floor (entry) ─────────────────────────
    pub vol_depth_max_pct: Option<f64>,
    pub vol_min_duration_ms: Option<i64>,
    pub vol_min_up_duration_ms: Option<i64>,
    pub min_kills_before_volume: Option<u32>,
    // ── Entry confirmation (entry) ────────────────────────────────────────────
    pub entry_pullback_pct: Option<f64>,
    pub entry_higher_low_secs: Option<u64>,
    pub entry_max_age_secs: Option<u64>,
    pub entry_min_liquidity_sol: Option<f64>,
    // ── Symmetric next-kill exit (exit-only) ──────────────────────────────────
    pub exit_next_kill_depth_min_pct: Option<f64>,
    pub exit_next_kill_max_duration_ms: Option<i64>,
}

/// One evaluated combo: the `Copy` scalar params (for `entry_key`/`params_json`)
/// paired with the [`Swing1Rule`] they overlay onto the base, **resolved once at
/// sample time**. Mirrors tpsl2's per-combo rule build — handing the entry/exit fns
/// `&combo.rule` keeps their `&Swing1Rule` signatures, so the live path is
/// untouched and decision parity is exact.
#[derive(Clone)]
pub struct Swing1Combo {
    pub raw: Swing1Params,
    pub rule: Swing1Rule,
}

/// The entry-param identity of a combo: every knob the swing scan + latch +
/// higher-low confirmation read, and nothing else. Two combos with equal keys
/// resolve the same entry on any token, so the engine resolves the entry once per
/// distinct key. The exit knobs (TP/SL/trailing/stall/time/liq + next-kill) are
/// deliberately absent — the entry never depends on them.
#[derive(Clone, Copy, PartialEq)]
pub struct Swing1EntryKey {
    swing_high_to_low_sol: Option<f64>,
    swing_high_to_low_pct: Option<f64>,
    swing_low_to_high_sol: Option<f64>,
    swing_low_to_high_pct: Option<f64>,
    swing_min_leg_trades: Option<u32>,
    dust_frac: Option<f64>,
    kill_depth_min_pct: Option<f64>,
    kill_max_duration_ms: Option<i64>,
    kill_min_net_flow_per_sec: Option<f64>,
    vol_depth_max_pct: Option<f64>,
    vol_min_duration_ms: Option<i64>,
    vol_min_up_duration_ms: Option<i64>,
    min_kills_before_volume: Option<u32>,
    entry_pullback_pct: Option<f64>,
    entry_higher_low_secs: Option<u64>,
    entry_max_age_secs: Option<u64>,
    entry_min_liquidity_sol: Option<f64>,
}

/// The resolved entry for a token under one entry-param tuple: either no entry, or
/// the worst-case paper fill price + time. Cached by the engine and reused across
/// the tuple's whole exit sub-grid.
#[derive(Clone, Copy)]
pub enum Swing1Entry {
    None,
    Entered { price: f64, time: DateTime<Utc>, slot: u64 },
}

/// Declares the param axes and carries the base rule the swept params overlay.
pub struct Swing1Strategy {
    base: Swing1Rule,
    axes: Swing1Axes,
    /// Execution costs applied to every simulated round-trip.
    costs: CostModel,
    /// The analysis "present" for the death-close quiet-clock — the sweep/simulate
    /// **run time** (`Utc::now()`), captured once so it's uniform across the whole run
    /// (matching live/paper, which judge deadness against wall-clock `now`). `None` ⇒
    /// fall back to each token's own last trade (the pre-fix per-token behavior, kept
    /// only for unit tests that don't set it). Over the sealed-days lake `now` is
    /// always ≫ every last trade, so a token that stopped trading is booked `Dead`,
    /// not `Open` — see
    /// [`trading_core::strategies::swing_1::exit::find_trade_driven_exit_as_of`].
    as_of: Option<DateTime<Utc>>,
}

/// Grid axes for the coarse pass. Each `Vec` is one knob's candidate values.
/// `Serialize` so the resolved axes (after [`Swing1Axes::from_spec`]) are stored
/// verbatim on the sweep run for the UI to echo back / re-run.
#[derive(Clone, Serialize)]
pub struct Swing1Axes {
    pub take_profit: Vec<f64>,
    pub stop_loss: Vec<f64>,
    pub trailing_stop_pct: Vec<Option<f64>>,
    pub time_stop_secs: Vec<Option<u64>>,
    pub stall_secs: Vec<Option<u64>>,
    pub liquidity_drop_pct: Vec<Option<f64>>,
    pub swing_high_to_low_sol: Vec<Option<f64>>,
    pub swing_high_to_low_pct: Vec<Option<f64>>,
    pub swing_low_to_high_sol: Vec<Option<f64>>,
    pub swing_low_to_high_pct: Vec<Option<f64>>,
    pub swing_min_leg_trades: Vec<Option<u32>>,
    pub dust_frac: Vec<Option<f64>>,
    pub kill_depth_min_pct: Vec<Option<f64>>,
    pub kill_max_duration_ms: Vec<Option<i64>>,
    pub kill_min_net_flow_per_sec: Vec<Option<f64>>,
    pub vol_depth_max_pct: Vec<Option<f64>>,
    pub vol_min_duration_ms: Vec<Option<i64>>,
    pub vol_min_up_duration_ms: Vec<Option<i64>>,
    pub min_kills_before_volume: Vec<Option<u32>>,
    pub entry_pullback_pct: Vec<Option<f64>>,
    pub entry_higher_low_secs: Vec<Option<u64>>,
    pub entry_max_age_secs: Vec<Option<u64>>,
    pub entry_min_liquidity_sol: Vec<Option<f64>>,
    pub exit_next_kill_depth_min_pct: Vec<Option<f64>>,
    pub exit_next_kill_max_duration_ms: Vec<Option<i64>>,
}

impl Default for Swing1Axes {
    fn default() -> Self {
        // A focused default grid: a few high-leverage exit + entry knobs carry a
        // real candidate list; every other axis defaults to a single `[None]`
        // ("inert") so the grid stays small until the page supplies values. The
        // ~25-axis space MUST be swept with LHS/refine, never a full grid (see the
        // plan's grid-explosion mitigation), and the entry needs at least a
        // pullback + a volume bound configured to ever fire.
        Self {
            take_profit: vec![50.0, 100.0, 200.0],
            stop_loss: vec![30.0, 50.0],
            trailing_stop_pct: vec![None, Some(25.0)],
            time_stop_secs: vec![None],
            stall_secs: vec![None, Some(60)],
            liquidity_drop_pct: vec![None],
            // Swing reversal: a single pct threshold splits legs; SOL thresholds off.
            swing_high_to_low_sol: vec![None],
            swing_high_to_low_pct: vec![Some(20.0)],
            swing_low_to_high_sol: vec![None],
            swing_low_to_high_pct: vec![Some(20.0)],
            swing_min_leg_trades: vec![None],
            // Dust floor OFF by default (only literally-zero trades dropped).
            dust_frac: vec![None],
            // Kill profile: deep + short.
            kill_depth_min_pct: vec![Some(0.6)],
            kill_max_duration_ms: vec![Some(8_000)],
            kill_min_net_flow_per_sec: vec![None],
            // Volume profile: shallow + long, with real accumulation on the up-leg.
            vol_depth_max_pct: vec![Some(0.4)],
            vol_min_duration_ms: vec![Some(10_000)],
            vol_min_up_duration_ms: vec![None],
            // Count-free transition floor (0 = kill-phase gate adds no edge).
            min_kills_before_volume: vec![Some(1)],
            // Entry confirmation: pullback + higher-low window.
            entry_pullback_pct: vec![Some(10.0)],
            entry_higher_low_secs: vec![None],
            entry_max_age_secs: vec![None],
            entry_min_liquidity_sol: vec![None],
            // Symmetric next-kill flee (separate thresholds from the entry kill_*).
            exit_next_kill_depth_min_pct: vec![None, Some(0.6)],
            exit_next_kill_max_duration_ms: vec![Some(8_000)],
        }
    }
}

/// The page-editable grid: one optional candidate list per swept knob. An omitted
/// **or empty** axis falls back to that axis's hardcoded [`Swing1Axes::default`]
/// value, so a partial grid is valid. `null` inside a nullable axis is the
/// "disabled" option for that knob.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AxesSpec {
    #[serde(default)]
    pub take_profit: Option<Vec<f64>>,
    #[serde(default)]
    pub stop_loss: Option<Vec<f64>>,
    #[serde(default)]
    pub trailing_stop_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub time_stop_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub stall_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub liquidity_drop_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub swing_high_to_low_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub swing_high_to_low_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub swing_low_to_high_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub swing_low_to_high_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub swing_min_leg_trades: Option<Vec<Option<u32>>>,
    #[serde(default)]
    pub dust_frac: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub kill_depth_min_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub kill_max_duration_ms: Option<Vec<Option<i64>>>,
    #[serde(default)]
    pub kill_min_net_flow_per_sec: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub vol_depth_max_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub vol_min_duration_ms: Option<Vec<Option<i64>>>,
    #[serde(default)]
    pub vol_min_up_duration_ms: Option<Vec<Option<i64>>>,
    #[serde(default)]
    pub min_kills_before_volume: Option<Vec<Option<u32>>>,
    #[serde(default)]
    pub entry_pullback_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_higher_low_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub entry_max_age_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub entry_min_liquidity_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub exit_next_kill_depth_min_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub exit_next_kill_max_duration_ms: Option<Vec<Option<i64>>>,
}

impl Swing1Axes {
    /// Build axes from a page-supplied [`AxesSpec`], falling back to the default for
    /// any omitted/empty axis. Dedups each axis (the grid product shouldn't
    /// double-count a repeated value the UI may submit).
    pub fn from_spec(spec: &AxesSpec) -> Self {
        fn axis<T: Clone + PartialEq>(supplied: &Option<Vec<T>>, default: Vec<T>) -> Vec<T> {
            match supplied {
                Some(v) if !v.is_empty() => {
                    let mut out: Vec<T> = Vec::with_capacity(v.len());
                    for x in v {
                        if !out.contains(x) {
                            out.push(x.clone());
                        }
                    }
                    out
                }
                _ => default,
            }
        }
        let d = Swing1Axes::default();
        Self {
            take_profit: axis(&spec.take_profit, d.take_profit),
            stop_loss: axis(&spec.stop_loss, d.stop_loss),
            trailing_stop_pct: axis(&spec.trailing_stop_pct, d.trailing_stop_pct),
            time_stop_secs: axis(&spec.time_stop_secs, d.time_stop_secs),
            stall_secs: axis(&spec.stall_secs, d.stall_secs),
            liquidity_drop_pct: axis(&spec.liquidity_drop_pct, d.liquidity_drop_pct),
            swing_high_to_low_sol: axis(&spec.swing_high_to_low_sol, d.swing_high_to_low_sol),
            swing_high_to_low_pct: axis(&spec.swing_high_to_low_pct, d.swing_high_to_low_pct),
            swing_low_to_high_sol: axis(&spec.swing_low_to_high_sol, d.swing_low_to_high_sol),
            swing_low_to_high_pct: axis(&spec.swing_low_to_high_pct, d.swing_low_to_high_pct),
            swing_min_leg_trades: axis(&spec.swing_min_leg_trades, d.swing_min_leg_trades),
            dust_frac: axis(&spec.dust_frac, d.dust_frac),
            kill_depth_min_pct: axis(&spec.kill_depth_min_pct, d.kill_depth_min_pct),
            kill_max_duration_ms: axis(&spec.kill_max_duration_ms, d.kill_max_duration_ms),
            kill_min_net_flow_per_sec: axis(
                &spec.kill_min_net_flow_per_sec,
                d.kill_min_net_flow_per_sec,
            ),
            vol_depth_max_pct: axis(&spec.vol_depth_max_pct, d.vol_depth_max_pct),
            vol_min_duration_ms: axis(&spec.vol_min_duration_ms, d.vol_min_duration_ms),
            vol_min_up_duration_ms: axis(&spec.vol_min_up_duration_ms, d.vol_min_up_duration_ms),
            min_kills_before_volume: axis(
                &spec.min_kills_before_volume,
                d.min_kills_before_volume,
            ),
            entry_pullback_pct: axis(&spec.entry_pullback_pct, d.entry_pullback_pct),
            entry_higher_low_secs: axis(&spec.entry_higher_low_secs, d.entry_higher_low_secs),
            entry_max_age_secs: axis(&spec.entry_max_age_secs, d.entry_max_age_secs),
            entry_min_liquidity_sol: axis(
                &spec.entry_min_liquidity_sol,
                d.entry_min_liquidity_sol,
            ),
            exit_next_kill_depth_min_pct: axis(
                &spec.exit_next_kill_depth_min_pct,
                d.exit_next_kill_depth_min_pct,
            ),
            exit_next_kill_max_duration_ms: axis(
                &spec.exit_next_kill_max_duration_ms,
                d.exit_next_kill_max_duration_ms,
            ),
        }
    }

    /// The per-axis candidate-count list, in the **canonical axis order** every
    /// sampler decodes against (the mixed-radix `combo_at` order, the LHS plan
    /// column order, and the `refine`/`order_for_entry_cache` walks). Defined once
    /// so those stay in lockstep.
    fn axis_lens(&self) -> [usize; 25] {
        [
            self.take_profit.len(),
            self.stop_loss.len(),
            self.trailing_stop_pct.len(),
            self.time_stop_secs.len(),
            self.stall_secs.len(),
            self.liquidity_drop_pct.len(),
            self.swing_high_to_low_sol.len(),
            self.swing_high_to_low_pct.len(),
            self.swing_low_to_high_sol.len(),
            self.swing_low_to_high_pct.len(),
            self.swing_min_leg_trades.len(),
            self.dust_frac.len(),
            self.kill_depth_min_pct.len(),
            self.kill_max_duration_ms.len(),
            self.kill_min_net_flow_per_sec.len(),
            self.vol_depth_max_pct.len(),
            self.vol_min_duration_ms.len(),
            self.vol_min_up_duration_ms.len(),
            self.min_kills_before_volume.len(),
            self.entry_pullback_pct.len(),
            self.entry_higher_low_secs.len(),
            self.entry_max_age_secs.len(),
            self.entry_min_liquidity_sol.len(),
            self.exit_next_kill_depth_min_pct.len(),
            self.exit_next_kill_max_duration_ms.len(),
        ]
    }

    /// Number of combos a full grid over these axes yields (product of lengths).
    /// The handler rejects a spec whose product exceeds the combo cap.
    pub fn combo_count(&self) -> usize {
        self.axis_lens().iter().product()
    }

    /// Grid size as `u128` — the index space the random sampler draws **without
    /// replacement** from. `u128` (not `combo_count`'s `usize`) so a wide page-
    /// supplied grid can't overflow the product before the draw clamps to it.
    fn grid_total_u128(&self) -> u128 {
        self.axis_lens().iter().map(|&l| l as u128).product()
    }
}

impl Swing1Strategy {
    pub fn new(base: Swing1Rule, axes: Swing1Axes) -> Self {
        Self { base, axes, costs: CostModel::pumpfun_default(), as_of: None }
    }

    /// Minimal strategy for re-simulating a single stored combo (drill-in endpoint).
    pub fn for_replay(base: Swing1Rule) -> Self {
        Self {
            base,
            axes: Swing1Axes::default(),
            costs: CostModel::pumpfun_default(),
            as_of: None,
        }
    }

    /// Set the death-close "present" — the run's wall-clock `now` (see
    /// [`Swing1Strategy::as_of`]). Called once by the driver so deadness is judged
    /// against run time (like live), uniformly across the whole run.
    pub fn with_as_of(mut self, as_of: DateTime<Utc>) -> Self {
        self.as_of = Some(as_of);
        self
    }

    /// Pair a scalar param set with its resolved [`Swing1Rule`] (built once here,
    /// not per token in the hot loop — see [`Swing1Combo`]).
    fn combo(&self, raw: Swing1Params) -> Swing1Combo {
        Swing1Combo { rule: self.rule_from(&raw), raw }
    }

    /// Decode a flat grid index into its combo by mixed-radix over the axis lengths
    /// (the low-order digit is `take_profit`, matching the LHS plan's column order).
    /// Shared by the full-grid and the random (without-replacement) samplers so the
    /// two index the **same** grid identically.
    // The `take` macro advances `rem` after every axis, including the last whose
    // result is never re-read — an intentional dead store, not a bug.
    #[allow(unused_assignments)]
    fn combo_at(&self, index: u128) -> Swing1Combo {
        let a = &self.axes;
        let mut rem = index;
        macro_rules! take {
            ($axis:expr) => {{
                let xs = &$axis;
                let v = xs[(rem % xs.len() as u128) as usize];
                rem /= xs.len() as u128;
                v
            }};
        }
        self.combo(Swing1Params {
            take_profit: take!(a.take_profit),
            stop_loss: take!(a.stop_loss),
            trailing_stop_pct: take!(a.trailing_stop_pct),
            time_stop_secs: take!(a.time_stop_secs),
            stall_secs: take!(a.stall_secs),
            liquidity_drop_pct: take!(a.liquidity_drop_pct),
            swing_high_to_low_sol: take!(a.swing_high_to_low_sol),
            swing_high_to_low_pct: take!(a.swing_high_to_low_pct),
            swing_low_to_high_sol: take!(a.swing_low_to_high_sol),
            swing_low_to_high_pct: take!(a.swing_low_to_high_pct),
            swing_min_leg_trades: take!(a.swing_min_leg_trades),
            dust_frac: take!(a.dust_frac),
            kill_depth_min_pct: take!(a.kill_depth_min_pct),
            kill_max_duration_ms: take!(a.kill_max_duration_ms),
            kill_min_net_flow_per_sec: take!(a.kill_min_net_flow_per_sec),
            vol_depth_max_pct: take!(a.vol_depth_max_pct),
            vol_min_duration_ms: take!(a.vol_min_duration_ms),
            vol_min_up_duration_ms: take!(a.vol_min_up_duration_ms),
            min_kills_before_volume: take!(a.min_kills_before_volume),
            entry_pullback_pct: take!(a.entry_pullback_pct),
            entry_higher_low_secs: take!(a.entry_higher_low_secs),
            entry_max_age_secs: take!(a.entry_max_age_secs),
            entry_min_liquidity_sol: take!(a.entry_min_liquidity_sol),
            exit_next_kill_depth_min_pct: take!(a.exit_next_kill_depth_min_pct),
            exit_next_kill_max_duration_ms: take!(a.exit_next_kill_max_duration_ms),
        })
    }

    /// Overlay one param set onto the base rule, producing the exact [`Swing1Rule`]
    /// the live pure fns expect.
    fn rule_from(&self, p: &Swing1Params) -> Swing1Rule {
        let mut r = self.base.clone();
        r.p_exit_take_profit = p.take_profit;
        r.p_exit_stop_loss = p.stop_loss;
        r.p_exit_trailing_stop_pct = p.trailing_stop_pct;
        r.p_exit_time_stop_secs = p.time_stop_secs;
        r.p_exit_stall_secs = p.stall_secs;
        r.p_exit_liquidity_drop_pct = p.liquidity_drop_pct;
        r.p_swing_high_to_low_sol = p.swing_high_to_low_sol;
        r.p_swing_high_to_low_pct = p.swing_high_to_low_pct;
        r.p_swing_low_to_high_sol = p.swing_low_to_high_sol;
        r.p_swing_low_to_high_pct = p.swing_low_to_high_pct;
        r.p_swing_min_leg_trades = p.swing_min_leg_trades;
        r.p_dust_frac = p.dust_frac;
        r.p_kill_depth_min_pct = p.kill_depth_min_pct;
        r.p_kill_max_duration_ms = p.kill_max_duration_ms;
        r.p_kill_min_net_flow_per_sec = p.kill_min_net_flow_per_sec;
        r.p_vol_depth_max_pct = p.vol_depth_max_pct;
        r.p_vol_min_duration_ms = p.vol_min_duration_ms;
        r.p_vol_min_up_duration_ms = p.vol_min_up_duration_ms;
        r.p_min_kills_before_volume = p.min_kills_before_volume;
        r.p_entry_pullback_pct = p.entry_pullback_pct;
        r.p_entry_higher_low_secs = p.entry_higher_low_secs;
        r.p_entry_max_age_secs = p.entry_max_age_secs;
        r.p_entry_min_liquidity_sol = p.entry_min_liquidity_sol;
        r.p_exit_next_kill_depth_min_pct = p.exit_next_kill_depth_min_pct;
        r.p_exit_next_kill_max_duration_ms = p.exit_next_kill_max_duration_ms;
        r
    }

    /// Reconstruct a combo from the `params_json` stored in the results table.
    /// JSON keys match [`Strategy::params_json`]'s output.
    pub fn combo_from_params_json(&self, v: &serde_json::Value) -> anyhow::Result<Swing1Combo> {
        fn opt_f(v: &serde_json::Value, k: &str) -> Option<f64> {
            v.get(k).and_then(|x| x.as_f64())
        }
        fn opt_u(v: &serde_json::Value, k: &str) -> Option<u64> {
            v.get(k).and_then(|x| x.as_u64())
        }
        fn opt_u32(v: &serde_json::Value, k: &str) -> Option<u32> {
            v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32)
        }
        fn opt_i(v: &serde_json::Value, k: &str) -> Option<i64> {
            v.get(k).and_then(|x| x.as_i64())
        }
        let take_profit = v
            .get("exit_take_profit")
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("params_json missing 'exit_take_profit'"))?;
        let stop_loss = v
            .get("exit_stop_loss")
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("params_json missing 'exit_stop_loss'"))?;
        let raw = Swing1Params {
            take_profit,
            stop_loss,
            trailing_stop_pct: opt_f(v, "exit_trailing_stop_pct"),
            time_stop_secs: opt_u(v, "exit_time_stop_secs"),
            stall_secs: opt_u(v, "exit_stall_secs"),
            liquidity_drop_pct: opt_f(v, "exit_liquidity_drop_pct"),
            swing_high_to_low_sol: opt_f(v, "swing_high_to_low_sol"),
            swing_high_to_low_pct: opt_f(v, "swing_high_to_low_pct"),
            swing_low_to_high_sol: opt_f(v, "swing_low_to_high_sol"),
            swing_low_to_high_pct: opt_f(v, "swing_low_to_high_pct"),
            swing_min_leg_trades: opt_u32(v, "swing_min_leg_trades"),
            dust_frac: opt_f(v, "dust_frac"),
            kill_depth_min_pct: opt_f(v, "kill_depth_min_pct"),
            kill_max_duration_ms: opt_i(v, "kill_max_duration_ms"),
            kill_min_net_flow_per_sec: opt_f(v, "kill_min_net_flow_per_sec"),
            vol_depth_max_pct: opt_f(v, "vol_depth_max_pct"),
            vol_min_duration_ms: opt_i(v, "vol_min_duration_ms"),
            vol_min_up_duration_ms: opt_i(v, "vol_min_up_duration_ms"),
            min_kills_before_volume: opt_u32(v, "min_kills_before_volume"),
            entry_pullback_pct: opt_f(v, "entry_pullback_pct"),
            entry_higher_low_secs: opt_u(v, "entry_higher_low_secs"),
            entry_max_age_secs: opt_u(v, "entry_max_age_secs"),
            entry_min_liquidity_sol: opt_f(v, "entry_min_liquidity_sol"),
            exit_next_kill_depth_min_pct: opt_f(v, "exit_next_kill_depth_min_pct"),
            exit_next_kill_max_duration_ms: opt_i(v, "exit_next_kill_max_duration_ms"),
        };
        Ok(self.combo(raw))
    }
}

impl ParamSpace for Swing1Strategy {
    type Params = Swing1Combo;

    // The LHS `take` macro advances `col` after every axis, including the last whose
    // result is never re-read — an intentional dead store, not a bug.
    #[allow(unused_assignments)]
    fn sample(&self, method: SweepMethod) -> Vec<Swing1Combo> {
        let a = &self.axes;
        let combos = match method {
            SweepMethod::Grid => {
                // Full grid = Cartesian product of every axis, one combo per flat
                // index (mixed-radix decode in `combo_at`). Pre-checked ≤ cap.
                (0..a.combo_count() as u128).map(|idx| self.combo_at(idx)).collect()
            }
            SweepMethod::Random { n, seed } => {
                // Draw `n` combos **without replacement** from the grid index space.
                let total = a.grid_total_u128();
                let target = (n as u128).min(total) as usize;
                let mut rng = StdRng::seed_from_u64(seed);
                let mut seen: HashSet<u128> = HashSet::with_capacity(target);
                let mut out = Vec::with_capacity(target);
                while out.len() < target {
                    let idx = rng.gen_range(0..total);
                    if seen.insert(idx) {
                        out.push(self.combo_at(idx));
                    }
                }
                if target < n {
                    tracing::info!(
                        requested = n,
                        distinct = target,
                        "random sweep: grid smaller than N — sampled all distinct combos"
                    );
                }
                out
            }
            SweepMethod::LatinHypercube { n, seed } => {
                // Real LHS over the discrete axes (balanced + permuted strata). The
                // `lens` order MUST match the `take!` calls below and `axis_lens`.
                let mut rng = StdRng::seed_from_u64(seed);
                let lens = a.axis_lens();
                let plan = lhs_index_plan(&mut rng, n, &lens);
                (0..n)
                    .map(|i| {
                        let mut col = 0usize;
                        macro_rules! take {
                            ($axis:expr) => {{
                                let v = $axis[plan[col][i]];
                                col += 1;
                                v
                            }};
                        }
                        self.combo(Swing1Params {
                            take_profit: take!(a.take_profit),
                            stop_loss: take!(a.stop_loss),
                            trailing_stop_pct: take!(a.trailing_stop_pct),
                            time_stop_secs: take!(a.time_stop_secs),
                            stall_secs: take!(a.stall_secs),
                            liquidity_drop_pct: take!(a.liquidity_drop_pct),
                            swing_high_to_low_sol: take!(a.swing_high_to_low_sol),
                            swing_high_to_low_pct: take!(a.swing_high_to_low_pct),
                            swing_low_to_high_sol: take!(a.swing_low_to_high_sol),
                            swing_low_to_high_pct: take!(a.swing_low_to_high_pct),
                            swing_min_leg_trades: take!(a.swing_min_leg_trades),
                            dust_frac: take!(a.dust_frac),
                            kill_depth_min_pct: take!(a.kill_depth_min_pct),
                            kill_max_duration_ms: take!(a.kill_max_duration_ms),
                            kill_min_net_flow_per_sec: take!(a.kill_min_net_flow_per_sec),
                            vol_depth_max_pct: take!(a.vol_depth_max_pct),
                            vol_min_duration_ms: take!(a.vol_min_duration_ms),
                            vol_min_up_duration_ms: take!(a.vol_min_up_duration_ms),
                            min_kills_before_volume: take!(a.min_kills_before_volume),
                            entry_pullback_pct: take!(a.entry_pullback_pct),
                            entry_higher_low_secs: take!(a.entry_higher_low_secs),
                            entry_max_age_secs: take!(a.entry_max_age_secs),
                            entry_min_liquidity_sol: take!(a.entry_min_liquidity_sol),
                            exit_next_kill_depth_min_pct: take!(a.exit_next_kill_depth_min_pct),
                            exit_next_kill_max_duration_ms: take!(
                                a.exit_next_kill_max_duration_ms
                            ),
                        })
                    })
                    .collect()
            }
        };
        prune_shape_invalid(combos)
    }

    /// Coordinate-move neighborhood: for each survivor, vary one axis at a time to
    /// its adjacent candidate value(s), holding the others fixed. `Swing1Params` is
    /// `Copy`, so each neighbor is a cheap field overwrite. Duplicates are fine —
    /// the grouped driver dedups by `params_json`.
    fn refine(&self, survivors: &[Swing1Combo]) -> Vec<Swing1Combo> {
        let a = &self.axes;
        let mut out = Vec::new();
        for s in survivors {
            let p = s.raw;
            macro_rules! walk {
                ($axis:expr, $field:ident) => {{
                    let xs = &$axis;
                    if let Some(i) = index_of(xs, &p.$field) {
                        for ni in neighbor_indices(i, xs.len()) {
                            let mut np = p;
                            np.$field = xs[ni];
                            out.push(self.combo(np));
                        }
                    }
                }};
            }
            walk!(a.take_profit, take_profit);
            walk!(a.stop_loss, stop_loss);
            walk!(a.trailing_stop_pct, trailing_stop_pct);
            walk!(a.time_stop_secs, time_stop_secs);
            walk!(a.stall_secs, stall_secs);
            walk!(a.liquidity_drop_pct, liquidity_drop_pct);
            walk!(a.swing_high_to_low_sol, swing_high_to_low_sol);
            walk!(a.swing_high_to_low_pct, swing_high_to_low_pct);
            walk!(a.swing_low_to_high_sol, swing_low_to_high_sol);
            walk!(a.swing_low_to_high_pct, swing_low_to_high_pct);
            walk!(a.swing_min_leg_trades, swing_min_leg_trades);
            walk!(a.dust_frac, dust_frac);
            walk!(a.kill_depth_min_pct, kill_depth_min_pct);
            walk!(a.kill_max_duration_ms, kill_max_duration_ms);
            walk!(a.kill_min_net_flow_per_sec, kill_min_net_flow_per_sec);
            walk!(a.vol_depth_max_pct, vol_depth_max_pct);
            walk!(a.vol_min_duration_ms, vol_min_duration_ms);
            walk!(a.vol_min_up_duration_ms, vol_min_up_duration_ms);
            walk!(a.min_kills_before_volume, min_kills_before_volume);
            walk!(a.entry_pullback_pct, entry_pullback_pct);
            walk!(a.entry_higher_low_secs, entry_higher_low_secs);
            walk!(a.entry_max_age_secs, entry_max_age_secs);
            walk!(a.entry_min_liquidity_sol, entry_min_liquidity_sol);
            walk!(a.exit_next_kill_depth_min_pct, exit_next_kill_depth_min_pct);
            walk!(a.exit_next_kill_max_duration_ms, exit_next_kill_max_duration_ms);
        }
        prune_shape_invalid(out)
    }

    /// Stable-sort the combo set so combos sharing the entry-gate knobs land
    /// contiguously, restoring the engine's per-entry-key cache hit rate under
    /// `random`/`lhs`/`refine`. The entry resolve — a full swing scan + classify +
    /// higher-low confirmation — is the expensive half, so without this it re-runs
    /// ~once per combo on a shuffled order. Decision-neutral.
    fn order_for_entry_cache(&self, params: &mut [Swing1Combo]) {
        params.sort_by_cached_key(|c| entry_order_key(&c.raw));
    }
}

/// A total-order key over a combo's entry-gate knobs (the [`Swing1EntryKey`]
/// fields). Used only to group same-entry combos contiguously, so the encoding
/// only needs to be injective on the candidate values: `Option`s map a present
/// value to its bit pattern and `None` to a reserved sentinel. The axes never
/// carry `NaN`/`-0.0`, so equal bits ⟺ equal value ⟺ equal `entry_key`.
fn entry_order_key(p: &Swing1Params) -> [u64; 17] {
    // `+1` keeps `None` (0) distinct from `Some(0)`; real ms/secs never hit MAX.
    fn ou64(o: Option<u64>) -> u64 {
        o.map(|v| v.wrapping_add(1)).unwrap_or(0)
    }
    fn ou32(o: Option<u32>) -> u64 {
        o.map(|v| (v as u64).wrapping_add(1)).unwrap_or(0)
    }
    fn oi64(o: Option<i64>) -> u64 {
        // i64 candidates are non-negative durations; bias by +1 so None(0) is distinct.
        o.map(|v| (v as u64).wrapping_add(1)).unwrap_or(0)
    }
    fn of64(o: Option<f64>) -> u64 {
        o.map(f64::to_bits).unwrap_or(u64::MAX)
    }
    [
        of64(p.swing_high_to_low_sol),
        of64(p.swing_high_to_low_pct),
        of64(p.swing_low_to_high_sol),
        of64(p.swing_low_to_high_pct),
        ou32(p.swing_min_leg_trades),
        of64(p.dust_frac),
        of64(p.kill_depth_min_pct),
        oi64(p.kill_max_duration_ms),
        of64(p.kill_min_net_flow_per_sec),
        of64(p.vol_depth_max_pct),
        oi64(p.vol_min_duration_ms),
        oi64(p.vol_min_up_duration_ms),
        ou32(p.min_kills_before_volume),
        of64(p.entry_pullback_pct),
        ou64(p.entry_higher_low_secs),
        ou64(p.entry_max_age_secs),
        of64(p.entry_min_liquidity_sol),
    ]
}

impl Strategy for Swing1Strategy {
    type Entry = Swing1Entry;
    type EntryKey = Swing1EntryKey;
    /// swing1 has no param-independent per-token precompute; the swing scan
    /// depends on the entry axes and runs in `resolve_entry`.
    type TokenState = ();
    type BoundParams = ();

    fn entry_key(&self, params: &Swing1Combo) -> Swing1EntryKey {
        let p = &params.raw;
        Swing1EntryKey {
            swing_high_to_low_sol: p.swing_high_to_low_sol,
            swing_high_to_low_pct: p.swing_high_to_low_pct,
            swing_low_to_high_sol: p.swing_low_to_high_sol,
            swing_low_to_high_pct: p.swing_low_to_high_pct,
            swing_min_leg_trades: p.swing_min_leg_trades,
            dust_frac: p.dust_frac,
            kill_depth_min_pct: p.kill_depth_min_pct,
            kill_max_duration_ms: p.kill_max_duration_ms,
            kill_min_net_flow_per_sec: p.kill_min_net_flow_per_sec,
            vol_depth_max_pct: p.vol_depth_max_pct,
            vol_min_duration_ms: p.vol_min_duration_ms,
            vol_min_up_duration_ms: p.vol_min_up_duration_ms,
            min_kills_before_volume: p.min_kills_before_volume,
            entry_pullback_pct: p.entry_pullback_pct,
            entry_higher_low_secs: p.entry_higher_low_secs,
            entry_max_age_secs: p.entry_max_age_secs,
            entry_min_liquidity_sol: p.entry_min_liquidity_sol,
        }
    }

    fn prepare_token(&self, _token: &crate::sweep::corpus::CorpusToken) {}

    fn bind_param(&self, _params: &Swing1Combo) {}

    fn resolve_entry(
        &self,
        trades: &[CorpusTrade],
        _state: &(),
        _bound: &(),
        params: &Swing1Combo,
    ) -> Swing1Entry {
        // The shared pure entry: latch the kill→volume transition, take the first
        // volume-phase confirmed higher-low, worst-case fill. Identical decision to
        // live (same `find_phase_entry` over the `TradeRow` abstraction).
        match entry::find_phase_entry(trades, &params.rule) {
            Some((_, fill)) if fill.price > 0.0 => {
                Swing1Entry::Entered { price: fill.price, time: fill.block_time, slot: fill.slot }
            }
            _ => Swing1Entry::None,
        }
    }

    fn resolve_exit(
        &self,
        trades: &[CorpusTrade],
        _state: &(),
        _bound: &(),
        entry: &Swing1Entry,
        params: &Swing1Combo,
    ) -> TokenOutcome {
        let Swing1Entry::Entered { price: entry_price, time: entry_time, slot: entry_slot } = *entry
        else {
            return TokenOutcome::no_entry();
        };
        let rule = &params.rule;
        let notional = rule.buy_amount_sol;

        // Death-close "present": the run's wall-clock `now` (matching live), or this
        // token's own last trade when unset (unit tests only). Over the sealed lake
        // `now` ≫ every last trade, so a token that stopped trading is booked `Dead`.
        let as_of = self
            .as_of
            .unwrap_or_else(|| trading_core::strategies::death::analysis_as_of(trades, entry_time));
        match exit::find_trade_driven_exit_as_of(trades, entry_time, entry_price, rule, as_of) {
            Some(f) => {
                let (pnl_sol, pnl_percent) =
                    round_trip_with_costs(entry_price, f.price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: (f.block_time - entry_time).num_seconds(),
                    pnl_percent: pnl_percent as f32,
                    pnl_sol: pnl_sol as f32,
                    exit: ExitCode::from_reason(f.reason.as_str()),
                    entry_time: Some(entry_time),
                    entry_price: Some(entry_price),
                    entry_slot: Some(entry_slot),
                    exit_time: Some(f.block_time),
                    exit_price: Some(f.price),
                    exit_slot: Some(f.slot),
                }
            }
            None => {
                // Still open at end of history — mark unrealized PnL at last price.
                let last_price = trades.last().map(|t| t.price_per_token).unwrap_or(entry_price);
                let (pnl_sol, pnl_percent) =
                    round_trip_with_costs(entry_price, last_price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: 0,
                    pnl_percent: pnl_percent as f32,
                    pnl_sol: pnl_sol as f32,
                    exit: ExitCode::Open,
                    entry_time: Some(entry_time),
                    entry_price: Some(entry_price),
                    entry_slot: Some(entry_slot),
                    exit_time: None,
                    exit_price: None,
                    exit_slot: None,
                }
            }
        }
    }

    fn params_json(&self, params: &Swing1Combo) -> serde_json::Value {
        let p = &params.raw;
        serde_json::json!({
            "exit_take_profit": p.take_profit,
            "exit_stop_loss": p.stop_loss,
            "exit_trailing_stop_pct": p.trailing_stop_pct,
            "exit_time_stop_secs": p.time_stop_secs,
            "exit_stall_secs": p.stall_secs,
            "exit_liquidity_drop_pct": p.liquidity_drop_pct,
            "swing_high_to_low_sol": p.swing_high_to_low_sol,
            "swing_high_to_low_pct": p.swing_high_to_low_pct,
            "swing_low_to_high_sol": p.swing_low_to_high_sol,
            "swing_low_to_high_pct": p.swing_low_to_high_pct,
            "swing_min_leg_trades": p.swing_min_leg_trades,
            "dust_frac": p.dust_frac,
            "kill_depth_min_pct": p.kill_depth_min_pct,
            "kill_max_duration_ms": p.kill_max_duration_ms,
            "kill_min_net_flow_per_sec": p.kill_min_net_flow_per_sec,
            "vol_depth_max_pct": p.vol_depth_max_pct,
            "vol_min_duration_ms": p.vol_min_duration_ms,
            "vol_min_up_duration_ms": p.vol_min_up_duration_ms,
            "min_kills_before_volume": p.min_kills_before_volume,
            "entry_pullback_pct": p.entry_pullback_pct,
            "entry_higher_low_secs": p.entry_higher_low_secs,
            "entry_max_age_secs": p.entry_max_age_secs,
            "entry_min_liquidity_sol": p.entry_min_liquidity_sol,
            "exit_next_kill_depth_min_pct": p.exit_next_kill_depth_min_pct,
            "exit_next_kill_max_duration_ms": p.exit_next_kill_max_duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy() -> Swing1Strategy {
        let base = Swing1Rule::new(
            "test".into(),
            None,
            None,
            None,
            serde_json::json!([]),
            "paper".into(),
            1.0,
            50.0,
            30.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        Swing1Strategy::new(base, Swing1Axes::default())
    }

    #[test]
    fn random_samples_without_replacement() {
        let s = strategy();
        let grid = s.axes.combo_count();
        let distinct = |combos: &[Swing1Combo]| -> usize {
            combos
                .iter()
                .map(|c| s.params_json(c).to_string())
                .collect::<std::collections::HashSet<_>>()
                .len()
        };

        // N over the grid clamps to the distinct grid size — no silent duplicates.
        let over = s.sample(SweepMethod::Random { n: grid * 4, seed: 7 });
        assert_eq!(over.len(), grid, "clamped to grid size");
        assert_eq!(distinct(&over), over.len(), "all distinct");
    }

    #[test]
    fn sample_prunes_shape_invalid_combos() {
        // A grid that straddles the `kill_depth_min ≥ vol_depth_max` invariant:
        // kill ∈ {0.4, 0.6}, vol = 0.6. The (0.4, 0.6) combos are invalid (0.4 < 0.6)
        // and must be pruned; (0.6, 0.6) is valid (equality allowed). Same predicate
        // rule validation uses — the sweep never surfaces an unsaveable "winner".
        let spec = AxesSpec {
            kill_depth_min_pct: Some(vec![Some(0.4), Some(0.6)]),
            vol_depth_max_pct: Some(vec![Some(0.6)]),
            ..Default::default()
        };
        let mut s = strategy();
        s.axes = Swing1Axes::from_spec(&spec);

        let full_grid = s.axes.combo_count();
        let combos = s.sample(SweepMethod::Grid);

        // Every survivor satisfies the shared invariant …
        assert!(combos.iter().all(|c| swing1_shape_sane(
            c.raw.kill_depth_min_pct,
            c.raw.vol_depth_max_pct,
            c.raw.kill_max_duration_ms,
            c.raw.vol_min_duration_ms,
        )
        .is_ok()));
        // … the invalid (0.4, 0.6) half was dropped …
        assert!(combos.len() < full_grid, "shape-invalid combos must be pruned");
        // … and the valid (0.6, 0.6) half remains (fires, doesn't over-prune).
        assert!(!combos.is_empty(), "valid combos must survive");
    }

    #[test]
    fn order_for_entry_cache_is_a_permutation_grouping_same_entry() {
        // A shuffled (random) sample, ordered for the entry cache, must (a) be a
        // pure permutation and (b) place same-entry-key combos contiguously.
        let s = strategy();
        let grid = s.axes.combo_count();
        let mut combos = s.sample(SweepMethod::Random { n: grid, seed: 11 });

        let mut before: Vec<String> =
            combos.iter().map(|c| s.params_json(c).to_string()).collect();
        s.order_for_entry_cache(&mut combos);
        let mut after: Vec<String> =
            combos.iter().map(|c| s.params_json(c).to_string()).collect();
        before.sort();
        after.sort();
        assert_eq!(before, after, "ordering must be a pure permutation of the combos");

        // Contiguity: each distinct entry key forms one unbroken run.
        let mut seen: HashSet<[u64; 17]> = HashSet::new();
        let mut prev: Option<[u64; 17]> = None;
        for c in &combos {
            let k = entry_order_key(&c.raw);
            if Some(k) != prev {
                assert!(seen.insert(k), "entry key reappeared non-contiguously");
                prev = Some(k);
            }
        }
    }

    #[test]
    fn params_json_round_trips() {
        let s = strategy();
        let combo = s.combo_at(0);
        let json = s.params_json(&combo);
        let back = s.combo_from_params_json(&json).expect("round-trip");
        assert_eq!(s.params_json(&back).to_string(), json.to_string());
    }
}
