//! swing1 phase classifier — **causal, count-free** kill→volume transition.
//!
//! Walks the raw alternating leg ledger ([`detect_swing_legs_raw`]) and decides,
//! per swing-LOW leg, whether the token has transitioned from the dev's
//! kill-swing gauntlet (short, deep near-death lows that eat bots) into the
//! volume-making phase (longer, shallower higher-lows that attract real traders).
//! Every gate is a per-leg *causal* predicate — a leg's classification never
//! depends on a future leg — so the live-incremental machine (Phase 2) and the
//! batch backtest produce the identical latch, pinned by a parity test.
//!
//! The transition is **count-free**: it detects the shape change (legs get longer
//! + shallower), with the floor `min_kills_before_volume` (incl. 0) a swept param
//! rather than a hardcoded count.
//!
//! [`detect_swing_legs_raw`]: super::swing::detect_swing_legs_raw

use super::swing::{SwingLeg, SwingType};

/// Per-swing-low features derived from a finalized [`SwingLeg`], all causal.
#[derive(Debug, Clone, Copy)]
pub struct LowFeatures {
    /// Fractional drop from the leg's start to its terminal pivot (the low
    /// extreme / last big sell): `(start_price - pivot_end_price) / start_price`,
    /// in `[0, 1]`. Larger = deeper. `0` when `start_price <= 0`.
    pub depth_pct: f64,
    pub duration_ms: i64,
    /// `|net_flow| / (duration_ms / 1000)`; `0` for instantaneous (0-duration) legs.
    pub net_flow_per_sec: f64,
    pub trade_count: u32,
    /// The leg's terminal pivot price (its low) — used for higher-low comparison.
    pub pivot_price: f64,
}

impl LowFeatures {
    /// Derive the causal features of a swing-LOW leg. Returns `None` for a leg
    /// that is not a [`SwingType::SwingLow`].
    pub fn from_low(leg: &SwingLeg) -> Option<Self> {
        if leg.leg_type != SwingType::SwingLow {
            return None;
        }
        let depth_pct = if leg.start_price > 0.0 {
            ((leg.start_price - leg.pivot_end_price) / leg.start_price).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let net_flow_per_sec = if leg.duration_ms > 0 {
            leg.net_flow.abs() / (leg.duration_ms as f64 / 1000.0)
        } else {
            0.0
        };
        Some(Self {
            depth_pct,
            duration_ms: leg.duration_ms,
            net_flow_per_sec,
            trade_count: leg.trade_count,
            pivot_price: leg.pivot_end_price,
        })
    }
}

/// The kill / volume profile thresholds the classifier gates on. `0`/`None` =
/// "no bound" for that term (it drops out of the AND). All sourced from
/// `Swing1Params` (Phase 1c) — both batch and live build the same struct.
#[derive(Debug, Clone, Copy)]
pub struct PhaseProfile {
    // ── kill-low profile (deep + short) ───────────────────────────────────────
    /// Min fractional depth for a kill low, e.g. `0.6` = 60% drop. `0` = no bound.
    pub kill_depth_min_pct: f64,
    /// Max duration for a kill low (short). `0` = no bound.
    pub kill_max_duration_ms: i64,
    /// Optional min `|net_flow|/sec` magnitude for a kill low. `0` = no bound.
    pub kill_min_net_flow_per_sec: f64,
    // ── volume-low profile (shallow + long, after real accumulation) ───────────
    /// Max fractional depth for a volume low (shallow). `0` = no bound.
    pub vol_depth_max_pct: f64,
    /// Min duration for a volume low (long). `0` = no bound.
    pub vol_min_duration_ms: i64,
    /// Min duration of the up-leg immediately preceding the volume low — requires
    /// real accumulation before the higher-low. `0` = no bound.
    pub vol_min_up_duration_ms: i64,
    // ── transition floor (count-free) ─────────────────────────────────────────
    /// Number of kill lows that must be seen before the volume phase can latch.
    /// `0` means the kill gate adds no edge (the sweep tests this).
    pub min_kills_before_volume: u32,
}

impl PhaseProfile {
    /// A swing-low leg is a **kill** low: deep AND short (AND optionally a fast
    /// net-flow drain). Each bound with value `0` is skipped.
    pub fn is_kill_low(&self, f: &LowFeatures) -> bool {
        (self.kill_depth_min_pct <= 0.0 || f.depth_pct >= self.kill_depth_min_pct)
            && (self.kill_max_duration_ms <= 0 || f.duration_ms <= self.kill_max_duration_ms)
            && (self.kill_min_net_flow_per_sec <= 0.0
                || f.net_flow_per_sec >= self.kill_min_net_flow_per_sec)
            // A kill must clear at least one positive gate, else "deep+short" is
            // vacuously true for every leg and the count-free floor is meaningless.
            && (self.kill_depth_min_pct > 0.0
                || self.kill_max_duration_ms > 0
                || self.kill_min_net_flow_per_sec > 0.0)
    }

