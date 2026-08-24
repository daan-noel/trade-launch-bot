//! Engine state — everything the fold carries between events. All maps are keyed
//! by sorted keys (`Mint`, `RuleId`, `PositionId`) so iteration order — and hence
//! the emitted effect order — is reproducible (plan §6 determinism rule).
//!
//! The state is deliberately *only* what decisions need: compiled rules + loaded
//! fingerprints, per-token metric tracks + arm states, per-rule cap counters, and
//! the two monotonic id generators (intents, positions). No clock, no I/O.

use std::collections::BTreeMap;

use crate::arm::{ArmState, ClockHorizons, CompiledRule};
use crate::dupe_guard::DupeGuard;
use crate::event::{IntentId, LoadedRule, ManualExit, Mint, PositionId, RuleId, TradeMode};
use crate::fingerprint::{Fingerprint, FingerprintId};
use crate::identity::IdentityHash;
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
    /// The `(name, symbol)` key the duplicate-identity guard matches on. `None` =
    /// unknown or blank ⇒ this token never blocks and is never recorded. Set once
    /// from `TokenCreated`; a boot-adopted position must be given it explicitly
    /// (see `live`'s adopt path) or the guard silently forgets across a restart.
    pub identity: Option<IdentityHash>,
    pub track: TokenTrack,
    /// Newest *meaningful*-trade time (drives the deadness quiet clock). `None`
    /// until a meaningful trade prints — callers fall back to `created_at`.
    pub last_meaningful_at: Option<Ts>,
    /// Newest folded trade time (**any** size, unlike `last_meaningful_at`) — the
    /// origin every trade-anchored clock horizon measures from. `None` until the
    /// first print, where `created_at` stands in.
    pub last_trade_at: Option<Ts>,
    /// Cached "this token is done changing on its own" verdict — see
    /// [`Settled`]. `None` ⇒ evaluate on every tick.
    pub settled: Option<Settled>,
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

/// A token's "nothing of mine can change on its own any more" verdict, stamped by
/// the evaluate sweep and consumed by [`crate::reduce`]'s `Tick` branch.
///
/// **Why this exists.** A token leaves `tokens` only when every arm goes terminal,
/// and the only thing that disarms an idle *armed* token is the dead verdict —
/// which needs real reserves under `DEAD_MAX_LIQUIDITY_SOL`. A token that pumped
/// past that floor (or whose rows carry no reserve at all, so liquidity reads
/// `NaN`) is therefore **never** pruned, and without this skip it is swept
/// arm-by-arm five times a second for the rest of the run. Live that is a slow
/// leak; in a multi-day simulate it is the dominant cost, and it grows with corpus
/// width rather than with anything the rule actually does.
///
/// Skipping is only sound if it is *decision-neutral*, and the verdict is only
/// stamped when both of these hold:
///
/// * the sweep that stamped it ran at an instant **at or past** `until` — the last
///   instant any of this token's own readings can move: its rules'
///   [`ClockHorizons`] anchored on creation / the last trade / each entry fill, the
///   one-shot dead flip at `last_meaningful + DEAD_QUIET_SECS`, and any pending
///   re-entry cooldown. This is deliberately "*has already been* evaluated past the
///   horizon", not "`now` is past the horizon": tick cadence is not the engine's to
///   assume (the live loop ticks every `TICK_MS`, a replay driver may tick at
///   arbitrary instants), and comparing against `now` silently swallows any
///   crossing that falls inside a tick gap. Evaluating *at* the horizon is what
///   makes every later instant provably identical.
/// * `epoch` still matches [`EngineState::cross_epoch`], which is bumped whenever
///   *another* token's event changes something this one's decision reads: a cap
///   counter (a freed slot lets a waiting arm enter), a copycat-guard record (a new
///   identity can disarm an armed token), or a rules reload. A stale epoch means
///   "re-evaluate once, then re-settle".
///
/// The third cross-arm input, `exclusive`, resolves through events on *this* token
/// (a fill / close on a sibling arm), so those branches clear `settled` outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settled {
    /// The horizon the stamping sweep had already reached. Diagnostic — the skip
    /// predicate does not re-compare it (see above).
    pub until: Ts,
    pub epoch: u64,
}

impl TokenState {
    /// Whether any arm is still non-terminal — else the token can be pruned.
    pub fn is_active(&self) -> bool {
        self.arms.values().any(ArmState::is_active)
    }

    /// Whether a `Tick` is provably a no-op for this token — see [`Settled`].
    /// Deliberately independent of `now`: the verdict is only stamped by a sweep
    /// that already ran at or past the horizon, so *every* later instant repeats it.
    pub fn tick_is_noop(&self, epoch: u64) -> bool {
        self.settled.is_some_and(|s| s.epoch == epoch)
    }

