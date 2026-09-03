//! `m_burst_wave` — this token's buys in the current consecutive-slot run.
//!
//! Static (no window). Token-level for wallet/sol/gap/hole/tip; `working_buy_count`
//! and `this_working` intersect this fingerprint's `m_burst_slot.working_templates`
//! at read (same list, no second config). A **wave** is consecutive buy-slots (no
//! empty buy-slot between them). It resets when the next buy is at least 2 slots
//! after the last buy-slot. The gap is empty buy-slots *before this wave started*,
//! not before a later printer in the same run.
//!
//! Create slot does not start a fireable wave: seed `creation_slot` so buys in
//! that slot (and consecutive slots after it) stay unfireable until a real gap.
//! Launch creates are not members. Per-print facts (`this_member`, `this_working`,
//! `this_tip`, `hole`, `tip_seen`) clear on a tick.
//!
//! Completing prints: `wallet_count` crosses 2 (any member), or `working_buy_count`
//! crosses 2 (named-list prints in this wave). `hole` and `tip_seen` follow every
//! curve buy in the wave (same predecessor as the Python mem fold), not only
//! template members. `hole` is a wave `tx_index` gap, not `m_burst_slot.packed`.
//! `tip_seen` is this print's tip band already present on an earlier wave buy.

use crate::hash::HashedSet;
use super::burst_slot::{is_member, BurstPatterns};
use super::{MetricId, Side, TradeLite};

/// One token's current consecutive-slot buy wave.
#[derive(Debug, Clone)]
pub struct BurstWaveState {
    last_slot: Option<u64>,
    fireable: bool,
    gap_slots: f64,
    wave_wals: HashedSet,
    wave_sol: f64,
    wave_unknown: bool,
    /// Wallets that have been members of any earlier wave (or this one so far).
    seen: HashedSet,
    seen_wave_start: HashedSet,
    this_member: bool,
    /// Template grain of each member print this wave, in fold order.
    wave_grains: Vec<u64>,
    /// Prior member's `tx_index` this wave. Missing index stores `-1`.
    prev_txi: Option<i64>,
    /// Tip-band bits already seen on earlier members of this wave.
    seen_tip_bands: u8,
    this_template_hash: Option<u64>,
    this_program_hash: Option<u64>,
    /// Program hash of each member print this wave, parallel to `wave_grains`.
    wave_programs: Vec<Option<u64>>,
    this_tip: f64,
    this_hole: bool,
    this_tip_seen: bool,
}

impl Default for BurstWaveState {
    fn default() -> Self {
        Self {
            last_slot: None,
            fireable: false,
            gap_slots: f64::NAN,
            wave_wals: HashedSet::default(),
            wave_sol: 0.0,
            wave_unknown: false,
            seen: HashedSet::default(),
            seen_wave_start: HashedSet::default(),
            this_member: false,
            wave_grains: Vec::new(),
            prev_txi: None,
            seen_tip_bands: 0,
            this_template_hash: None,
            this_program_hash: None,
            wave_programs: Vec::new(),
            this_tip: f64::NAN,
            this_hole: false,
            this_tip_seen: false,
        }
    }
}

/// Tip band for `tip_seen`. Absent is its own band (`na`), not zero.
fn tip_band(tip: Option<u64>) -> u8 {
    match tip {
        None => 0,
        Some(0) => 1,
        Some(v) if v < 100_000 => 2,
        Some(v) if v < 1_000_000 => 3,
        Some(_) => 4,
    }
}

impl BurstWaveState {
    /// Seed the mint's create slot so that slot is not this event.
    pub fn seed_creation_slot(&mut self, slot: u64) {
        self.last_slot = Some(slot);
        self.start_wave(f64::NAN, false);
    }

    fn start_wave(&mut self, gap: f64, fireable: bool) {
        self.seen_wave_start = self.seen.clone();
        self.wave_wals.clear();
        self.wave_sol = 0.0;
        self.wave_unknown = false;
        self.wave_grains.clear();
        self.wave_programs.clear();
        self.prev_txi = None;
        self.seen_tip_bands = 0;
        self.gap_slots = gap;
        self.fireable = fireable;
    }

    fn clear_this_print(&mut self) {
        self.this_member = false;
        self.this_template_hash = None;
        self.this_program_hash = None;
        self.this_tip = f64::NAN;
        self.this_hole = false;
        self.this_tip_seen = false;
    }

    /// A tick is not a print — membership and leftover bits must not survive
    /// into a later `can_enter`. `this_tip` is the last member print's tip and
    /// stays; the event still requires `this_member`.
    pub fn on_tick(&mut self) {
        self.this_member = false;
        self.this_template_hash = None;
        self.this_program_hash = None;
        self.this_hole = false;
        self.this_tip_seen = false;
    }

