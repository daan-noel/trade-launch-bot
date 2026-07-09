//! Shared swing1 detection funnel — the leg ledger + per-low classifier verdicts +
//! kill→volume latch for one token, built by a single `TradeRow`-generic fn so the
//! diagnostic surfaces can never drift from each other.
//!
//! **Single source of truth, two levels.** The *decision* logic (entry latch + exit
//! ladder) already lives in one place — [`super::entry::find_phase_entry`] /
//! [`super::exit::find_trade_driven_exit`] — and the grouped sweep, the backtest, the
//! detect handler and the probe all call those same pure fns, so a rule prices
//! identically however it is run. This module owns the *diagnostic* layer on top: the
//! leg ledger + per-low verdicts + latch that the inspect chart and detection page
//! render. Its callers:
//! - the `POST /api/tokens/{mint}/swing1-detect` handler (over the lake `CorpusTrade`)
//!   builds the whole funnel for the per-token detection page;
//! - `lab swing-probe` / `lab swing-census` (over `Trade`) print the same funnel /
//!   share the same [`classify_swing_lows`] walk from the CLI;
//! - the lab backtest carries only the leg ledger in its per-token result (the inspect
//!   chart draws just the legs), sourced from the same [`detect_swing_legs_raw`]
//!   primitive this builder uses — so the carried legs equal the funnel's by
//!   construction, without paying to serialize per-token lows/latch the sim table
//!   never displays.
//!
//! Only the leg/low/latch core lives here. Entry + exit are resolved by the caller via
//! the shared decision fns (re-resolving here would double the work), so this stays
//! pure-CPU and I/O-free. Pinned to the primitives by `funnel_matches_leg_primitive`.

use serde::Serialize;

use crate::models::trade::TradeRow;
use crate::models::Swing1Rule;

use super::classifier::{self, LowFeatures, PhaseProfile};
use super::swing::{detect_swing_legs_raw, SwingLeg, SwingType};
use super::{phase_profile_from_rule, rule_configures_any_entry_gate, swing_params_from_rule};