    /// A swing-low leg is a **volume** low: shallow AND long, with a long enough
    /// preceding up-leg (real accumulation). `prev_up_duration_ms` is the duration
    /// of the swing-HIGH leg immediately before this low (`None` if this low is the
    /// first leg / has no preceding up-leg).
    pub fn is_volume_low(&self, f: &LowFeatures, prev_up_duration_ms: Option<i64>) -> bool {
        let up_ok = self.vol_min_up_duration_ms <= 0
            || prev_up_duration_ms.is_some_and(|d| d >= self.vol_min_up_duration_ms);
        (self.vol_depth_max_pct <= 0.0 || f.depth_pct <= self.vol_depth_max_pct)
            && (self.vol_min_duration_ms <= 0 || f.duration_ms >= self.vol_min_duration_ms)
            && up_ok
            // Like the kill gate: require at least one positive volume bound so the
            // latch is meaningful (else every low is a "volume" low).
            && (self.vol_depth_max_pct > 0.0
                || self.vol_min_duration_ms > 0
                || self.vol_min_up_duration_ms > 0)
    }
}

/// The result of classifying a token's swing chain up to some prefix.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhaseLatch {
    /// `true` once the volume phase has latched (sticky for entry).
    pub volume_phase_latched: bool,
    /// Index into the *raw leg ledger* of the swing-low leg that latched the
    /// volume phase (`None` until latched). Entry arming begins after this leg.
    pub latched_leg_index: Option<usize>,
    /// How many kill lows were seen before the latch (diagnostic; 0 is meaningful).
    pub kills_seen: u32,
}

