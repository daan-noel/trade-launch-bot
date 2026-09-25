//! `m_position` — our own trade. Exists only while a position is held, so its metrics
//! are for sell lines.
//!
//! Every other family is coin-scoped: one value per coin, shared by every rule armed on
//! it. These anchor on **our fill**, so they only have a value while we hold. That state
//! — the entry price, the since-entry peak and trough, the entry time and when the
//! current stage began — is the [`PositionCtx`], carried on
//! [`ArmState::Entered`](crate::arm::ArmState) and folded forward each event.
//!
//! * `retrace_pct` — percent below the **since-entry peak**: `(peak - price) / peak ·
//!   100`. At the fill the peak IS the fill price, so before price rises it measures
//!   the drop from entry (a soft stop); after a run-up it trails the new peak.
//! * `bounce_pct` — percent above the **since-entry trough**, the twin.
//! * `pnl_pct` — signed percent vs the entry price. Take profit and stop loss compile
//!   to lines on it.
//! * `held_sec` — seconds since the fill (floored at zero against block-time regression).
//! * `room_taken_pct` — percent of the entry's room to the graduation wall covered:
//!   `(price - entry) / (wall - entry) · 100`, `wall = entry · (115 / vsol at fill)²`.
//! * `stage_sec` — seconds since the rule entered its current stage.
//!
//! Before entry there is no context, so a position metric reads `NaN` (which satisfies
//! nothing) — why the pre-entry veto needs no special case.

use super::{secs_between, Metric, Ts};

/// Priced SOL reserve (`vsol`) of a pump.fun bonding curve at graduation: the 30
/// virtual SOL every curve starts with plus the 85 real SOL that completes it. The
/// `room_taken` wall. Core's `PUMP_INITIAL_VIRTUAL_SOL + PUMP_GRADUATION_REAL_SOL`
/// is the same number; a core test asserts the two stay equal.
pub const GRADUATION_PRICED_RESERVE_SOL: f64 = 115.0;

/// The since-entry state a position metric reads. Built from
/// [`ArmState::Entered`](crate::arm::ArmState) on each open-side evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionCtx {
    /// The entry fill price (`pnl_pct` reference).
    pub entry_price: f64,
    /// Highest price observed since entry (`retrace_pct` reference), folded per event.
    pub peak_price: f64,
    /// Lowest price observed since entry (`bounce_pct` reference), folded per event.
    pub trough_price: f64,
    /// The entry fill time (`held_sec` reference).
    pub entered_at: Ts,
    /// When the rule entered its current stage (`stage_sec` reference). The fill time
    /// for the first stage.
    pub stage_since: Ts,
    /// Priced SOL reserve (`vsol`) of the last print folded when the entry filled
    /// (`room_taken_pct` reference). `NaN` when unknown, and `room_taken_pct` then reads
    /// `NaN`.
    pub entry_priced_reserve: f64,
}

impl PositionCtx {
    /// Fresh context at the entry fill — peak and trough both seed to the fill, the
    /// first stage starts at the fill.
    pub fn at_fill(entry_price: f64, entered_at: Ts) -> Self {
        Self {
            entry_price,
            peak_price: entry_price,
            trough_price: entry_price,
            entered_at,
            stage_since: entered_at,
            entry_priced_reserve: f64::NAN,
        }
    }

    /// The same context with the entry's priced reserve set (`room_taken_pct` reference).
    pub fn with_entry_priced_reserve(mut self, vsol: f64) -> Self {
        self.entry_priced_reserve = vsol;
        self
    }

    /// Ratchet peak up / trough down for one finite price. No-op on a non-finite one.
    pub fn fold_price(&mut self, price: f64) {
        if !price.is_finite() {
            return;
        }
        if price > self.peak_price {
            self.peak_price = price;
        }
        if price < self.trough_price {
            self.trough_price = price;
        }
    }

    /// `retrace` — percent below the since-entry peak; `NaN` if the peak is
    /// non-positive or `price` is non-finite.
    pub fn retrace(&self, price: f64) -> f64 {
        if self.peak_price > 0.0 && price.is_finite() {
            (self.peak_price - price) / self.peak_price * 100.0
        } else {
            f64::NAN
        }
    }

