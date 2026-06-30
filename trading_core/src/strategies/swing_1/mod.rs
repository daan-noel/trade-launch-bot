//! `swing_1` — Kill→Volume Swing-Phase strategy domain.
//!
//! Meme-coin devs manufacture price swings: early **kill-swings** (short, deep
//! near-death lows) eat sniper/launch bots, then a **volume-making phase**
//! (longer, shallower higher-lows) attracts real traders before the rug. This
//! strategy reads each token's swing chain with a single causal leg classifier
//! applied three ways — to find the kill→volume transition, to trigger entry on
//! the first volume-phase confirmed higher-low, and to detect a symmetric
//! next-kill exit. See [swing1-plan.md](../../../../swing1-plan.md).
//!
//! Pricing is the shared GMGN spot ([`crate::models::trade::TradeRow::chart_spot_price`])
//! so a leg detected offline (sweep) is the leg detected live.

pub mod classifier;
pub mod entry;
pub mod exit;
pub mod swing;

use crate::models::Swing1Rule;
use classifier::PhaseProfile;
use swing::SwingParams;

/// Map zero/None to None (a 0 bound is "no bound"), mirroring the tpsl util.
fn nz_f64(v: Option<f64>) -> Option<f64> {
    v.filter(|x| *x != 0.0)
}
fn nz_i64(v: Option<i64>) -> Option<i64> {
    v.filter(|x| *x != 0)
}
fn nz_u32(v: Option<u32>) -> Option<u32> {
    v.filter(|x| *x != 0)
}

/// Build the [`SwingParams`] the analyzer scans with from a [`Swing1Rule`]. Only
/// the reversal thresholds + the min-leg-trades floor are taken from the rule;
/// the non-causal pair-drop quality filters stay at their inert defaults (the
/// classifier walks the raw ledger). `0` reversal bounds fall back to the
/// `SwingParams` defaults so a rule that sets neither still detects swings.
pub fn swing_params_from_rule(r: &Swing1Rule) -> SwingParams {
    let d = SwingParams::default();
    SwingParams {
        high_to_low_threshold_sol: nz_f64(r.p_swing_high_to_low_sol)
            .unwrap_or(d.high_to_low_threshold_sol),
        high_to_low_threshold_pct: nz_f64(r.p_swing_high_to_low_pct)
            .unwrap_or(d.high_to_low_threshold_pct),
        low_to_high_threshold_sol: nz_f64(r.p_swing_low_to_high_sol)
            .unwrap_or(d.low_to_high_threshold_sol),
        low_to_high_threshold_pct: nz_f64(r.p_swing_low_to_high_pct)
            .unwrap_or(d.low_to_high_threshold_pct),
        min_leg_trades: nz_u32(r.p_swing_min_leg_trades).unwrap_or(d.min_leg_trades),
        ..SwingParams::default()
    }
}

/// Build the [`PhaseProfile`] (kill/volume gates + transition floor) from a rule.
/// The *entry* kill thresholds (`p_kill_*`) drive the kill→volume transition; the
/// symmetric *exit* next-kill thresholds (`p_exit_next_kill_*`) are separate (see
/// [`exit_next_kill_profile`]).
pub fn phase_profile_from_rule(r: &Swing1Rule) -> PhaseProfile {
    PhaseProfile {
        kill_depth_min_pct: nz_f64(r.p_kill_depth_min_pct).unwrap_or(0.0),
        kill_max_duration_ms: nz_i64(r.p_kill_max_duration_ms).unwrap_or(0),
        kill_min_net_flow_per_sec: nz_f64(r.p_kill_min_net_flow_per_sec).unwrap_or(0.0),
        vol_depth_max_pct: nz_f64(r.p_vol_depth_max_pct).unwrap_or(0.0),
        vol_min_duration_ms: nz_i64(r.p_vol_min_duration_ms).unwrap_or(0),
        vol_min_up_duration_ms: nz_i64(r.p_vol_min_up_duration_ms).unwrap_or(0),
        min_kills_before_volume: r.p_min_kills_before_volume.unwrap_or(0),
    }
}

/// The kill profile the **exit** ladder uses to detect a post-entry next-kill leg
/// — deep + short, tuned independently of the entry kill thresholds. Returns
/// `None` when neither next-kill bound is configured (the arm is disabled).
pub fn exit_next_kill_profile(r: &Swing1Rule) -> Option<PhaseProfile> {
    let depth = nz_f64(r.p_exit_next_kill_depth_min_pct).unwrap_or(0.0);
    let dur = nz_i64(r.p_exit_next_kill_max_duration_ms).unwrap_or(0);
    if depth <= 0.0 && dur <= 0 {
        return None;
    }
    Some(PhaseProfile {
        kill_depth_min_pct: depth,
        kill_max_duration_ms: dur,
        kill_min_net_flow_per_sec: 0.0,
        // The volume side is unused by the exit (it only calls is_kill_low).
        vol_depth_max_pct: 0.0,
        vol_min_duration_ms: 0,
        vol_min_up_duration_ms: 0,
        min_kills_before_volume: 0,
    })
}

/// Whether a rule configures **any** swing1 entry gate. swing1's entry needs the
/// volume-phase latch (≥1 volume bound) AND a higher-low confirmation; without a
/// configured pullback or volume profile the rule can never resolve an entry, so
/// the validator/backtest reject it up front (never a silent buy-everything path).
pub fn rule_configures_any_entry_gate(r: &Swing1Rule) -> bool {
    nz_f64(r.p_entry_pullback_pct).is_some()
        && (nz_f64(r.p_vol_depth_max_pct).is_some()
            || nz_i64(r.p_vol_min_duration_ms).is_some()
            || nz_i64(r.p_vol_min_up_duration_ms).is_some())
}
