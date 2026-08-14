//! Duplicate-identity guard — "don't buy the same trap twice".
//!
//! A scam re-launch keeps the name and icon and mints a fresh address, so the
//! per-token gates (`exclusive`, the caps, re-entry) all see a brand-new token
//! and let it through. This guard remembers the [`IdentityHash`] of every entry
//! the engine *attempts* and refuses a later entry on a **different mint** with
//! the same identity, for a rolling window.
//!
//! Four things it is deliberately not:
//!
//! * **Not per-rule.** The memory is one portfolio-lifetime object that every arm
//!   writes to; a per-rule read gate over a globally-written blocklist would mean
//!   rule A's buy silently changes rule B's behavior. One global switch instead
//!   (`strategy.skip_duplicate_identity`), applied to every rule.
//! * **Not fill-gated.** A *failed* entry counts. The attempt is the evidence that
//!   the identity looked tradeable, and a copycat that reverts our buy is exactly
//!   the case worth remembering.
//! * **Not mint-blind.** Recording the attempt would otherwise block the very
//!   token that produced it — including its own retry ladder. Entries carry their
//!   mint and only a *different* mint is blocked.
//! * **Not mode-shared.** Paper and real keep separate memories, so a paper
//!   experiment never narrows what the real rules may buy (the same mistake the
//!   mode-blind run cache made).
//!
//! Cost: one `BTreeMap` lookup per armed entry decision, over a map that holds at
//! most one entry per identity traded inside the window.

use chrono::Duration;
use smallvec::SmallVec;
use std::collections::BTreeMap;

use crate::event::{Mint, TradeMode};
use crate::identity::IdentityHash;
use crate::metrics::Ts;

/// Default memory horizon: 4 hours. Copycats re-launch within hours, so this
/// covers the re-launch pattern; anything longer mostly bans common names —
/// every `PEPE`/`MOON` on the curve shares an identity with the last one, and a
/// multi-day horizon turns that base rate into a blanket skip whose cost is
/// invisible (a trade that never happened leaves no evidence).
/// Operator-tunable via `strategy.duplicate_identity_window_hours`.
pub const DEFAULT_WINDOW_HOURS: u64 = 4;

/// How often (in **event** time, never wall clock) expired entries are swept.
/// The map is small; this only keeps the tick from walking it 5× a second.
const PRUNE_INTERVAL_SECS: i64 = 60;

/// Mints retained per identity. With the guard on, one identity can only ever
/// gain a new mint once per window, so this is slack for the enable-mid-flight
/// case — never a per-event growth path. Oldest is dropped first.
const MAX_MINTS_PER_IDENTITY: usize = 8;

/// Recently-traded identities, split by trade mode.
///
/// Disabled by default and empty until [`set_policy`](Self::set_policy) turns it
/// on — an engine that never enables it pays one `bool` check per entry decision.
#[derive(Debug, Clone)]
pub struct DupeGuard {
    enabled: bool,
    window: Duration,
    paper: Memory,
    real: Memory,
    /// Event-time of the next sweep (`None` ⇒ sweep on the next call).
    next_prune_at: Option<Ts>,
}

/// One mode's memory: identity → the mints traded under it, newest last.
type Memory = BTreeMap<IdentityHash, SmallVec<[(Mint, Ts); 1]>>;

impl Default for DupeGuard {
    fn default() -> Self {
        Self {
            enabled: false,
            window: Duration::hours(DEFAULT_WINDOW_HOURS as i64),
            paper: Memory::new(),
            real: Memory::new(),
            next_prune_at: None,
        }
    }
}

