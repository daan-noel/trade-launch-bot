//! `m_burst_wave` — this token's buys in the current consecutive-slot run.
//!
//! Static (no window). Token-level, not fingerprint-scoped: every member counts,
//! not a working-template list. A **wave** is consecutive buy-slots (no empty
//! buy-slot between them). It resets when the next buy is at least 2 slots after
//! the last buy-slot. The gap is empty buy-slots *before this wave started*, not
//! before a later printer in the same run.
//!
//! Create slot does not start a fireable wave: seed `creation_slot` so buys in
//! that slot (and consecutive slots after it) stay unfireable until a real gap.
//! Launch creates are not members. `this_member` clears on a tick.
//!
//! Completing print: `this_member = 1` and `wallet_count` crosses 2.

use crate::hash::HashedSet;
use super::burst_slot::is_member;
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
        }
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
        self.gap_slots = gap;
        self.fireable = fireable;
    }

    /// A tick is not a print — `this_member` must not survive into a later
    /// `can_enter` on a clock advance.
    pub fn on_tick(&mut self) {
        self.this_member = false;
    }

    pub fn on_trade(&mut self, t: &TradeLite) {
        self.this_member = false;
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
            if t.wallet_hash == 0 {
                self.wave_unknown = true;
            } else {
                self.wave_wals.insert(t.wallet_hash);
                self.seen.insert(t.wallet_hash);
            }
            self.wave_sol += t.sol;
        } else if t.wallet_hash != 0 {
            self.seen.insert(t.wallet_hash);
        }
        self.last_slot = Some(t.slot);
    }

    pub fn value(&self, id: MetricId) -> f64 {
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
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(s.value(MetricId::WaveGapSlots).is_nan());
        assert_eq!(s.value(MetricId::WaveWalletCount), 2.0);
        assert_eq!(s.value(MetricId::WaveThisMember), 1.0);
    }

    #[test]
    fn gap_is_before_the_wave_not_before_a_later_slot() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(100);
        s.on_trade(&buy(100, 1, 0.5));
        // Quiet of 25 empty slots, then two wallets in one slot.
        s.on_trade(&buy(126, 2, 1.0));
        assert_eq!(s.value(MetricId::WaveGapSlots), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 1.0);
        s.on_trade(&buy(126, 3, 1.5));
        assert_eq!(s.value(MetricId::WaveGapSlots), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 2.0);
        assert_eq!(s.value(MetricId::WaveBuySol), 2.5);
        assert_eq!(s.value(MetricId::WaveAllNew), 1.0);
        assert_eq!(s.value(MetricId::WaveThisMember), 1.0);
        // Consecutive slot continues the SAME wave; gap does not become 1.
        s.on_trade(&buy(127, 4, 0.4));
        assert_eq!(s.value(MetricId::WaveGapSlots), 26.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 3.0);
        assert_eq!(s.value(MetricId::WaveBuySol), 2.9);
    }

    #[test]
    fn all_new_is_every_wave_wallet_first_on_mint() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(10);
        s.on_trade(&buy(10, 1, 1.0));
        s.on_trade(&buy(40, 1, 1.0));
        s.on_trade(&buy(40, 2, 1.0));
        assert_eq!(s.value(MetricId::WaveAllNew), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 2.0);
    }

    #[test]
    fn tick_clears_this_member() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(1);
        s.on_trade(&buy(10, 1, 1.0));
        assert_eq!(s.value(MetricId::WaveThisMember), 1.0);
        s.on_tick();
        assert_eq!(s.value(MetricId::WaveThisMember), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 1.0);
    }

    #[test]
    fn launch_is_not_a_member_and_does_not_start_a_wave() {
        let mut s = BurstWaveState::default();
        s.seed_creation_slot(5);
        let mut launch = buy(5, 9, 3.0);
        launch.is_launch = true;
        s.on_trade(&launch);
        assert_eq!(s.value(MetricId::WaveThisMember), 0.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 0.0);
        assert!(s.value(MetricId::WaveGapSlots).is_nan());
        s.on_trade(&buy(20, 1, 1.0));
        assert_eq!(s.value(MetricId::WaveGapSlots), 15.0);
        assert_eq!(s.value(MetricId::WaveWalletCount), 1.0);
    }
}
