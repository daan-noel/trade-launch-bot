//! Engine state — everything the fold carries between events. All maps are keyed
//! by sorted keys (`Mint`, `RuleId`, `PositionId`) so iteration order — and hence
//! the emitted effect order — is reproducible (plan §6 determinism rule).
//!
//! The state is deliberately *only* what decisions need: compiled rules + loaded
//! fingerprints, per-token metric tracks + arm states, per-rule cap counters, and
//! the two monotonic id generators (intents, positions). No clock, no I/O.

use std::collections::BTreeMap;

use crate::arm::{ArmState, CompiledRule};
use crate::event::{IntentId, LoadedRule, ManualExit, Mint, PositionId, RuleId, TradeMode};
use crate::fingerprint::{Fingerprint, FingerprintId};
use crate::grouping::TokenFingerprint;
use crate::metrics::flow_split::FlowPatterns;
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
    /// Per-rule completed-episode count, for the re-entry cap (plan Ph4). Only ever
    /// touched for rules with re-entry configured (a one-shot rule never inserts a
    /// key), so it stays empty for every legacy rule. Lives beside `arms` and dies
    /// with the token, so no separate lifetime to manage.
    pub episodes: BTreeMap<RuleId, u32>,
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
    /// One-off exit rules synthesized for **manual** positions with TP/SL, keyed
    /// by position (each manual episode has its own config). Deliberately outside
    /// `rules` so a `RulesReloaded` cannot wipe them; removed with the position.
    /// A manual position with NO entry here is tracked-only: the evaluate sweep
    /// finds no rule and makes no decision — no TP/SL, no Dead-exit.
    pub manual_rules: BTreeMap<PositionId, CompiledRule>,
    /// Loaded fingerprints (input order preserved for multi-match).
    pub fps: Vec<Fingerprint>,
    /// Union of every rule's distinct **flow** `window_size_sec` (`m_flow_window` +
    /// `m_flow_split_window`) — ensured on each new track.
    pub all_windows: Vec<f64>,
    /// Union of every rule's distinct **price** `window_size_sec`
    /// (`m_price_window`) — ensured on each new track alongside `all_windows`.
    pub all_price_windows: Vec<f64>,
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

    /// Drop a closed position from the owner map AND its one-off manual exit rule
    /// (1:1 with the position) — the one removal path, so the two can't leak apart.
    pub fn remove_position(&mut self, position: PositionId) {
        self.positions.remove(&position);
        self.manual_rules.remove(&position);
    }

    /// Synthesize + install the one-off exit rule for a manual position's TP/SL
    /// config. `None` / empty config removes any existing rule (tracked-only).
    pub fn set_manual_exit(&mut self, position: PositionId, rule: RuleId, exit: Option<ManualExit>) {
        match exit {
            Some(e) if e.is_some() => {
                self.manual_rules.insert(position, compile_manual_exit_rule(rule, &e));
            }
            _ => {
                self.manual_rules.remove(&position);
            }
        }
    }

    /// Whether a fingerprint (by id) has a first-slot axis, i.e. its full identity
    /// only resolves after `FirstSlotSettled`. Unknown ids report `false`.
    pub fn fp_has_first_slot(&self, id: FingerprintId) -> bool {
        self.fps.iter().find(|f| f.id == id).is_some_and(Fingerprint::has_first_slot_criteria)
    }

    /// Rebuild the compiled rule set + fingerprints from a reload. Recomputes the
    /// distinct-window union and ensures any newly-referenced window / flow state
    /// exists on every already-tracked token (going forward — past history is not
    /// re-folded).
    pub fn reload(&mut self, rules: &[LoadedRule], fps: &[Fingerprint]) {
        self.rules = rules.iter().map(|r| (r.id, CompiledRule::compile(r))).collect();
        self.fps = fps.to_vec();

        self.all_windows.clear();
        self.all_price_windows.clear();
        for r in self.rules.values() {
            for &w in &r.flow_windows {
                if !self.all_windows.contains(&w) {
                    self.all_windows.push(w);
                }
            }
            for &w in &r.price_windows {
                if !self.all_price_windows.contains(&w) {
                    self.all_price_windows.push(w);
                }
            }
        }
        for token in self.tokens.values_mut() {
            Self::ensure_track_windows_and_flow(
                &mut token.track,
                &self.all_windows,
                &self.all_price_windows,
                &self.fps,
            );
        }
    }

    /// A fresh track for a token created at `at`, pre-registering every rule
    /// window (flow + price) and every configured flow fingerprint.
    pub fn new_track(&self, at: Ts) -> TokenTrack {
        let mut track = TokenTrack::new(at);
        Self::ensure_track_windows_and_flow(
            &mut track,
            &self.all_windows,
            &self.all_price_windows,
            &self.fps,
        );
        track
    }

    /// Resolve the compiled rule an arm evaluates under: a real rule by id, else
    /// the position's one-off manual exit rule (manual episodes are never in
    /// `rules`). `None` ⇒ no decision is made for the arm (tracked-only manual).
    pub fn rule_for(&self, rule: RuleId, position: Option<PositionId>) -> Option<&CompiledRule> {
        self.rules
            .get(&rule)
            .or_else(|| position.and_then(|p| self.manual_rules.get(&p)))
    }

    fn ensure_track_windows_and_flow(
        track: &mut TokenTrack,
        windows: &[f64],
        price_windows: &[f64],
        fps: &[Fingerprint],
    ) {
        for &w in windows {
            track.ensure_window(w);
        }
        for &w in price_windows {
            track.ensure_price_window(w);
        }
        for fp in fps {
            if let Some(patterns) = FlowPatterns::from_metric_config(&fp.metric_config) {
                track.ensure_flow(fp.id, &patterns, windows);
            }
        }
    }
}

/// Compile a manual position's TP/SL config into a one-off exit rule via the ONE
/// bot desugar path ([`CompiledRule::compile`]'s pnl-req expansion) — so a manual
/// TP/SL can never drift from a rule TP/SL. No entry conditions, no caps, no
/// fingerprint (nil id; the arm exists, arming never re-evaluates).
fn compile_manual_exit_rule(rule: RuleId, exit: &ManualExit) -> CompiledRule {
    use crate::fingerprint::FingerprintId;
    use crate::rule_params::RuleParams;

    let params = RuleParams {
        take_profit: exit.tp_pct.filter(|v| v.is_finite() && *v > 0.0),
        stop_loss: exit.sl_pct.filter(|v| v.is_finite() && *v > 0.0),
        entry: None,
        exit: None,
        reentry: None,
    };
    let loaded = LoadedRule {
        id: rule,
        fingerprint_id: FingerprintId(uuid::Uuid::nil()),
        trade_mode: TradeMode::Real,
        buy_amount_lamports: 0,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params,
    };
    CompiledRule::compile(&loaded)
}