    pub fn on_trade(&mut self, t: &TradeLite) {
        self.clear_this_print();
        if t.side != Side::Buy || !t.on_curve || t.is_launch {
            return;
        }
        if t.slot == 0 {
            return;
        }

        match self.last_slot {
            None => self.start_wave(f64::NAN, false),
            Some(prev) if t.slot >= prev.saturating_add(2) => {
                self.start_wave((t.slot - prev) as f64, true);
            }
            Some(prev) if t.slot < prev => self.start_wave(f64::NAN, false),
            _ => {}
        }

        if is_member(t) {
            self.this_member = true;
            self.this_template_hash = t.template_hash;
            self.this_program_hash = t.program_hash;
            self.this_tip = t.fee.tip_lamports().map(|v| v as f64).unwrap_or(f64::NAN);
            let txi = t.tx_index.map(i64::from).unwrap_or(-1);
            self.this_hole = self.prev_txi.is_some_and(|p| txi - p > 1);
            let band = tip_band(t.fee.tip_lamports());
            self.this_tip_seen = self.seen_tip_bands & (1 << band) != 0;
            if t.wallet_hash == 0 {
                self.wave_unknown = true;
            } else {
                self.wave_wals.insert(t.wallet_hash);
                self.seen.insert(t.wallet_hash);
            }
            self.wave_sol += t.sol;
            if let Some(h) = t.template_hash {
                self.wave_grains.push(h);
                self.wave_programs.push(t.program_hash);
            }
        } else if t.wallet_hash != 0 {
            self.seen.insert(t.wallet_hash);
        }
        // Wave tx_index / tip-band memory follows every curve buy in the run
        // (Python `ix7.mem` fold), not only template members — otherwise
        // `hole` / `tip_seen` read gaps and bands on the wrong predecessor.
        let txi = t.tx_index.map(i64::from).unwrap_or(-1);
        let band = tip_band(t.fee.tip_lamports());
        self.prev_txi = Some(txi);
        self.seen_tip_bands |= 1 << band;
        self.last_slot = Some(t.slot);
    }

    pub fn value(&self, id: MetricId, patterns: Option<&BurstPatterns>) -> f64 {
        use MetricId::*;
        match id {
            WaveThisMember => f64::from(u8::from(self.this_member)),
            WaveWalletCount => self.wave_wals.len() as f64,
            WaveBuySol => self.wave_sol,
            WaveGapSlots => {
                if self.fireable {
                    self.gap_slots
                } else {
                    f64::NAN
                }
            }
            WaveAllNew => {
                if self.wave_wals.is_empty() {
                    f64::NAN
                } else {
                    let all_new = self
                        .wave_wals
                        .iter()
                        .all(|w| !self.seen_wave_start.contains(w));
                    f64::from(u8::from(all_new))
                }
            }
            WaveHasUnknown => f64::from(u8::from(self.wave_unknown)),
            WaveWorkingBuyCount => {
                let Some(p) = patterns else {
                    return f64::NAN;
                };
                if !self.fireable {
                    return f64::NAN;
                }
                self.wave_grains
                    .iter()
                    .zip(self.wave_programs.iter())
                    .filter(|(h, prog)| p.matches(Some(**h), **prog))
                    .count() as f64
            }
            WaveThisWorking => {
                let Some(p) = patterns else {
                    return f64::NAN;
                };
                f64::from(u8::from(
                    p.matches(self.this_template_hash, self.this_program_hash),
                ))
            }
            WaveThisTip => self.this_tip,
            WaveHole => f64::from(u8::from(self.this_hole)),
            WaveTipSeen => f64::from(u8::from(self.this_tip_seen)),
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::BurstPatterns;
    use crate::metrics::template_grain::grain_id_hash;
    use crate::metrics::TradeLite;
    use chrono::{TimeZone, Utc};

    fn ts(secs: i64) -> super::super::Ts {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn buy(slot: u64, wallet: u64, sol: f64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol,
            price: 1.0,
            reserve_sol: 10.0,
            priced_reserve_sol: 40.0,
            at: ts(slot as i64),
            slot,
            tx_index: Some(1),
            template_hash: Some(grain_id_hash("Pump.Fun|CU|ATA|F")),
            wallet_hash: wallet,
            on_curve: true,
            is_launch: false,
            ..Default::default()
        }
    }

    #[test]
    fn create_slot_is_not_this_event() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(100);
        s.on_trade(&buy(100, 1, 1.0));
        s.on_trade(&buy(100, 2, 1.0));
        assert!(s.value(MetricId::WaveGapSlots, None).is_nan());
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 2.0);
        assert_eq!(s.value(MetricId::WaveThisMember, None), 1.0);
    }