/// One swing-low row's classifier verdict — the gates that drive the latch, plus the
/// leg's chart-anchoring timestamps so the UI can mark it on the candle chart.
#[derive(Debug, Clone, Serialize)]
pub struct Swing1LowVerdict {
    /// Index into the raw leg ledger (matches `legs[i]`).
    pub leg_index: usize,
    pub start_at_ms: i64,
    pub end_at_ms: i64,
    pub depth_pct: f64,
    pub duration_ms: i64,
    pub net_flow_per_sec: f64,
    pub trade_count: u32,
    pub pivot_price: f64,
    pub is_kill: bool,
    pub is_volume: bool,
    /// Whether this low is a higher-low vs the running last-kill pivot — the exact
    /// gate `classify_phase` applies (vacuously true with no prior kill).
    pub higher_low_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing1LatchInfo {
    pub volume_phase_latched: bool,
    pub latched_leg_index: Option<usize>,
    pub kills_seen: u32,
}

/// The shared funnel core: the raw leg ledger, the per-low verdicts, and the latch.
/// `gate_configured` is `false` when the rule configures no entry gate — the caller
/// then skips entry/exit resolution (the funnel still shows legs + verdicts for
/// diagnosis).
#[derive(Debug, Clone, Serialize)]
pub struct Swing1Funnel {
    pub gate_configured: bool,
    pub legs: Vec<SwingLeg>,
    pub lows: Vec<Swing1LowVerdict>,
    pub latch: Swing1LatchInfo,
}

/// Walk a raw leg ledger and emit the per-swing-low classifier verdicts — the single
/// low-verdict walk shared by the funnel builder, `lab swing-probe`, and `lab
/// swing-census`, so no caller re-implements the gate loop. Tracks the preceding
/// up-leg duration and the running last-kill pivot exactly as [`classify_phase`] does,
/// so `higher_low_ok` matches the latch gate. Pure CPU; operates on already-detected
/// legs so a caller with multiple profiles (census) can reuse one leg scan.
///
/// [`classify_phase`]: super::classifier::classify_phase
pub fn classify_swing_lows(legs: &[SwingLeg], profile: &PhaseProfile) -> Vec<Swing1LowVerdict> {
    let mut lows = Vec::new();
    let mut prev_up: Option<i64> = None;
    let mut last_kill_pivot: Option<f64> = None;
    for (i, leg) in legs.iter().enumerate() {
        match leg.leg_type {
            SwingType::SwingHigh => prev_up = Some(leg.duration_ms),
            SwingType::SwingLow => {
                if let Some(f) = LowFeatures::from_low(leg) {
                    let is_kill = profile.is_kill_low(&f);
                    let is_volume = profile.is_volume_low(&f, prev_up);
                    let higher_low_ok = last_kill_pivot.map_or(true, |p| f.pivot_price >= p);
                    lows.push(Swing1LowVerdict {
                        leg_index: i,
                        start_at_ms: leg.start_at,
                        end_at_ms: leg.end_at,
                        depth_pct: f.depth_pct,
                        duration_ms: f.duration_ms,
                        net_flow_per_sec: f.net_flow_per_sec,
                        trade_count: f.trade_count,
                        pivot_price: f.pivot_price,
                        is_kill,
                        is_volume,
                        higher_low_ok,
                    });
                    if is_kill {
                        last_kill_pivot = Some(f.pivot_price);
                    }
                }
                prev_up = None;
            }
        }
    }
    lows
}

/// Build the funnel from a chronological trade slice — pure CPU, no I/O. Generic over
/// [`TradeRow`] so it runs over the backtest/detect `CorpusTrade` and the probe's
/// `Trade` identically (`detect_swing_legs_raw` + `classify_phase` are both generic).
pub fn build_swing1_funnel<T: TradeRow>(trades: &[T], rule: &Swing1Rule) -> Swing1Funnel {
    let gate_configured = rule_configures_any_entry_gate(rule);
    let sparams = swing_params_from_rule(rule);
    let profile = phase_profile_from_rule(rule);
    let legs = detect_swing_legs_raw(trades, &sparams);

    let lows = classify_swing_lows(&legs, &profile);
    let latch = classifier::classify_phase(&legs, &profile);
    Swing1Funnel {
        gate_configured,
        legs,
        lows,
        latch: Swing1LatchInfo {
            volume_phase_latched: latch.volume_phase_latched,
            latched_leg_index: latch.latched_leg_index,
            kills_seen: latch.kills_seen,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use crate::models::Swing1Rule;
    use chrono::Utc;

    fn trade(ms: i64, ty: TradeType, sol: f64, tokens: u64) -> Trade {
        let mut t = Trade::new(
            "mint".into(),
            "user".into(),
            ty,
            sol,
            tokens,
            "sig".into(),
            1,
            Utc::now(),
        );
        t.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        t
    }

    /// A rule with real reversal thresholds + kill/volume gates so the fixture forms
    /// several legs with a mix of low verdicts (the walk under test).
    fn fixture_rule() -> Swing1Rule {
        let mut r = Swing1Rule::new(
            "funnel-parity".into(),
            None,
            None,
            None,
            serde_json::json!([]),
            "paper".into(),
            0.05,
            100.0,
            50.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(60),
            None,
        );
        r.p_swing_high_to_low_sol = Some(1.0);
        r.p_swing_low_to_high_sol = Some(1.0);
        r.p_swing_high_to_low_pct = Some(15.0);
        r.p_swing_low_to_high_pct = Some(15.0);
        r.p_kill_depth_min_pct = Some(0.4);
        r.p_kill_max_duration_ms = Some(10_000);
        r.p_vol_depth_max_pct = Some(0.6);
        r.p_min_kills_before_volume = Some(0);
        r.p_entry_pullback_pct = Some(10.0);
        r
    }

    /// A varied buy/sell chain that reverses several times → multiple legs.
    fn fixture_trades() -> Vec<Trade> {
        vec![
            trade(1_000, TradeType::Buy, 3.0, 300_000),
            trade(2_000, TradeType::Buy, 2.0, 150_000),
            trade(3_000, TradeType::Sell, 2.5, 250_000), // reversal down
            trade(4_000, TradeType::Sell, 1.5, 200_000),
            trade(5_000, TradeType::Buy, 3.0, 200_000), // reversal up
            trade(6_000, TradeType::Buy, 2.0, 120_000),
            trade(7_000, TradeType::Sell, 3.0, 240_000), // reversal down
            trade(8_000, TradeType::Buy, 2.5, 140_000),  // reversal up
        ]
    }

    /// The funnel must never drift from the primitives it wraps: its legs equal the
    /// exact `detect_swing_legs_raw` call the backtest carries, its lows equal the
    /// shared `classify_swing_lows` walk, and its latch equals `classify_phase` — so
    /// the detect page, the probe, and the sim's carried legs all agree by
    /// construction, not by coincidence.
    #[test]
    fn funnel_matches_leg_primitive() {
        let rule = fixture_rule();
        let trades = fixture_trades();

        let funnel = build_swing1_funnel(&trades, &rule);

        // Legs == the exact primitive the backtest carries (`backtest.rs` uses
        // `detect_swing_legs_raw(trades, &swing_params_from_rule(&rule))`).
        let legs = detect_swing_legs_raw(&trades, &swing_params_from_rule(&rule));
        assert_eq!(
            serde_json::to_value(&funnel.legs).unwrap(),
            serde_json::to_value(&legs).unwrap(),
            "funnel legs must equal the backtest's leg primitive"
        );
        assert!(!funnel.legs.is_empty(), "fixture must form legs to exercise the walk");

        // Lows == the shared walk the probe/census also call.
        let profile = phase_profile_from_rule(&rule);
        let lows = classify_swing_lows(&legs, &profile);
        assert_eq!(
            serde_json::to_value(&funnel.lows).unwrap(),
            serde_json::to_value(&lows).unwrap(),
            "funnel lows must equal the shared classify_swing_lows walk"
        );

        // Latch == classify_phase over the same legs/profile.
        let latch = classifier::classify_phase(&legs, &profile);
        assert_eq!(funnel.latch.volume_phase_latched, latch.volume_phase_latched);
        assert_eq!(funnel.latch.latched_leg_index, latch.latched_leg_index);
        assert_eq!(funnel.latch.kills_seen, latch.kills_seen);
    }
}
