//! Feed-lag gauge — how far behind the chain the LaserStream feed runs.
//!
//! Every stage timer downstream (`snipe_latency`, `exit_latency`) starts at
//! `received_at`, the wall clock when a frame reached our socket. None of them can
//! see the time *before* that, and a transaction frame carries no chain clock to
//! compare against — the decoder stamps `block_time: received_at`, so
//! `received_at - block_time` is identically zero everywhere downstream. Block
//! metas are the one frame on the stream that does carry the chain's own clock, so
//! this is where the missing segment is measured.
//!
//! **Resolution is whole seconds** (`UnixTimestamp.timestamp`), so one sample
//! bounds lag rather than timing it: it separates "under a second" from "three
//! seconds behind" and cannot separate 40 ms from 300 ms. That is the split worth
//! having — a healthy feed sits at 0–1, and a backlogged or replaying one climbs
//! and stays there, which is invisible in every other counter.
//!
//! Runs ON THE TRANSPORT TASK: three atomic adds per slot (~2.5/s), a log line per
//! window, no allocation and no lock.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use chrono::Utc;
use tracing::{info, warn};

/// Slots per reporting window. ~150 slots × 400 ms ≈ 60 s.
const WINDOW_SLOTS: u64 = 150;

/// Lag (seconds) at or above which the window logs at WARN instead of INFO. One
/// second is a healthy feed's ceiling at this resolution; two means we are
/// deciding on stale state and every downstream timer is measuring the wrong race.
const WARN_LAG_SECS: i64 = 2;

/// Windowed `now - block_time` accumulator. All fields are per-window and reset
/// together by the thread that closes the window.
#[derive(Debug, Default)]
pub struct FeedLagGauge {
    /// Samples in the current window.
    samples: AtomicU64,
    /// Σ lag seconds — with `samples`, the window mean.
    sum_secs: AtomicI64,
    /// Worst lag seen in the window.
    max_secs: AtomicI64,
    /// Samples at or above [`WARN_LAG_SECS`].
    stale: AtomicU64,
    /// Highest slot observed — drives window rollover.
    last_slot: AtomicU64,
    /// Slot the current window opened at.
    window_start_slot: AtomicU64,
}

impl FeedLagGauge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one block meta. `block_time_unix` is `None` on a frame that omits it
    /// (the field is optional in the proto) — those are skipped, not counted as
    /// zero lag, so a stream that stops sending block times reports `n=0` rather
    /// than a healthy-looking mean.
    pub fn observe(&self, slot: u64, block_time_unix: Option<i64>) {
        let Some(block_time) = block_time_unix else { return };
        // Negative lag means our clock trails the validator's; clamp so a skewed
        // host clock cannot drag the mean below zero and mask real lag.
        let lag = (Utc::now().timestamp() - block_time).max(0);

        self.samples.fetch_add(1, Ordering::Relaxed);
        self.sum_secs.fetch_add(lag, Ordering::Relaxed);
        self.max_secs.fetch_max(lag, Ordering::Relaxed);
        if lag >= WARN_LAG_SECS {
            self.stale.fetch_add(1, Ordering::Relaxed);
        }

        // Slots arrive in order but a reconnect can rewind them; seed the window on
        // the first sample and on any backwards jump.
        let start = self.window_start_slot.load(Ordering::Relaxed);
        if start == 0 || slot < start {
            self.window_start_slot.store(slot, Ordering::Relaxed);
            self.last_slot.store(slot, Ordering::Relaxed);
            return;
        }
        self.last_slot.store(slot, Ordering::Relaxed);
        if slot.saturating_sub(start) >= WINDOW_SLOTS {
            self.close_window(slot);
        }
    }

    /// Log the closing window and reset. Only reached from `observe`, so exactly
    /// one caller is inside it per rollover.
    fn close_window(&self, slot: u64) {
        let n = self.samples.swap(0, Ordering::Relaxed);
        let sum = self.sum_secs.swap(0, Ordering::Relaxed);
        let max = self.max_secs.swap(0, Ordering::Relaxed);
        let stale = self.stale.swap(0, Ordering::Relaxed);
        self.window_start_slot.store(slot, Ordering::Relaxed);
        if n == 0 {
            return;
        }
        let mean = sum as f64 / n as f64;
        if max >= WARN_LAG_SECS {
            warn!(
                slots = n,
                mean_lag_secs = format!("{mean:.2}"),
                max_lag_secs = max,
                stale_slots = stale,
                "feed_lag"
            );
        } else {
            info!(
                slots = n,
                mean_lag_secs = format!("{mean:.2}"),
                max_lag_secs = max,
                stale_slots = stale,
                "feed_lag"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame with no chain clock is skipped, never counted as zero lag — else a
    /// stream that stops sending block times reads as a perfectly healthy feed.
    #[test]
    fn missing_block_time_is_not_zero_lag() {
        let g = FeedLagGauge::new();
        g.observe(100, None);
        assert_eq!(g.samples.load(Ordering::Relaxed), 0);
        assert_eq!(g.sum_secs.load(Ordering::Relaxed), 0);
    }

    /// A window closes on the slot span, not on sample count, and resets every
    /// accumulator — otherwise `max` latches forever and one bad slot condemns
    /// every later window.
    #[test]
    fn window_rollover_resets_accumulators() {
        let g = FeedLagGauge::new();
        let now = Utc::now().timestamp();
        g.observe(1_000, Some(now));
        assert_eq!(g.samples.load(Ordering::Relaxed), 1);
        // Same window — still accumulating.
        g.observe(1_000 + WINDOW_SLOTS - 1, Some(now));
        assert_eq!(g.samples.load(Ordering::Relaxed), 2);
        // Crosses the span ⇒ closes and resets.
        g.observe(1_000 + WINDOW_SLOTS, Some(now));
        assert_eq!(g.samples.load(Ordering::Relaxed), 0);
        assert_eq!(g.max_secs.load(Ordering::Relaxed), 0);
        assert_eq!(g.window_start_slot.load(Ordering::Relaxed), 1_000 + WINDOW_SLOTS);
    }

    /// A reconnect can rewind the slot counter; the window re-seeds instead of
    /// treating the rewind as a giant span and closing on every frame.
    #[test]
    fn backwards_slot_reseeds_the_window() {
        let g = FeedLagGauge::new();
        let now = Utc::now().timestamp();
        g.observe(5_000, Some(now));
        g.observe(10, Some(now));
        assert_eq!(g.window_start_slot.load(Ordering::Relaxed), 10);
    }

    /// A host clock behind the validator's must not produce negative lag — that
    /// would net against real lag from other slots and understate the mean.
    #[test]
    fn future_block_time_clamps_to_zero() {
        let g = FeedLagGauge::new();
        g.observe(1, Some(Utc::now().timestamp() + 30));
        assert_eq!(g.sum_secs.load(Ordering::Relaxed), 0);
        assert_eq!(g.samples.load(Ordering::Relaxed), 1);
    }
}
