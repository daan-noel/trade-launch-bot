//! Slot-based block-time estimation for LaserStream replay paths.
//!
//! Yellowstone gRPC transaction frames carry slot numbers (exact, from chain)
//! but no `blockTime`. For live frames `received_at ≈ block_time`; for replayed
//! frames the gap can be hours. This module anchors one RPC `getBlockTime` call
//! at stream start and estimates any historical slot's time from it, giving
//! far more accurate `created_at` / `block_time` values than stamping `now()`.
//!
//! Error is ≈ ±slot_drift across the window — for a 10 h replay the absolute
//! error is minutes, not 10 h (the current bug). Good enough for a freshness
//! gate with a 30 s threshold.

use chrono::{DateTime, Duration, Utc};

/// Approximate Solana slot duration.
const SLOT_MS: i64 = 400;

/// A pinned (slot, wall-clock time) pair derived from one `getBlockTime` RPC
/// call. Used to estimate `block_time` for frames carrying only a slot number.
#[derive(Clone, Copy, Debug)]
pub struct SlotAnchor {
    pub slot: u64,
    pub time: DateTime<Utc>,
}

impl SlotAnchor {
    pub fn new(slot: u64, time: DateTime<Utc>) -> Self {
        Self { slot, time }
    }

    /// Estimate the wall-clock time of `tx_slot` using the anchor as a reference.
    /// For slots newer than the anchor the estimate is the anchor time + delta;
    /// for older slots it is anchor time − delta. The 400 ms/slot constant keeps
    /// the error to minutes even over a 10 h gap — far better than `now()`.
    pub fn estimate_block_time(&self, tx_slot: u64) -> DateTime<Utc> {
        let delta_slots = tx_slot as i64 - self.slot as i64;
        self.time + Duration::milliseconds(delta_slots * SLOT_MS)
    }

    /// True when `tx_slot` is significantly behind the anchor — i.e. this frame
    /// is a replay, not live. "Significant" is defined as more than one Solana
    /// epoch (~2 s) to avoid mis-classifying a legitimately slow tip update.
    pub fn is_replay(&self, tx_slot: u64) -> bool {
        self.slot.saturating_sub(tx_slot) > 5
    }
}