    #[test]
    fn gap_is_before_the_wave_not_before_a_later_slot() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(100);
        s.on_trade(&buy(100, 1, 0.5));
        // Quiet of 25 empty slots, then two wallets in one slot.
        s.on_trade(&buy(126, 2, 1.0));
        assert_eq!(s.value(MetricId::WaveGapSlots, None), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 1.0);
        s.on_trade(&buy(126, 3, 1.5));
        assert_eq!(s.value(MetricId::WaveGapSlots, None), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 2.0);
        assert_eq!(s.value(MetricId::WaveBuySol, None), 2.5);
        assert_eq!(s.value(MetricId::WaveAllNew, None), 1.0);
        assert_eq!(s.value(MetricId::WaveThisMember, None), 1.0);
        // Consecutive slot continues the SAME wave; gap does not become 1.
        s.on_trade(&buy(127, 4, 0.4));
        assert_eq!(s.value(MetricId::WaveGapSlots, None), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 3.0);
        assert_eq!(s.value(MetricId::WaveBuySol, None), 2.9);
    }

    #[test]
    fn all_new_is_every_wave_wallet_first_on_mint() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&buy(40, 1, 1.0));
        s.on_trade(&buy(40, 2, 1.0));
        assert_eq!(s.value(MetricId::WaveAllNew, None), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 2.0);
    }

    #[test]
    fn tick_clears_this_member() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(1);
        s.on_trade(&buy(10, 1, 1.0));
        assert_eq!(s.value(MetricId::WaveThisMember, None), 1.0);
        s.on_tick();
        assert_eq!(s.value(MetricId::WaveThisMember, None), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 1.0);
    }

    #[test]
    fn launch_is_not_a_member_and_does_not_start_a_wave() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(5);
        let mut launch = buy(5, 9, 3.0);
        launch.is_launch = true;
        s.on_trade(&launch);
        assert_eq!(s.value(MetricId::WaveThisMember, None), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 0.0);
        assert!(s.value(MetricId::WaveGapSlots, None).is_nan());
        s.on_trade(&buy(20, 1, 1.0));
        assert_eq!(s.value(MetricId::WaveGapSlots, None), 15.0);
        assert_eq!(s.value(MetricId::WaveWalletCount, None), 1.0);
    }

    #[test]
    fn working_buy_count_is_named_prints_in_the_fireable_wave() {
        use super::BurstPatterns;
        let named = BurstPatterns::from_metric_config(&serde_json::json!({
            "m_burst_slot": { "working_templates": ["Axiom Trade|CU|ATA|F"] }
        }))
        .unwrap();
        let mut other = buy(30, 2, 1.0);
        other.template_hash = Some(grain_id_hash("Pump.Fun|CU|ATA|F"));
        let mut named_buy = buy(30, 3, 0.4);
        named_buy.template_hash = Some(grain_id_hash("Axiom Trade|CU|ATA|F"));

        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        assert!(s.value(MetricId::WaveWorkingBuyCount, Some(&named)).is_nan());
        s.on_trade(&other);
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&named)), 0.0);
        s.on_trade(&named_buy);
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&named)), 1.0);
        let mut named2 = buy(31, 4, 0.4);
        named2.template_hash = Some(grain_id_hash("Axiom Trade|CU|ATA|F"));
        s.on_trade(&named2);
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&named)), 2.0);
        assert!(s.value(MetricId::WaveWorkingBuyCount, None).is_nan());
    }

    fn axiom(slot: u64, txi: u32, wallet: u64, tip: Option<u64>) -> TradeLite {
        let mut t = buy(slot, wallet, 0.4);
        t.tx_index = Some(txi);
        t.template_hash = Some(grain_id_hash("Axiom Trade|CU|ATA|F"));
        t.fee = crate::metrics::fee::FeeKeys::new(None, None, tip);
        t
    }

    fn named_patterns() -> BurstPatterns {
        BurstPatterns::from_metric_config(&serde_json::json!({
            "m_burst_slot": {
                "working_templates": [
                    "Axiom Trade|CU|ATA|F",
                    "Axiom Trade|CU|ATA|N|F"
                ]
            }
        }))
        .unwrap()
    }

    #[test]
    fn this_working_is_this_print_on_the_fingerprint_list() {
        let p = named_patterns();
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&axiom(20, 1, 2, Some(200_000)));
        assert_eq!(s.value(MetricId::WaveThisWorking, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::WaveThisTip, None), 200_000.0);
        let mut pump = buy(20, 3, 1.0);
        pump.tx_index = Some(3);
        s.on_trade(&pump);
        assert_eq!(s.value(MetricId::WaveThisWorking, Some(&p)), 0.0);
        assert!(s.value(MetricId::WaveThisWorking, None).is_nan());
    }

    #[test]
    fn consecutive_slots_are_one_wave_and_second_working_completes() {
        let p = named_patterns();
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&axiom(20, 4, 2, Some(200_000)));
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&p)), 1.0);
        s.on_trade(&axiom(21, 1, 3, Some(200_000)));
        assert_eq!(s.value(MetricId::WaveGapSlots, None), 10.0);
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::WaveThisWorking, Some(&p)), 1.0);
    }

    #[test]
    fn hole_and_tip_seen_follow_every_curve_buy() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        let mut bare = buy(20, 2, 1.0);
        bare.template_hash = None;
        bare.tx_index = Some(1);
        bare.fee = crate::metrics::fee::FeeKeys::new(None, None, Some(200_000));
        s.on_trade(&bare);
        assert_eq!(s.value(MetricId::WaveThisMember, None), 0.0);
        s.on_trade(&axiom(20, 4, 3, Some(250_000)));
        assert_eq!(s.value(MetricId::WaveHole, None), 1.0);
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 1.0);
    }

    #[test]
    fn hole_is_wave_tx_index_gap_not_slot_prefix() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        let mut a = axiom(20, 5, 2, Some(200_000));
        s.on_trade(&a);
        assert_eq!(s.value(MetricId::WaveHole, None), 0.0);
        a.slot = 20;
        a.tx_index = Some(7);
        a.wallet_hash = 3;
        s.on_trade(&a);
        assert_eq!(s.value(MetricId::WaveHole, None), 1.0);
        // Next slot continues the wave; 1 - 7 is not a hole.
        a.slot = 21;
        a.tx_index = Some(1);
        a.wallet_hash = 4;
        s.on_trade(&a);
        assert_eq!(s.value(MetricId::WaveHole, None), 0.0);
    }

    #[test]
    fn missing_tx_index_is_minus_one() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        let mut first = axiom(20, 0, 2, Some(200_000));
        first.tx_index = None;
        s.on_trade(&first);
        assert_eq!(s.value(MetricId::WaveHole, None), 0.0);
        let second = axiom(20, 5, 3, Some(200_000));
        s.on_trade(&second);
        // 5 - (-1) > 1
        assert_eq!(s.value(MetricId::WaveHole, None), 1.0);
    }

    #[test]
    fn this_tip_is_nan_when_absent_and_zero_when_captured() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&axiom(20, 1, 2, None));
        assert!(s.value(MetricId::WaveThisTip, None).is_nan());
        s.on_trade(&axiom(20, 2, 3, Some(0)));
        assert_eq!(s.value(MetricId::WaveThisTip, None), 0.0);
        s.on_trade(&axiom(20, 3, 4, Some(250_000)));
        assert_eq!(s.value(MetricId::WaveThisTip, None), 250_000.0);
    }

    #[test]
    fn tip_seen_reads_any_prior_wave_buy_band() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        let mut pump = buy(20, 2, 1.0);
        pump.tx_index = Some(1);
        pump.fee = crate::metrics::fee::FeeKeys::new(None, None, Some(200_000));
        s.on_trade(&pump);
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 0.0);
        s.on_trade(&axiom(20, 3, 3, Some(250_000)));
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 1.0);
        s.on_trade(&axiom(20, 4, 4, Some(2_000_000)));
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 0.0);
    }

    #[test]
    fn tick_clears_per_print_facts() {
        let p = named_patterns();
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&axiom(20, 1, 2, Some(200_000)));
        s.on_trade(&axiom(20, 3, 3, Some(200_000)));
        assert_eq!(s.value(MetricId::WaveThisMember, None), 1.0);
        assert_eq!(s.value(MetricId::WaveHole, None), 1.0);
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 1.0);
        assert_eq!(s.value(MetricId::WaveThisWorking, Some(&p)), 1.0);
        s.on_tick();
        assert_eq!(s.value(MetricId::WaveThisMember, None), 0.0);
        assert_eq!(s.value(MetricId::WaveHole, None), 0.0);
        assert_eq!(s.value(MetricId::WaveTipSeen, None), 0.0);
        assert_eq!(s.value(MetricId::WaveThisWorking, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::WaveThisTip, None), 200_000.0);
        assert_eq!(s.value(MetricId::WaveWorkingBuyCount, Some(&p)), 2.0);
    }
}
