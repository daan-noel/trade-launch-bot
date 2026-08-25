//! Cross-transport signature dedupe — the guard that lets two feeds run at once.
//!
//! Two transports can deliver the same transaction:
//!
//! - **During a source switch.** The new curve feed starts before the old one
//!   stops, so nothing is lost in the handover; the overlap is duplicated.
//! - **In steady state.** A migration transaction touches both the venue program
//!   (carried by the curve feed) and a freshly-tracked pool PDA (carried by the
//!   gRPC AMM filter), so both transports match it.
//!
//! Without this, both would decode and the host would book the trade twice.
//!
//! # Shape
//!
//! A fixed, pre-allocated ring of `AtomicU64` slots holding the first 8 bytes of
//! each signature. No locks, no allocation per transaction, no background sweep —
//! it sits directly on the hot path of every transport task.
//!
//! **The window is enforced by capacity, not by a clock**: a signature is
//! remembered until roughly `capacity` further signatures have passed through.
//! [`SignatureDedupe::for_window`] sizes the ring from a duration and a
//! deliberately generous throughput ceiling, so the real window is at least the
//! one asked for and usually much longer.
//!
//! A false *positive* (dropping a distinct transaction) needs two signatures that
//! agree on all 8 leading bytes — 2^-64, i.e. never. False *negatives* (a
//! duplicate slipping through) are possible only once the ring has wrapped, which
//! is exactly the intended expiry.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Transactions per second the ring is sized to absorb. Well above the observed
/// pump.fun curve rate; over-sizing costs 8 bytes a slot and buys margin against
/// a burst silently shortening the window.
const ASSUMED_PEAK_TPS: u64 = 4_000;

/// Slots probed from the hash position before giving up and overwriting.
const PROBE: usize = 4;

/// Lock-free bounded set of recently-seen signatures.
pub struct SignatureDedupe {
    slots: Box<[AtomicU64]>,
    mask: usize,
}

impl SignatureDedupe {
    /// Size the ring so `window` worth of traffic fits at [`ASSUMED_PEAK_TPS`].
    ///
    /// Rounded up to a power of two, floored at 4096 slots (32 KB) and capped at
    /// 1M slots (8 MB) so a misconfigured window cannot balloon on the 4 GB
    /// deploy box.
    pub fn for_window(window: Duration) -> Self {
        let want = window.as_secs().max(1).saturating_mul(ASSUMED_PEAK_TPS);
        let capacity = want.clamp(4_096, 1 << 20).next_power_of_two() as usize;
        Self::with_capacity(capacity)
    }

    /// `capacity` is rounded up to a power of two.
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(PROBE).next_power_of_two();
        let slots = (0..capacity)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            mask: capacity - 1,
        }
    }

    /// Record `signature`, returning `true` if it had **not** been seen.
    ///
    /// `false` means a duplicate — the caller drops the transaction.
    /// A signature shorter than 8 bytes is malformed; it is admitted rather than
    /// dropped, so a bad frame surfaces downstream instead of vanishing here.
    pub fn insert_new(&self, signature: &[u8]) -> bool {
        let Some(head) = signature.get(..8) else {
            return true;
        };
        // 0 is the vacant marker, so fold it away rather than losing the slot.
        let fingerprint = match u64::from_le_bytes(head.try_into().expect("8 bytes")) {
            0 => 1,
            v => v,
        };
        let home = (fingerprint as usize) & self.mask;

        for i in 0..PROBE {
            let slot = &self.slots[(home + i) & self.mask];
            match slot.compare_exchange(0, fingerprint, Ordering::AcqRel, Ordering::Acquire) {
                // Claimed a vacant slot — first sighting.
                Ok(_) => return true,
                // Occupied: by us (duplicate) or by someone else (keep probing).
                Err(existing) if existing == fingerprint => return false,
                Err(_) => {}
            }
        }

        // The probe window is full of other signatures. Evict the home slot; this
        // is the ring wrapping, which is how entries expire.
        self.slots[home].store(fingerprint, Ordering::Release);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(n: u64) -> Vec<u8> {
        let mut v = vec![0u8; 64];
        v[..8].copy_from_slice(&n.to_le_bytes());
        v
    }

    #[test]
    fn first_sighting_is_new_and_the_repeat_is_not() {
        let d = SignatureDedupe::with_capacity(1024);
        assert!(d.insert_new(&sig(42)));
        assert!(!d.insert_new(&sig(42)));
        assert!(!d.insert_new(&sig(42)));
    }

    #[test]
    fn distinct_signatures_all_pass() {
        let d = SignatureDedupe::with_capacity(4096);
        for n in 1..1000u64 {
            assert!(d.insert_new(&sig(n)), "signature {n} wrongly deduped");
        }
    }

    /// The overlap a source switch produces: the same batch arriving twice, from
    /// two transports, must decode exactly once.
    #[test]
    fn a_switch_overlap_is_absorbed() {
        let d = SignatureDedupe::with_capacity(4096);
        let batch: Vec<_> = (1..200u64).map(sig).collect();
        let from_grpc = batch.iter().filter(|s| d.insert_new(s)).count();
        let from_nats = batch.iter().filter(|s| d.insert_new(s)).count();
        assert_eq!(from_grpc, 199);
        assert_eq!(from_nats, 0);
    }

    #[test]
    fn a_zero_prefix_signature_still_dedupes() {
        let d = SignatureDedupe::with_capacity(64);
        assert!(d.insert_new(&sig(0)));
        assert!(!d.insert_new(&sig(0)));
    }

    #[test]
    fn a_malformed_short_signature_is_admitted() {
        let d = SignatureDedupe::with_capacity(64);
        assert!(d.insert_new(&[1, 2, 3]));
        assert!(d.insert_new(&[1, 2, 3]));
    }

    #[test]
    fn window_sizing_is_clamped_and_power_of_two() {
        let tiny = SignatureDedupe::for_window(Duration::from_secs(0));
        assert_eq!(tiny.slots.len(), 4_096);
        let huge = SignatureDedupe::for_window(Duration::from_secs(86_400));
        assert_eq!(huge.slots.len(), 1 << 20);
        let normal = SignatureDedupe::for_window(Duration::from_secs(30));
        assert!(normal.slots.len().is_power_of_two());
        assert!(normal.slots.len() >= 30 * ASSUMED_PEAK_TPS as usize);
    }
}