/// Walk the raw leg ledger in time order and latch the kill→volume transition.
///
/// Maintains `kills_seen` and the last kill low's pivot price. The volume phase
/// latches at the **first** swing-low `L` where:
///   1. `kills_seen >= min_kills_before_volume`, AND
///   2. `is_volume_low(L)` (shallow + long, real accumulation), AND
///   3. `L` is a *higher low* vs the last kill low (`L.pivot >= last_kill_pivot`).
///
/// The latch is sticky (once set it never clears within this chain). With no
/// prior kill low (e.g. `min_kills_before_volume == 0`), the higher-low gate is
/// vacuously satisfied.
pub fn classify_phase(legs: &[SwingLeg], profile: &PhaseProfile) -> PhaseLatch {
    let mut kills_seen: u32 = 0;
    let mut last_kill_pivot: Option<f64> = None;
    let mut prev_up_duration_ms: Option<i64> = None;

    for (i, leg) in legs.iter().enumerate() {
        match leg.leg_type {
            SwingType::SwingHigh => {
                prev_up_duration_ms = Some(leg.duration_ms);
            }
            SwingType::SwingLow => {
                let f = LowFeatures::from_low(leg).expect("leg_type checked SwingLow");

                // Try to latch the volume phase BEFORE counting this low as a kill —
                // a single leg is either the transition or another kill, never both.
                let higher_low = match last_kill_pivot {
                    Some(p) => f.pivot_price >= p,
                    None => true, // no prior kill ⇒ higher-low gate vacuous
                };
                if kills_seen >= profile.min_kills_before_volume
                    && profile.is_volume_low(&f, prev_up_duration_ms)
                    && higher_low
                {
                    return PhaseLatch {
                        volume_phase_latched: true,
                        latched_leg_index: Some(i),
                        kills_seen,
                    };
                }

                if profile.is_kill_low(&f) {
                    kills_seen += 1;
                    last_kill_pivot = Some(f.pivot_price);
                }
                // Reset the preceding-up tracker; the next high sets it again.
                prev_up_duration_ms = None;
            }
        }
    }

    PhaseLatch {
        volume_phase_latched: false,
        latched_leg_index: None,
        kills_seen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn low(start: f64, pivot: f64, dur_ms: i64, net_flow: f64, trades: u32) -> SwingLeg {
        SwingLeg {
            leg_type: SwingType::SwingLow,
            start_at: 0,
            end_at: dur_ms,
            duration_ms: dur_ms,
            start_price: start,
            end_price: pivot,
            pivot_end_at: dur_ms,
            pivot_end_price: pivot,
            inflow: 0.0,
            outflow: net_flow.abs(),
            net_flow,
            trade_count: trades,
        }
    }

    fn high(start: f64, end: f64, dur_ms: i64, trades: u32) -> SwingLeg {
        SwingLeg {
            leg_type: SwingType::SwingHigh,
            start_at: 0,
            end_at: dur_ms,
            duration_ms: dur_ms,
            start_price: start,
            end_price: end,
            pivot_end_at: dur_ms,
            pivot_end_price: end,
            inflow: 0.0,
            outflow: 0.0,
            net_flow: 0.0,
            trade_count: trades,
        }
    }

    fn profile() -> PhaseProfile {
        PhaseProfile {
            kill_depth_min_pct: 0.6,    // ≥60% drop
            kill_max_duration_ms: 5_000, // ≤5s
            kill_min_net_flow_per_sec: 0.0,
            vol_depth_max_pct: 0.3,        // ≤30% drop
            vol_min_duration_ms: 10_000,   // ≥10s
            vol_min_up_duration_ms: 5_000, // up-leg ≥5s before it
            min_kills_before_volume: 2,
        }
    }

    #[test]
    fn kill_low_is_deep_and_short() {
        let p = profile();
        // 70% drop in 2s → kill.
        let f = LowFeatures::from_low(&low(100.0, 30.0, 2_000, -3.0, 4)).unwrap();
        assert!(p.is_kill_low(&f));
        // 70% drop but 12s long → not short → not a kill.
        let f = LowFeatures::from_low(&low(100.0, 30.0, 12_000, -3.0, 4)).unwrap();
        assert!(!p.is_kill_low(&f));
        // shallow (20%) + short → not deep → not a kill.
        let f = LowFeatures::from_low(&low(100.0, 80.0, 2_000, -1.0, 4)).unwrap();
        assert!(!p.is_kill_low(&f));
    }

    #[test]
    fn volume_low_is_shallow_long_with_accumulation() {
        let p = profile();
        let f = LowFeatures::from_low(&low(100.0, 80.0, 12_000, -1.0, 6)).unwrap();
        assert!(p.is_volume_low(&f, Some(6_000)));
        // No preceding up-leg → fails accumulation gate.
        assert!(!p.is_volume_low(&f, None));
        // Up-leg too short → fails.
        assert!(!p.is_volume_low(&f, Some(2_000)));
        // Too deep (50%) → fails shallow gate.
        let deep = LowFeatures::from_low(&low(100.0, 50.0, 12_000, -1.0, 6)).unwrap();
        assert!(!p.is_volume_low(&deep, Some(6_000)));
    }

    #[test]
    fn latches_after_kills_on_higher_low() {
        let p = profile();
        // 2 kill lows (deep+short), then an up-leg, then a shallow+long higher-low.
        let legs = vec![
            high(10.0, 100.0, 6_000, 5),    // up
            low(100.0, 20.0, 2_000, -4.0, 4),  // kill #1 (pivot 20)
            high(20.0, 90.0, 6_000, 5),     // up
            low(90.0, 25.0, 2_000, -4.0, 4),   // kill #2 (pivot 25)
            high(25.0, 95.0, 6_000, 5),     // up (accumulation)
            low(95.0, 70.0, 12_000, -1.0, 6),  // volume low, higher-low (70 ≥ 25)
        ];
        let latch = classify_phase(&legs, &p);
        assert!(latch.volume_phase_latched);
        assert_eq!(latch.latched_leg_index, Some(5));
        assert_eq!(latch.kills_seen, 2);
    }

    #[test]
    fn no_latch_before_min_kills() {
        let p = profile(); // min_kills_before_volume = 2
        // Only ONE kill before a perfect volume low → must NOT latch.
        let legs = vec![
            high(10.0, 100.0, 6_000, 5),
            low(100.0, 20.0, 2_000, -4.0, 4), // kill #1
            high(20.0, 95.0, 6_000, 5),
            low(95.0, 70.0, 12_000, -1.0, 6), // volume low but only 1 kill seen
        ];
        let latch = classify_phase(&legs, &p);
        assert!(!latch.volume_phase_latched);
        assert_eq!(latch.kills_seen, 1);
    }

    #[test]
    fn min_kills_zero_latches_on_first_volume_low() {
        let mut p = profile();
        p.min_kills_before_volume = 0;
        let legs = vec![
            high(10.0, 100.0, 6_000, 5),
            low(100.0, 70.0, 12_000, -1.0, 6), // first low is a volume low → latch
        ];
        let latch = classify_phase(&legs, &p);
        assert!(latch.volume_phase_latched);
        assert_eq!(latch.latched_leg_index, Some(1));
        assert_eq!(latch.kills_seen, 0);
    }

    #[test]
    fn lower_low_after_kills_does_not_latch() {
        let p = profile();
        // 2 kills, then a shallow+long low but it's LOWER than the last kill pivot.
        let legs = vec![
            high(10.0, 100.0, 6_000, 5),
            low(100.0, 20.0, 2_000, -4.0, 4), // kill #1 (pivot 20)
            high(20.0, 90.0, 6_000, 5),
            low(90.0, 40.0, 2_000, -4.0, 4), // kill #2 (pivot 40)
            high(40.0, 45.0, 6_000, 5),
            // shallow (only 22% drop: (45-35)/45) + long, but pivot 35 < last kill
            // pivot 40 → not a higher-low → must NOT latch.
            low(45.0, 35.0, 12_000, -1.0, 6),
        ];
        let latch = classify_phase(&legs, &p);
        assert!(!latch.volume_phase_latched);
    }
}