    /// `bounce` — percent above the since-entry trough; `NaN` if the trough is
    /// non-positive or `price` is non-finite.
    pub fn bounce(&self, price: f64) -> f64 {
        if self.trough_price > 0.0 && price.is_finite() {
            (price - self.trough_price) / self.trough_price * 100.0
        } else {
            f64::NAN
        }
    }

    /// `pnl` — signed percent vs the entry price; `NaN` if the entry price is
    /// non-positive or `price` is non-finite.
    pub fn pnl(&self, price: f64) -> f64 {
        if self.entry_price > 0.0 && price.is_finite() {
            (price - self.entry_price) / self.entry_price * 100.0
        } else {
            f64::NAN
        }
    }

    /// `held` — seconds since the entry fill, floored at zero (block-time can
    /// regress a few seconds across slots — never read negative held).
    pub fn held(&self, now: Ts) -> f64 {
        secs_between(self.entered_at, now).max(0.0)
    }

    /// `stage_sec` — seconds since the current stage began, floored at zero.
    pub fn stage_sec(&self, now: Ts) -> f64 {
        secs_between(self.stage_since, now).max(0.0)
    }

    /// `room_taken`: `pnl` as a percent of the entry's room to the graduation wall,
    /// `((GRADUATION_PRICED_RESERVE_SOL / vsol)² − 1) · 100`. `NaN` without a positive
    /// entry reserve, or with no room left (an entry at or past the wall).
    pub fn room_taken(&self, price: f64) -> f64 {
        let v = self.entry_priced_reserve;
        if !(v.is_finite() && v > 0.0) {
            return f64::NAN;
        }
        let room_pct = ((GRADUATION_PRICED_RESERVE_SOL / v).powi(2) - 1.0) * 100.0;
        if room_pct > 0.0 {
            self.pnl(price) / room_pct * 100.0
        } else {
            f64::NAN
        }
    }
}

