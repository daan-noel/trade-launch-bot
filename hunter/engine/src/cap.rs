//! [`Cap`] — a governance limit with its `0 = unlimited` storage encoding already
//! decoded away.
//!
//! Both `strategy_rules.max_total_tokens` and `strategy_rules.max_concurrent_tokens`
//! store `0` for "unlimited". That is the *legitimate* sentinel case (a cap of zero
//! is not a meaningful value of the domain — see hunter/CLAUDE.md *Zero-as-unbound*),
//! and it decodes in exactly ONE place. An ad-hoc decode re-derived at each site
//! (`max_total != 0 && counters.total >= max_total`, `(r.max_total_tokens != 0).then_some(..)`)
//! is how the two fingerprint sentinel bugs happened — a sentinel with two readers
//! fails in opposite directions.
//!
//! `Cap` is that one reader. `Cap::UNLIMITED` is `u32::MAX` rather than a
//! `None`/enum variant on purpose: [`Cap::allows`] stays a single `<` with no
//! branch or match, which matters on the per-event decision path.
//!
//! Deliberately NOT `Option<u32>` at the storage layer: making the columns
//! nullable would reintroduce a meaningless `Some(0)` state, and the rule PATCH
//! semantics (`apply_rule_update_patches`) would then need a three-state wire
//! (absent / null / value) that is easy to get wrong in a way which silently
//! clears fields.

use serde::{Deserialize, Serialize};

/// A decoded governance cap. Construct through one of the `from_*` decoders —
/// never from a raw stored number, so the sentinel branch cannot be forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cap(u32);

impl Cap {
    /// No bound. `u32::MAX` in practice, so `allows` never has to special-case it.
    pub const UNLIMITED: Cap = Cap(u32::MAX);

    /// Decode the stored `0 = unlimited` encoding — the ONE decoder, shared by
    /// both governance caps (`max_total_tokens`, `max_concurrent_tokens`).
    pub const fn zero_unlimited(stored: u32) -> Self {
        if stored == 0 {
            Self::UNLIMITED
        } else {
            Self(stored)
        }
    }

    /// An explicit bound, already decoded (test/sweep construction).
    pub const fn exactly(bound: u32) -> Self {
        Self(bound)
    }

    /// Is one more allowed given `count` already taken? The whole point of the
    /// newtype: the caller cannot forget the unlimited branch, and the hot path
    /// stays a single comparison.
    pub const fn allows(self, count: u32) -> bool {
        count < self.0
    }

    pub const fn is_unlimited(self) -> bool {
        self.0 == u32::MAX
    }

    /// The bound as a number when there is one — for display / persistence.
    /// `None` = unlimited (rendered as `∞`, stored back as `0`).
    pub const fn bounded(self) -> Option<u32> {
        if self.is_unlimited() {
            None
        } else {
            Some(self.0)
        }
    }

    /// The raw bound including the `u32::MAX` unlimited form. Only for callers
    /// that genuinely want a number to compare against (e.g. a sweep that runs
    /// caps-free); prefer [`allows`](Self::allows).
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_decodes_to_unlimited_and_never_blocks() {
        let cap = Cap::zero_unlimited(0);
        assert!(cap.is_unlimited());
        assert_eq!(cap.bounded(), None);
        assert!(cap.allows(0));
        assert!(cap.allows(u32::MAX - 1));
    }

    #[test]
    fn a_real_bound_blocks_at_the_bound() {
        let cap = Cap::zero_unlimited(3);
        assert_eq!(cap.bounded(), Some(3));
        assert!(cap.allows(2));
        assert!(!cap.allows(3));
        assert!(!cap.allows(4));
    }

    #[test]
    fn both_governance_caps_share_the_one_decoder() {
        // Concurrency and lifetime read `0` the same way — a blank field in the
        // rule editor is "no bound" on either, so there is no second decoder to
        // disagree with this one.
        assert_eq!(Cap::zero_unlimited(0), Cap::UNLIMITED);
        assert!(Cap::zero_unlimited(0).allows(4_000));
    }
}
