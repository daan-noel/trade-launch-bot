//! Engine state — everything the fold carries between events. All maps are keyed
//! by sorted keys (`Mint`, `RuleId`, `PositionId`) so iteration order — and hence
//! the emitted effect order — is reproducible (plan §6 determinism rule).
//!
//! The state is deliberately *only* what decisions need: compiled rules + loaded
//! fingerprints, per-token metric tracks + arm states, per-rule cap counters, and
//! the two monotonic id generators (intents, positions). No clock, no I/O.

use std::collections::BTreeMap;

use crate::arm::{ArmState, CompiledRule};
use crate::event::{IntentId, LoadedRule, Mint, PositionId, RuleId};
use crate::fingerprint::{Fingerprint, FingerprintId};
use crate::grouping::TokenFingerprint;
use crate::metrics::track::TokenTrack;
use crate::metrics::Ts;

/// Per-rule live counters, backing the concurrency + lifetime caps. `open` counts
/// in-flight + held positions (for `max_concurrent`); `total` counts committed
/// entries over the rule's life (for `max_total`). A give-up on an entry that
/// never filled rolls both back; a normal close decrements only `open`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuleCounters {
    pub open: u32,
    pub total: u32,
}

/// A tracked position's owner, for [`crate::event::Event::ManualClose`] targeting.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionRef {
    pub mint: Mint,
    pub rule: RuleId,
}

/// All engine state for one token: its metric track + the per-rule arm states, plus
/// the two inputs the dead-token verdict folds incrementally.
#[derive(Debug, Clone)]
pub struct TokenState {
    pub created_at: Ts,
    /// Observed creation axes (first-slot fields filled in at `FirstSlotSettled`).
    pub tf: TokenFingerprint,
    pub track: TokenTrack,
    /// Newest *meaningful*-trade time (drives the deadness quiet clock). `None`
    /// until a meaningful trade prints — callers fall back to `created_at`.
    pub last_meaningful_at: Option<Ts>,
    /// Whether the creation slot has settled (idempotency guard for a late event).
    pub first_slot_settled: bool,
    /// Per-rule arming state, sorted by rule id for deterministic iteration.
    pub arms: BTreeMap<RuleId, ArmState>,
}

impl TokenState {
    /// Whether any arm is still non-terminal — else the token can be pruned.
    pub fn is_active(&self) -> bool {
        self.arms.values().any(ArmState::is_active)
    }
}

/// The engine's whole world. Construct with [`EngineState::new`], feed it events
/// through [`crate::reduce::reduce`].
#[derive(Debug, Clone, Default)]
pub struct EngineState {
    /// Compiled active rules, by id.
    pub rules: BTreeMap<RuleId, CompiledRule>,
    /// Loaded fingerprints (input order preserved for multi-match).
    pub fps: Vec<Fingerprint>,
    /// Union of every rule's distinct `window_size_sec` — ensured on each new track.
    pub all_windows: Vec<f64>,
    /// Per-rule cap counters (persist across rule reloads).
    pub counters: BTreeMap<RuleId, RuleCounters>,
    /// Tracked tokens, by mint.
    pub tokens: BTreeMap<Mint, TokenState>,
    /// Open positions' owners, for manual-close targeting.
    pub positions: BTreeMap<PositionId, PositionRef>,
    /// Monotonic intent sequence (determinism: never random).
    intent_seq: u64,
    /// Monotonic position id sequence.
    position_seq: u64,
}

impl EngineState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint the next intent for `(rule, mint)` — a fresh id on every call, so a
    /// retry after a failure never collides with the attempt it replaces.
    pub fn next_intent(&mut self, rule: RuleId, mint: Mint) -> IntentId {
        self.intent_seq += 1;
        IntentId { rule, mint, seq: self.intent_seq }
    }

    /// Mint the next position id.
    pub fn next_position(&mut self) -> PositionId {
        self.position_seq += 1;
        PositionId(self.position_seq)
    }

    /// Whether a fingerprint (by id) has a first-slot axis, i.e. its full identity
    /// only resolves after `FirstSlotSettled`. Unknown ids report `false`.
    pub fn fp_has_first_slot(&self, id: FingerprintId) -> bool {
        self.fps.iter().find(|f| f.id == id).is_some_and(Fingerprint::has_first_slot_criteria)
    }

    /// Rebuild the compiled rule set + fingerprints from a reload. Recomputes the
    /// distinct-window union and ensures any newly-referenced window exists on every
    /// already-tracked token (going forward — past history is not re-folded).
    pub fn reload(&mut self, rules: &[LoadedRule], fps: &[Fingerprint]) {
        self.rules = rules.iter().map(|r| (r.id, CompiledRule::compile(r))).collect();
        self.fps = fps.to_vec();

        self.all_windows.clear();
        for r in self.rules.values() {
            for &w in &r.windows {
                if !self.all_windows.contains(&w) {
                    self.all_windows.push(w);
                }
            }
        }
        for token in self.tokens.values_mut() {
            for &w in &self.all_windows {
                token.track.ensure_window(w);
            }
        }
    }

    /// A fresh track for a token created at `at`, pre-registering every rule window.
    pub fn new_track(&self, at: Ts) -> TokenTrack {
        let mut track = TokenTrack::new(at);
        for &w in &self.all_windows {
            track.ensure_window(w);
        }
        track
    }
}