impl DupeGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the operator policy. `window_hours` of 0 falls back to the default
    /// rather than degenerating into "block nothing" — a zero window would look
    /// enabled while silently doing nothing, the exact false-green shape this
    /// codebase keeps getting bitten by.
    pub fn set_policy(&mut self, enabled: bool, window_hours: u64) {
        self.enabled = enabled;
        let hours = if window_hours == 0 { DEFAULT_WINDOW_HOURS } else { window_hours };
        self.window = Duration::hours(hours as i64);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn window(&self) -> Duration {
        self.window
    }

    /// Whether an entry on `mint` must be skipped: the guard is on, the token has
    /// an identity, and a **different** mint traded that identity inside the
    /// window.
    ///
    /// Reads only — a blocked decision records nothing, so a rule that keeps
    /// re-deciding cannot extend its own block.
    pub fn blocks(
        &self,
        mode: TradeMode,
        identity: Option<IdentityHash>,
        mint: &Mint,
        now: Ts,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(id) = identity else { return false };
        let Some(entries) = self.memory(mode).get(&id) else { return false };
        let cutoff = now - self.window;
        entries.iter().any(|(m, at)| m != mint && *at > cutoff)
    }

    /// Remember an entry **attempt** on `mint`. No-op when the token has no
    /// identity, and no-op while the guard is off.
    ///
    /// Not recording while disabled is the point of "start from now": switching the
    /// guard on begins an empty memory it fills going forward, matching what the
    /// boot rebuild does with `duplicate_identity_since`. If a disabled guard kept
    /// recording, flipping the toggle would retroactively block a week of entries
    /// the operator never opted into — the opposite of a gentle rollout.
    pub fn record(&mut self, mode: TradeMode, identity: Option<IdentityHash>, mint: &Mint, at: Ts) {
        if !self.enabled {
            return;
        }
        let Some(id) = identity else { return };
        let entries = self.memory_mut(mode).entry(id).or_default();
        match entries.iter_mut().find(|(m, _)| m == mint) {
            // Same mint again (a retry, a re-entry episode): keep the newest
            // attempt so the identity stays warm for the full window.
            Some(existing) if existing.1 < at => existing.1 = at,
            Some(_) => {}
            None => {
                if entries.len() >= MAX_MINTS_PER_IDENTITY {
                    entries.remove(0);
                }
                entries.push((mint.clone(), at));
            }
        }
    }

    /// Drop entries past the window. Driven by event time from the tick, so a
    /// replay prunes exactly where live did.
    pub fn prune(&mut self, now: Ts) {
        if self.next_prune_at.is_some_and(|t| now < t) {
            return;
        }
        self.next_prune_at = Some(now + Duration::seconds(PRUNE_INTERVAL_SECS));
        let cutoff = now - self.window;
        for memory in [&mut self.paper, &mut self.real] {
            memory.retain(|_, mints| {
                mints.retain(|(_, at)| *at > cutoff);
                !mints.is_empty()
            });
        }
    }

    /// Identities currently remembered for `mode` (diagnostics + tests).
    pub fn len(&self, mode: TradeMode) -> usize {
        self.memory(mode).len()
    }

    pub fn is_empty(&self, mode: TradeMode) -> bool {
        self.memory(mode).is_empty()
    }

    fn memory(&self, mode: TradeMode) -> &Memory {
        match mode {
            TradeMode::Paper => &self.paper,
            TradeMode::Real => &self.real,
        }
    }

    fn memory_mut(&mut self, mode: TradeMode) -> &mut Memory {
        match mode {
            TradeMode::Paper => &mut self.paper,
            TradeMode::Real => &mut self.real,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::token_identity_hash;
    use chrono::{TimeZone, Utc};

    /// The default horizon in seconds. Derived, never a literal: a test that
    /// spells the window out passes for the wrong reason the day the default
    /// moves — it would still assert against the old horizon.
    const WINDOW_SECS: i64 = DEFAULT_WINDOW_HOURS as i64 * 3600;

    fn ts(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn guard() -> DupeGuard {
        let mut g = DupeGuard::new();
        g.set_policy(true, DEFAULT_WINDOW_HOURS);
        g
    }

    fn id() -> Option<IdentityHash> {
        token_identity_hash("Moon Dog", "MDOG")
    }

    #[test]
    fn a_copycat_mint_is_blocked_and_the_original_is_not() {
        let mut g = guard();
        let first = Mint::from("mint-a");
        let copycat = Mint::from("mint-b");
        g.record(TradeMode::Real, id(), &first, ts(0));

        assert!(g.blocks(TradeMode::Real, id(), &copycat, ts(60)));
        // The token that produced the record must stay tradeable — this is what
        // keeps an entry's own retry ladder from being blocked by its own attempt.
        assert!(!g.blocks(TradeMode::Real, id(), &first, ts(60)));
    }

    #[test]
    fn paper_and_real_memories_are_independent() {
        let mut g = guard();
        let a = Mint::from("mint-a");
        let b = Mint::from("mint-b");
        g.record(TradeMode::Paper, id(), &a, ts(0));

        assert!(g.blocks(TradeMode::Paper, id(), &b, ts(60)));
        assert!(!g.blocks(TradeMode::Real, id(), &b, ts(60)), "paper must not gate real");
    }

    #[test]
    fn a_record_older_than_the_window_stops_blocking() {
        let mut g = guard();
        let a = Mint::from("mint-a");
        let b = Mint::from("mint-b");
        g.record(TradeMode::Real, id(), &a, ts(0));

        let almost = WINDOW_SECS - 1;
        assert!(g.blocks(TradeMode::Real, id(), &b, ts(almost)));
        assert!(!g.blocks(TradeMode::Real, id(), &b, ts(almost + 2)));
    }

    #[test]
    fn disabled_guard_blocks_nothing_even_with_a_record() {
        let mut g = guard();
        let a = Mint::from("mint-a");
        g.record(TradeMode::Real, id(), &a, ts(0));
        g.set_policy(false, DEFAULT_WINDOW_HOURS);
        assert!(!g.blocks(TradeMode::Real, id(), &Mint::from("mint-b"), ts(60)));
    }

    /// "Start from now": a disabled guard records nothing, so switching it on begins
    /// an empty memory rather than retroactively blocking a week of entries the
    /// operator never opted into.
    #[test]
    fn a_disabled_guard_records_nothing() {
        let mut g = DupeGuard::new();
        g.record(TradeMode::Real, id(), &Mint::from("mint-a"), ts(0));
        assert!(g.is_empty(TradeMode::Real));

        g.set_policy(true, DEFAULT_WINDOW_HOURS);
        assert!(
            !g.blocks(TradeMode::Real, id(), &Mint::from("mint-b"), ts(60)),
            "nothing was remembered from before the switch"
        );
    }

    #[test]
    fn a_token_with_no_identity_never_blocks_and_never_records() {
        let mut g = guard();
        g.record(TradeMode::Real, None, &Mint::from("mint-a"), ts(0));
        assert!(g.is_empty(TradeMode::Real));
        // Two nameless tokens are not the same token.
        assert!(!g.blocks(TradeMode::Real, None, &Mint::from("mint-b"), ts(60)));
    }

    #[test]
    fn prune_drops_expired_identities() {
        let mut g = guard();
        g.record(TradeMode::Real, id(), &Mint::from("mint-a"), ts(0));
        assert_eq!(g.len(TradeMode::Real), 1);

        // Inside the window the sweep keeps it...
        g.prune(ts(PRUNE_INTERVAL_SECS + 1));
        assert_eq!(g.len(TradeMode::Real), 1);
        // ...past it the identity is gone entirely, not just skipped on read.
        g.prune(ts(WINDOW_SECS + PRUNE_INTERVAL_SECS + 2));
        assert!(g.is_empty(TradeMode::Real));
    }

    #[test]
    fn re_recording_the_same_mint_keeps_one_entry_and_refreshes_it() {
        let mut g = guard();
        let a = Mint::from("mint-a");
        let b = Mint::from("mint-b");
        g.record(TradeMode::Real, id(), &a, ts(0));
        g.record(TradeMode::Real, id(), &a, ts(1000));

        // The block on a copycat now runs from the newer attempt.
        let past_first_window = WINDOW_SECS + 500;
        assert!(g.blocks(TradeMode::Real, id(), &b, ts(past_first_window)));
    }

    #[test]
    fn mints_per_identity_are_bounded_keeping_the_newest() {
        let mut g = guard();
        let total = MAX_MINTS_PER_IDENTITY + 5;
        for i in 0..total {
            g.record(TradeMode::Real, id(), &Mint::from(format!("mint-{i}").as_str()), ts(i as i64));
        }
        let entries = g.memory(TradeMode::Real).get(&id().unwrap()).unwrap();
        assert_eq!(entries.len(), MAX_MINTS_PER_IDENTITY);
        // The newest attempts survive the cap; the oldest are the ones dropped.
        let kept: Vec<&str> = entries.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(kept.first(), Some(&format!("mint-{}", total - MAX_MINTS_PER_IDENTITY).as_str()));
        assert_eq!(kept.last(), Some(&format!("mint-{}", total - 1).as_str()));
    }

    #[test]
    fn a_zero_window_falls_back_to_the_default_rather_than_blocking_nothing() {
        let mut g = DupeGuard::new();
        g.set_policy(true, 0);
        assert_eq!(g.window(), Duration::hours(DEFAULT_WINDOW_HOURS as i64));
    }
}