/// Value of one `m_position` metric given the position context, the current price,
/// and `now`. Non-position ids yield `NaN` (unreachable — the fold routes by scope).
pub fn position_value(id: Metric, ctx: &PositionCtx, price: f64, now: Ts) -> f64 {
    match id {
        Metric::RetracePct => ctx.retrace(price),
        Metric::BouncePct => ctx.bounce(price),
        Metric::PnlPct => ctx.pnl(price),
        Metric::HeldSec => ctx.held(now),
        Metric::RoomTakenPct => ctx.room_taken(price),
        Metric::StageSec => ctx.stage_sec(now),
        _ => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn ctx(entry: f64, peak: f64, trough: f64, entered: f64) -> PositionCtx {
        PositionCtx {
            entry_price: entry,
            peak_price: peak,
            trough_price: trough,
            entered_at: ts(entered),
            stage_since: ts(entered),
            entry_priced_reserve: f64::NAN,
        }
    }

    #[test]
    fn retrace_is_percent_below_since_entry_peak() {
        let c = ctx(1.0, 1.5, 1.0, 0.0);
        // At the peak → 0; 20% below the 1.5 peak → 20.
        assert_eq!(c.retrace(1.5), 0.0);
        assert!((c.retrace(1.2) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn bounce_is_percent_above_since_entry_trough() {
        let c = ctx(1.0, 1.0, 0.8, 0.0);
        // At the trough → 0; 25% above the 0.8 trough → 25.
        assert_eq!(c.bounce(0.8), 0.0);
        assert!((c.bounce(1.0) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn bounce_equals_pnl_when_trough_stays_at_entry() {
        let c = PositionCtx::at_fill(1.0, ts(0.0));
        assert!((c.bounce(1.3) - c.pnl(1.3)).abs() < 1e-9);
    }

    #[test]
    fn fold_price_ratchets_peak_and_trough() {
        let mut c = PositionCtx::at_fill(1.0, ts(0.0));
        c.fold_price(1.5);
        assert_eq!(c.peak_price, 1.5);
        assert_eq!(c.trough_price, 1.0);
        c.fold_price(0.8);
        assert_eq!(c.peak_price, 1.5);
        assert_eq!(c.trough_price, 0.8);
        c.fold_price(f64::NAN); // ignored
        assert_eq!((c.peak_price, c.trough_price), (1.5, 0.8));
    }

    #[test]
    fn pnl_is_signed_percent_vs_entry() {
        let c = ctx(1.0, 1.0, 1.0, 0.0);
        assert!((c.pnl(1.3) - 30.0).abs() < 1e-9);
        assert!((c.pnl(0.75) - -25.0).abs() < 1e-9);
    }

    #[test]
    fn held_counts_from_entry_and_floors_at_zero() {
        let c = ctx(1.0, 1.0, 1.0, 10.0);
        assert_eq!(c.held(ts(25.0)), 15.0);
        // Regressed block_time → floored, never negative.
        assert_eq!(c.held(ts(4.0)), 0.0);
    }

    #[test]
    fn room_taken_is_pnl_over_the_entrys_room_to_the_wall() {
        // vsol 57.5 at the fill: the wall price is (115 / 57.5)^2 = 4x the entry, so
        // the room is +300 % and +120 % covers 40 % of it.
        let c = PositionCtx::at_fill(1.0, ts(0.0)).with_entry_priced_reserve(57.5);
        assert!((c.room_taken(2.2) - 40.0).abs() < 1e-9);
        assert!((c.room_taken(4.0) - 100.0).abs() < 1e-9);
        assert!((c.room_taken(0.7) - -10.0).abs() < 1e-9);
        // The rule-1b target, 40 % of the room, at the depths the rule enters.
        for (vsol, target_pct) in [(100.0, 12.9), (90.0, 25.3), (80.0, 42.7), (70.0, 68.0)] {
            let c = PositionCtx::at_fill(1.0, ts(0.0)).with_entry_priced_reserve(vsol);
            let at_target = 1.0 + target_pct / 100.0;
            assert!((c.room_taken(at_target) - 40.0).abs() < 0.1, "vsol {vsol}");
        }
    }

    #[test]
    fn room_taken_is_nan_without_a_reserve_or_room() {
        assert!(PositionCtx::at_fill(1.0, ts(0.0)).room_taken(1.2).is_nan());
        let at_wall = PositionCtx::at_fill(1.0, ts(0.0))
            .with_entry_priced_reserve(GRADUATION_PRICED_RESERVE_SOL);
        assert!(at_wall.room_taken(1.2).is_nan());
        let past = PositionCtx::at_fill(1.0, ts(0.0)).with_entry_priced_reserve(120.0);
        assert!(past.room_taken(1.2).is_nan());
    }

    #[test]
    fn non_finite_price_or_bad_reference_is_nan() {
        assert!(ctx(1.0, 1.0, 1.0, 0.0).retrace(f64::NAN).is_nan());
        assert!(ctx(1.0, 1.0, 1.0, 0.0).bounce(f64::NAN).is_nan());
        assert!(ctx(1.0, 1.0, 1.0, 0.0).pnl(f64::NAN).is_nan());
        assert!(ctx(0.0, 0.0, 0.0, 0.0).retrace(1.0).is_nan()); // non-positive peak
        assert!(ctx(1.0, 1.0, 0.0, 0.0).bounce(1.0).is_nan()); // non-positive trough
        assert!(ctx(0.0, 1.0, 1.0, 0.0).pnl(1.0).is_nan()); // non-positive entry
    }

    #[test]
    fn position_value_routes_each_metric() {
        let c = ctx(1.0, 2.0, 0.8, 0.0);
        assert!((position_value(Metric::RetracePct, &c, 1.6, ts(5.0)) - 20.0).abs() < 1e-9);
        assert!((position_value(Metric::BouncePct, &c, 1.0, ts(5.0)) - 25.0).abs() < 1e-9);
        assert!((position_value(Metric::PnlPct, &c, 1.6, ts(5.0)) - 60.0).abs() < 1e-9);
        assert_eq!(position_value(Metric::HeldSec, &c, 1.6, ts(5.0)), 5.0);
        let staged = PositionCtx { stage_since: ts(3.0), ..c };
        assert_eq!(position_value(Metric::StageSec, &staged, 1.6, ts(5.0)), 2.0);
        let deep = c.with_entry_priced_reserve(57.5);
        assert!((position_value(Metric::RoomTakenPct, &deep, 2.2, ts(5.0)) - 40.0).abs() < 1e-9);
        // A token-scoped id is not a position metric → NaN.
        assert!(position_value(Metric::TrailPct, &c, 1.6, ts(5.0)).is_nan());
    }
}