    /// Forget the settled verdict — any change to this token's arms that did not go
    /// through the evaluate sweep must call this, or the next tick may skip a
    /// decision the change enabled.
    pub fn unsettle(&mut self) {
        self.settled = None;
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
    /// Union of every loaded rule's [`ClockHorizons`] — how long *any* rule's
    /// readings can still move without a trade. Drives [`Settled`].
    pub tick_horizons: ClockHorizons,
    /// Whether any loaded rule sets a non-zero `priority`.
    ///
    /// The evaluate sweep visits arms by `(Reverse(priority), rule_id)`, but
    /// `arms` is a `BTreeMap` and therefore *already* in rule-id order — so when
    /// every priority is equal the sort is a no-op it pays for on every event of
    /// every token. `priority` only ever changes behaviour between two contesting
    /// `exclusive` rules anyway; this flag is what lets the sweep skip the sort
    /// without changing the visit order in the case where it matters.
    pub any_priority: bool,
    /// Monotonic counter over **cross-token** state a settled token's decision
    /// depends on — cap counters, the copycat guard's memory, the rule set. See
    /// [`Settled`]; bump through [`bump_cross_epoch`](Self::bump_cross_epoch).
    pub cross_epoch: u64,
    /// Memo: the previous `Tick` sweep found **every** tracked token settled, and
    /// there were this many of them.
    ///
    /// Per-token skipping still costs one iteration + one compare per tracked token
    /// per tick, and the token set only grows (an un-prunable token never leaves).
    /// Over a multi-day replay — millions of ticks — that walk becomes the cost by
    /// itself. This collapses the whole tick to an O(1) check for the case that
    /// dominates a long quiet stretch: nobody has anything left to do.
    ///
    /// Conservative by construction: a stale `None` only costs one wasted walk.
    /// It is cleared by every non-`Tick` event (`reduce` does that up front), by any
    /// change in the token count, and by [`touch_token`](Self::touch_token) for the
    /// boot paths that mutate a tracked token outside the fold.
    pub(crate) all_settled_at: Option<usize>,
    /// Force every `Tick` to sweep every token, ignoring [`Settled`].
    ///
    /// The skip is an optimization that must be **decision-neutral**, and this is
    /// how that claim is tested: `settled_tick_skip_is_decision_neutral` replays one
    /// event stream through a dense engine and a skipping one and asserts the effect
    /// streams are equal. It doubles as the kill switch if a future metric ever
    /// gains a clock the horizons do not model — set it and the engine is back to
    /// its pre-optimization behaviour, at pre-optimization cost.
    pub dense_ticks: bool,
    /// Per-rule cap counters (persist across rule reloads).
    pub counters: BTreeMap<RuleId, RuleCounters>,
    /// Tracked tokens, by mint.
    pub tokens: BTreeMap<Mint, TokenState>,
    /// Open positions' owners, for manual-close targeting.
    pub positions: BTreeMap<PositionId, PositionRef>,
    /// Launches seen per creator wallet hash — the tally behind
    /// `m_snapshot.prior_launches`. Incremented on every `TokenCreated`, read
    /// (strictly before the increment) to seed the new token's metric.
    ///
    /// A live process starts empty, which would read every creator as a first-time
    /// launcher; [`prime_creator_launches`](Self::prime_creator_launches) is how a
    /// host loads real history in first. It is deliberately NOT pruned: a creator's
    /// count is the whole signal, and dropping a cold entry would resurrect exactly
    /// the "everyone is new" bias priming exists to remove.
    pub(crate) creator_launches: std::collections::HashMap<u64, u32>,
    /// Rolling memory of recently-traded `(name, symbol)` identities — the
    /// copycat guard. Disabled (and empty) unless the operator turns it on via
    /// [`set_dupe_guard_policy`](Self::set_dupe_guard_policy).
    pub dupe_guard: DupeGuard,
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

    /// Load known launch history into the `prior_launches` tally, before any event
    /// is folded. `(creator_wallet_hash, launches_so_far)` pairs; a repeated hash
    /// keeps the LARGER count, so priming twice cannot lose depth.
    ///
    /// Without this a fresh process reads every creator as a first-time launcher —
    /// which is not a small error but an inverted one, since `prior_launches == 0`
    /// is the value a rule selects ON. Offline this is free (the corpus is the
    /// history); live it wants a query over the `tokens` table at boot.
    pub fn prime_creator_launches(&mut self, seen: impl IntoIterator<Item = (u64, u32)>) {
        for (hash, n) in seen {
            let slot = self.creator_launches.entry(hash).or_insert(0);
            *slot = (*slot).max(n);
        }
    }

    /// Take this creator's launch count and record the launch — the strictly-prior
    /// count, so the creator's own first token reads `0`.
    pub(crate) fn take_prior_launches(&mut self, creator: u64) -> u32 {
        let slot = self.creator_launches.entry(creator).or_insert(0);
        let prior = *slot;
        *slot = slot.saturating_add(1);
        prior
    }

    /// Mint the next position id.
    pub fn next_position(&mut self) -> PositionId {
        self.position_seq += 1;
        PositionId(self.position_seq)
    }

    /// Invalidate every token's [`Settled`] verdict at once, because something a
    /// settled token's decision reads but does not own has changed. Cheap (one
    /// counter) precisely so the callers below can be liberal about calling it —
    /// an unnecessary bump costs one extra sweep, a missing one costs a decision.
    pub fn bump_cross_epoch(&mut self) {
        self.cross_epoch = self.cross_epoch.wrapping_add(1);
        self.all_settled_at = None;
    }

    /// Whether the next `Tick` is a whole-map no-op (see
    /// [`all_settled_at`](Self::all_settled_at)) — diagnostics and guard tests.
    pub fn all_tokens_settled(&self) -> bool {
        self.all_settled_at == Some(self.tokens.len())
    }

    /// Announce that a tracked token was mutated **outside** the fold, so the next
    /// tick re-decides it. Boot adoption (`live`'s orphan/manual reconcile, the
    /// re-entry episode seed) reaches into `tokens` directly; without this the
    /// engine could carry a settled verdict that predates the adoption.
    pub fn touch_token(&mut self, mint: &Mint) {
        if let Some(t) = self.tokens.get_mut(mint) {
            t.unsettle();
        }
        self.all_settled_at = None;
    }

    /// Mutate a rule's cap counters, invalidating settled tokens: a freed slot is
    /// exactly what an arm that stayed `Armed` because the cap refused it is
    /// waiting for. The ONE mutation path, so no call site can forget the bump.
    pub fn with_counters(&mut self, rule: RuleId, f: impl FnOnce(&mut RuleCounters)) {
        f(self.counters.entry(rule).or_default());
        self.bump_cross_epoch();
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

    /// Apply the operator's duplicate-identity policy.
    ///
    /// **Not an `Event`, deliberately.** It is an operator switch, not a market
    /// input: it carries no timestamp, must not appear in the event log's decision
    /// stream, and a replay sets it from its own run config rather than inheriting
    /// whatever live happened to have on. Live calls this whenever `app_settings`
    /// changes (the settings `watch` channel); the lab replay calls it once.
    pub fn set_dupe_guard_policy(&mut self, enabled: bool, window_hours: u64) {
        self.dupe_guard.set_policy(enabled, window_hours);
    }

    /// Remember an entry attempt's identity. Called for **every** entry the fold
    /// submits — bot or manual, filled or not — because a copycat that reverts our
    /// buy is exactly the trap worth not re-entering. A no-op while the guard is
    /// off (see [`DupeGuard::record`]).
    pub fn record_entry_identity(&mut self, mode: TradeMode, mint: &Mint, at: Ts) {
        let identity = self.tokens.get(mint).and_then(|t| t.identity);
        self.record_identity(mode, identity, mint, at);
    }

    /// The ONE write into the copycat guard's memory from the fold, so no call site
    /// can record an identity without invalidating settled tokens: a newly
    /// remembered identity can disarm an armed token that had already settled.
    ///
    /// (Expiry is deliberately NOT bumped. It only ever *un*blocks, and a copycat
    /// block is a terminal `Disarm` — nothing tracked is ever waiting for one to
    /// lapse.)
    pub fn record_identity(
        &mut self,
        mode: TradeMode,
        identity: Option<IdentityHash>,
        mint: &Mint,
        at: Ts,
    ) {
        self.dupe_guard.record(mode, identity, mint, at);
        self.bump_cross_epoch();
    }

    /// Seed one already-traded identity at boot (the PG rebuild). Same memory as
    /// [`record_entry_identity`](Self::record_entry_identity), but the token need
    /// not be tracked — a restart rebuilds from `strategy_positions`, not from
    /// whatever tokens happen to be live.
    pub fn seed_traded_identity(
        &mut self,
        mode: TradeMode,
        identity: Option<IdentityHash>,
        mint: &Mint,
        at: Ts,
    ) {
        self.dupe_guard.record(mode, identity, mint, at);
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

        let mut all_windows: Vec<f64> = Vec::new();
        let mut all_price_windows: Vec<f64> = Vec::new();
        let mut horizons = ClockHorizons::default();
        let mut any_priority = false;
        for r in self.rules.values() {
            horizons = horizons.widen(r.clock_horizons);
            any_priority |= r.priority != 0;
            for &w in &r.flow_windows {
                if !all_windows.contains(&w) {
                    all_windows.push(w);
                }
            }
            for &w in &r.price_windows {
                if !all_price_windows.contains(&w) {
                    all_price_windows.push(w);
                }
            }
        }
        self.all_windows = all_windows;
        self.all_price_windows = all_price_windows;
        self.tick_horizons = horizons;
        self.any_priority = any_priority;
        // A different rule set means different horizons, different priorities and a
        // different arming answer — nothing settled under the old set may stay settled.
        self.bump_cross_epoch();
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
        scale_out: None,
        reentry: None,
        exclusive: false,
        priority: 0,
        disabled: None,
        // Exit-only rule: there is no buy to size.
        buy_pct_of_vsol: None,
    };
    let loaded = LoadedRule {
        id: rule,
        fingerprint_id: FingerprintId(uuid::Uuid::nil()),
        trade_mode: TradeMode::Real,
        buy_amount_lamports: 0,
        max_concurrent_tokens: 1,
        max_total_tokens: 0,
        params,
        entry_enabled: true,
    };
    CompiledRule::compile(&loaded)
}
