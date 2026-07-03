//! Shared swing1 detection funnel — the leg ledger + per-low classifier verdicts +
//! kill→volume latch for one token, built by a single `TradeRow`-generic fn.
//!
//! One builder, three callers, so they can never drift:
//! - the lab backtest ([`crate::strategies::swing_1`] over `CorpusTrade`) carries the
//!   funnel in its per-token result, so the inspect chart draws exactly the legs the
//!   sim resolved entry/exit against — no separate detect round-trip;
//! - the `POST /api/tokens/{mint}/swing1-detect` handler (over the same lake
//!   `CorpusTrade`) renders the per-token detection page;
//! - `lab swing-probe` (over `Trade`) prints the same funnel from the CLI.
//!
//! Only the leg/low/latch core lives here (the part all three share). Entry + exit are
//! resolved by the caller (the backtest already computes them; re-resolving here would
//! double the work), so this fn stays pure-CPU and I/O-free.

use serde::Serialize;

use crate::models::trade::TradeRow;
use crate::models::Swing1Rule;

use super::classifier::{self, LowFeatures};
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

/// Build the funnel from a chronological trade slice — pure CPU, no I/O. Generic over
/// [`TradeRow`] so it runs over the backtest/detect `CorpusTrade` and the probe's
/// `Trade` identically (`detect_swing_legs_raw` + `classify_phase` are both generic).
pub fn build_swing1_funnel<T: TradeRow>(trades: &[T], rule: &Swing1Rule) -> Swing1Funnel {
    let gate_configured = rule_configures_any_entry_gate(rule);
    let sparams = swing_params_from_rule(rule);
    let profile = phase_profile_from_rule(rule);
    let legs = detect_swing_legs_raw(trades, &sparams);

    // Walk the ledger collecting per-low verdicts, tracking the preceding up-leg
    // duration and the running last-kill pivot — the same state `classify_phase`
    // keeps, so `higher_low_ok` matches the latch gate.
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
