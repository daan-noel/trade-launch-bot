//! `m_position` — position-scoped metrics (static; **exit-only**).
//!
//! Every other group is token-scoped: one value per token, shared by every rule
//! armed on it. These anchor on **your entry fill**, so they only have a value
//! while a position is held. That state — the entry price, the since-entry peak,
//! and the entry time — is the [`PositionCtx`], carried on
//! [`ArmState::Entered`](crate::arm::ArmState) and folded forward each event.
//!
//! * `retrace` — percent below the **since-entry peak**: `(peak − price) / peak ·
//!   100`, `>= 0`. The trailing stop (`retrace >= 3` = a 3% trail). At the entry
//!   fill the peak IS the fill price, so before price rises `retrace` measures the
//!   drop from entry (a soft stop); after a run-up it trails the new peak.
//! * `pnl` — signed percent vs the entry price: `(price − entry) / entry · 100`.
//!   TP/SL desugar into this (`pnl >= tp` / `pnl <= −sl`) — one exit computation for
//!   the ladder and for authored `m_position.pnl` conditions (see `arm.rs`).
//! * `held` — seconds since the entry fill (floored at zero against block-time
//!   regression, the `stall` precedent). Gives time-stop exits for free.
//!
//! Before entry there is no context, so a position metric reads `NaN` (the engine
//! convention: `NaN` satisfies no condition). That is exactly why the entry-side
//! `can_enter` gate needs no special case — with no position, these never fire.

use super::{secs_between, MetricId, Ts};

/// The since-entry state a position metric reads. Built from
/// [`ArmState::Entered`](crate::arm::ArmState) on each open-side evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionCtx {
    /// The entry fill price (`pnl` reference).
    pub entry_price: f64,
    /// Highest price observed since entry (`retrace` reference), folded per event.
    pub peak_price: f64,
    /// The entry fill time (`held` reference).
    pub entered_at: Ts,
}

impl PositionCtx {
    /// `retrace` — percent below the since-entry peak; `NaN` if the peak is
    /// non-positive or `price` is non-finite.
    pub fn retrace(&self, price: f64) -> f64 {
        if self.peak_price > 0.0 && price.is_finite() {
            (self.peak_price - price) / self.peak_price * 100.0
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
}

/// Value of one `m_position` metric given the position context, the current price,
/// and `now`. Non-position ids yield `NaN` (unreachable — the fold routes by scope).
pub fn position_value(id: MetricId, ctx: &PositionCtx, price: f64, now: Ts) -> f64 {
    match id {
        MetricId::Retrace => ctx.retrace(price),
        MetricId::Pnl => ctx.pnl(price),
        MetricId::Held => ctx.held(now),
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

    fn ctx(entry: f64, peak: f64, entered: f64) -> PositionCtx {
        PositionCtx { entry_price: entry, peak_price: peak, entered_at: ts(entered) }
    }

    #[test]
    fn retrace_is_percent_below_since_entry_peak() {
        let c = ctx(1.0, 1.5, 0.0);
        // At the peak → 0; 20% below the 1.5 peak → 20.
        assert_eq!(c.retrace(1.5), 0.0);
        assert!((c.retrace(1.2) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn pnl_is_signed_percent_vs_entry() {
        let c = ctx(1.0, 1.0, 0.0);
        assert!((c.pnl(1.3) - 30.0).abs() < 1e-9);
        assert!((c.pnl(0.75) - -25.0).abs() < 1e-9);
    }

    #[test]
    fn held_counts_from_entry_and_floors_at_zero() {
        let c = ctx(1.0, 1.0, 10.0);
        assert_eq!(c.held(ts(25.0)), 15.0);
        // Regressed block_time → floored, never negative.
        assert_eq!(c.held(ts(4.0)), 0.0);
    }

    #[test]
    fn non_finite_price_or_bad_reference_is_nan() {
        assert!(ctx(1.0, 1.0, 0.0).retrace(f64::NAN).is_nan());
        assert!(ctx(1.0, 1.0, 0.0).pnl(f64::NAN).is_nan());
        assert!(ctx(0.0, 0.0, 0.0).retrace(1.0).is_nan()); // non-positive peak
        assert!(ctx(0.0, 1.0, 0.0).pnl(1.0).is_nan()); // non-positive entry
    }

    #[test]
    fn position_value_routes_each_metric() {
        let c = ctx(1.0, 2.0, 0.0);
        assert!((position_value(MetricId::Retrace, &c, 1.6, ts(5.0)) - 20.0).abs() < 1e-9);
        assert!((position_value(MetricId::Pnl, &c, 1.6, ts(5.0)) - 60.0).abs() < 1e-9);
        assert_eq!(position_value(MetricId::Held, &c, 1.6, ts(5.0)), 5.0);
        // A token-scoped id is not a position metric → NaN.
        assert!(position_value(MetricId::Trail, &c, 1.6, ts(5.0)).is_nan());
    }
}
