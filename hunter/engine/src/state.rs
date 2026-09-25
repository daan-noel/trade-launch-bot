//! Engine state — everything the fold carries between events. All maps are keyed
//! by sorted keys (`Mint`, `RuleId`, `PositionId`) so iteration order — and hence
//! the emitted effect order — is reproducible (plan §6 determinism rule).
//!
//! The state is deliberately *only* what decisions need: compiled rules + loaded
//! fingerprints, per-token metric tracks + arm states, per-rule cap counters, and
//! the two monotonic id generators (intents, positions). No clock, no I/O.

use std::collections::{BTreeMap, BTreeSet};

use crate::arm::{ArmState, ClockHorizons, CompiledRule};
use crate::dupe_guard::DupeGuard;
use crate::event::{IntentId, LoadedRule, ManualExit, Mint, PositionId, RuleId, TradeMode};
use crate::fingerprint::{Fingerprint, FingerprintId};
use crate::identity::IdentityHash;
use crate::grouping::TokenFingerprint;
use crate::metrics::burst_slot::TemplatePatterns;
use crate::metrics::buffers::Buffers;
use crate::metrics::tags::config::{compile_tags, CompiledTag, TagPatterns};
use crate::metrics::tags::TagKey;
use crate::metrics::track::TokenTrack;
use crate::metrics::{Ts, WindowSpec};

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
    /// from `TokenCreated`, or for a boot-adopted token by
    /// [`crate::reduce::restore_adopted`].
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
    /// Per-rule slot that already had an `entry_event` candidate (`entry_lock:
    /// "slot"`). Absent key ⇒ this rule has not locked a slot on this token.
    pub entry_locks: BTreeMap<RuleId, u64>,
    /// Built from a stored position at boot, without the token's creation facts: its
    /// `created_at` is the entry fill, it has no creator, no creation slot and no
    /// identity. [`crate::reduce::restore_adopted`] supplies them from the token cache
    /// before the first cached trade folds, and clears this.
    pub facts_pending: bool,
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

/// One fingerprint tag some loaded rule reads, as a track registers it.
#[derive(Debug, Clone, PartialEq)]
struct TagNeed {
    patterns: TagPatterns,
    /// Read trade by trade: open a `TagState` with these windows.
    trade: bool,
    windows: Vec<WindowSpec>,
    /// Read at the template level (slot / wave).
    template: Option<TemplatePatterns>,
}

/// Everything a coin's track folds under the loaded rules: the union of every rule's
/// [`Buffers`], plus one entry per fingerprint tag a rule reads. A reload registers new
/// state on tracked coins going forward only, so a track holds the whole history of a
/// buffer only if it was registered at the coin's birth; [`adds_to`](Self::adds_to) says
/// when a reload broke that.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackRequirements {
    buffers: Buffers,
    tags: BTreeMap<(FingerprintId, TagKey), TagNeed>,
}

impl TrackRequirements {
    /// The union over `rules`, with each tag resolved against its fingerprint's
    /// compiled tags (a tag the fingerprint does not define is left out: its reads are
    /// `NaN`, which satisfies nothing).
    fn build<'a>(
        rules: impl Iterator<Item = &'a CompiledRule>,
        fp_tags: &BTreeMap<FingerprintId, Vec<CompiledTag>>,
    ) -> Self {
        let mut out = Self::default();
        for r in rules {
            out.buffers.union(&r.buffers);
            for read in &r.buffers.tags {
                let Some(tag) = fp_tags.get(&r.fingerprint_id).and_then(|t| t.iter().find(|t| t.key == read.key)) else {
                    continue;
                };
                let need = out.tags.entry((r.fingerprint_id, read.key)).or_insert_with(|| TagNeed {
                    patterns: tag.patterns.clone(),
                    trade: false,
                    windows: Vec::new(),
                    template: None,
                });
                need.trade |= read.trade;
                for w in &read.windows {
                    if !need.windows.contains(w) {
                        need.windows.push(*w);
                    }
                }
                if read.template {
                    need.template = tag.patterns.templates();
                }
            }
        }
        out.buffers.tags.clear(); // held per fingerprint in `tags`
        out
    }

    /// Register all of it on one track (idempotent; a re-registered tag adopts its
    /// edited definition).
    fn ensure(&self, track: &mut TokenTrack) {
        self.buffers.ensure_on(track);
        for (&(fp, key), need) in &self.tags {
            if need.trade {
                track.ensure_tag(fp, key, &need.patterns, &need.windows);
            }
            if let Some(t) = &need.template {
                track.ensure_template_tag(fp, key, t);
            }
        }
    }

    /// Whether `self` asks a track for anything `before` did not: a window, an anchor
    /// or a larger anchor cap, a map, the slot state, or a tag (new, redefined, or with
    /// a new window). A track built under `before` has not folded that from birth.
    pub fn adds_to(&self, before: &TrackRequirements) -> bool {
        let (b, was) = (&self.buffers, &before.buffers);
        let new_window = [
            (&b.flow_windows, &was.flow_windows),
            (&b.price_windows, &was.price_windows),
            (&b.crowd_windows, &was.crowd_windows),
            (&b.build_windows, &was.build_windows),
        ]
        .iter()
        .any(|(now, was)| now.iter().any(|w| !was.contains(w)));
        let new_anchor = b
            .crowd_anchors
            .iter()
            .any(|(a, cap)| !was.crowd_anchors.iter().any(|(x, c)| a == x && c >= cap));
        let new_tag = self.tags.iter().any(|(k, need)| match before.tags.get(k) {
            None => true,
            Some(old) => {
                old.patterns != need.patterns
                    || (need.trade && !old.trade)
                    || need.windows.iter().any(|w| !old.windows.contains(w))
                    || (need.template.is_some() && old.template != need.template)
            }
        });
        new_window
            || new_anchor
            || new_tag
            || (b.print_wallet && !was.print_wallet)
            || (b.holder_book && !was.holder_book)
            || (b.slot_state && !was.slot_state)
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
    ///
    /// **Only those a loaded rule names.** The two readers are [`crate::fingerprint::match_all`]
    /// (whose hits are then tested against a compiled rule's `fingerprint_id`) and
    /// the per-track registration below, so a fingerprint no rule points at can only
    /// produce a hit nobody reads — while still costing a match per creation and a
    /// classifier deque per token. The live edge hands over every `fingerprints` row;
    /// [`reload`](Self::reload) is where that narrows to the working set.
    pub fps: Vec<Fingerprint>,
    /// Each loaded fingerprint's compiled tags, keyed by id — compiled once per reload,
    /// never per coin.
    pub(crate) fp_tags: BTreeMap<FingerprintId, Vec<CompiledTag>>,
    /// What every coin's track registers under the loaded rules.
    requirements: TrackRequirements,
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
    /// The day's launch-build stats, by build hash — what the two
    /// `build_prev_day_*` fingerprint axes are stamped from at `TokenCreated`.
    /// Replaced whole by [`Event::LaunchBuildStatsReloaded`]; empty until a host
    /// loads one, in which case every door axis fails closed (never arms).
    pub launch_build_stats: crate::hash::HashedMap<crate::event::LaunchBuildStat>,
    /// The day's public-app recipes, by build-recipe hash: the rows of the daily
    /// build-breadth table that pass [`is_public_app`](crate::metrics::holder_book::is_public_app).
    /// Replaced whole by
    /// [`Event::BuildBreadthReloaded`](crate::event::Event::BuildBreadthReloaded);
    /// `None` until a host loads one, in which case every buy is stamped unknown and
    /// `m_holder_book.public_app_share` reads `NaN` (fails closed).
    pub public_recipes: Option<crate::hash::HashedSet>,
    /// Launches seen per creator wallet hash — the tally behind
    /// the `prior_launches` fingerprint axis. Incremented on every `TokenCreated`, read
    /// (strictly before the increment) to seed the new token's metric.
    ///
    /// A live process starts empty, which would read every creator as a first-time
    /// launcher; [`prime_creator_launches`](Self::prime_creator_launches) is how a
    /// host loads real history in first. It is deliberately NOT pruned: a creator's
    /// count is the whole signal, and dropping a cold entry would resurrect exactly
    /// the "everyone is new" bias priming exists to remove.
    pub(crate) creator_launches: std::collections::HashMap<u64, u32>,
    /// The `name_reuse_count` tally, kept only for builds in
    /// [`identity_builds`](Self::identity_builds) - one build's launches, not the tape's.
    pub(crate) identity_launches: crate::fingerprint::identity_launches::IdentityLaunches,
    /// Creation builds (ix hash of the exact creation labels) some loaded fingerprint
    /// reads `name_reuse_count` on. Rebuilt on every reload.
    pub(crate) identity_builds: crate::hash::HashedSet,
    /// Builds whose history a host has already primed (live primes each once).
    identity_primed: crate::hash::HashedSet,
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

    /// Load creations into the `name_reuse_count` tally:
    /// `(creation build ix hash, identity, created_at, mint hash)`. A host primes every
    /// creation of the tracked builds over the span it replays plus the window before
    /// it; a mint already present is skipped.
    pub fn prime_identity_launches(
        &mut self,
        seen: impl IntoIterator<Item = (u64, crate::identity::IdentityHash, Ts, u64)>,
    ) {
        for (build, identity, at, mint) in seen {
            self.identity_launches.record(build, identity, at, mint);
        }
    }

    /// Creation builds some loaded fingerprint reads `name_reuse_count` on - the
    /// ones a host must prime.
    pub fn identity_builds(&self) -> &crate::hash::HashedSet {
        &self.identity_builds
    }

    /// Whether a host has primed this build's history yet; marks it primed. A live
    /// process primes each tracked build once, the first reload that names it.
    pub fn claim_identity_priming(&mut self, build: u64) -> bool {
        self.identity_builds.contains(&build) && self.identity_primed.insert(build)
    }

    /// How many earlier creations of `build` carried `identity` in the trailing window,
    /// and record this one. `None` when no loaded fingerprint reads the axis on the build.
    pub(crate) fn take_name_reuse_count(
        &mut self,
        build: u64,
        identity: crate::identity::IdentityHash,
        at: Ts,
        mint: u64,
    ) -> Option<u32> {
        if !self.identity_builds.contains(&build) {
            return None;
        }
        let prior = self.identity_launches.prior(build, identity, at, mint);
        self.identity_launches.record(build, identity, at, mint);
        Some(prior)
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

    /// Rebuild the compiled rule set + fingerprints from a reload, and register what the
    /// rules read on every tracked coin (going forward — past history is not re-folded).
    ///
    /// **Narrows `fps` to the fingerprints the rules name**, and compiles each one's
    /// tags here rather than per coin — see [`fps`](Self::fps) and
    /// [`fp_tags`](Self::fp_tags). Decision-neutral: a dropped fingerprint has no rule to
    /// arm, so its match answer was never read.
    pub fn reload(&mut self, rules: &[LoadedRule], fps: &[Fingerprint]) {
        self.rules = rules.iter().map(|r| (r.id, CompiledRule::compile(r))).collect();
        let named: BTreeSet<FingerprintId> =
            self.rules.values().map(|c| c.fingerprint_id).collect();
        self.fps = fps.iter().filter(|f| named.contains(&f.id)).cloned().collect();
        self.fp_tags = self
            .fps
            .iter()
            .map(|f| (f.id, compile_tags(&f.tags)))
            .filter(|(_, t)| !t.is_empty())
            .collect();
        self.identity_builds = self
            .fps
            .iter()
            .filter(|f| f.criteria.get(crate::fingerprint::AxisId::NameReuseCount).is_some())
            .filter_map(|f| match f.criteria.get(crate::fingerprint::AxisId::IxLabels) {
                Some(crate::fingerprint::AxisPredicate::Sequence { labels }) => {
                    crate::metrics::trade_keys::ix_hash_opt(labels)
                }
                _ => None,
            })
            .collect();

        let mut horizons = ClockHorizons::default();
        let mut any_priority = false;
        for r in self.rules.values() {
            horizons = horizons.widen(r.clock_horizons);
            any_priority |= r.priority != 0;
        }
        self.requirements = TrackRequirements::build(self.rules.values(), &self.fp_tags);
        self.tick_horizons = horizons;
        self.any_priority = any_priority;
        // A different rule set means different horizons, different priorities and a
        // different arming answer — nothing settled under the old set may stay settled.
        self.bump_cross_epoch();
        for token in self.tokens.values_mut() {
            self.requirements.ensure(&mut token.track);
        }
    }

    /// What a coin's track folds under the loaded rules.
    pub fn track_requirements(&self) -> TrackRequirements {
        self.requirements.clone()
    }

    /// Stamp a buy with whether its recipe went through a public app the previous day
    /// ([`TradeLite::build_day_public`]) when a loaded rule reads `m_holder_book`; any
    /// other print passes unchanged. One set lookup per buy, and none at all for a rule
    /// set without the group.
    ///
    /// [`TradeLite::build_day_public`]: crate::metrics::TradeLite::build_day_public
    pub fn stamp_build_breadth(&self, t: crate::metrics::TradeLite) -> crate::metrics::TradeLite {
        if self.requirements.buffers.holder_book {
            crate::metrics::holder_book::stamp_public(t, self.public_recipes.as_ref())
        } else {
            t
        }
    }

    /// A fresh track for a coin created at `at`, with everything the loaded rules read
    /// registered from birth.
    pub fn new_track(&self, at: Ts) -> TokenTrack {
        let mut track = TokenTrack::new(at);
        self.requirements.ensure(&mut track);
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
        ..RuleParams::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TradeMode;
    use crate::fingerprint::{AxisId, AxisPredicate, Criteria};
    use crate::metrics::{Metric, MetricRef, Side, TagRef, TradeLite};
    use crate::rule_params::RuleParams;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    fn ts() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn fp_id(n: u128) -> FingerprintId {
        FingerprintId(Uuid::from_u128(n))
    }

    /// A fingerprint with one `volume` tag.
    fn fp(n: u128) -> Fingerprint {
        Fingerprint {
            id: fp_id(n),
            wildcard: false,
            criteria: Criteria::new().with(AxisId::CuLimit, AxisPredicate::exact(200_000 + n)),
            tags: json!({ "volume": { "match": { "ix_shape": [["Pump.Fun: Buy"]] } } }),
        }
    }

    fn rule_on(fp: FingerprintId, params: serde_json::Value) -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: fp,
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 1,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).expect("params"),
            entry_enabled: true,
        }
    }

    fn reads_volume() -> serde_json::Value {
        json!({ "enter": { "filters": [
            { "metric": "m_flow.buy_sol", "tag": "!volume", "is": [{ "operator": ">=", "value": 0.0 }] }
        ] } })
    }

    fn rest_buy() -> MetricRef {
        MetricRef::life(Metric::BuySol).with_tag(TagRef::parse("!volume").unwrap())
    }

    /// Only the fingerprints a rule names are kept and compiled, and a coin opens
    /// state only for the tags a rule reads.
    #[test]
    fn reload_keeps_only_what_the_rules_read() {
        let (named, unnamed, bare) = (fp(1), fp(2), Fingerprint::empty(fp_id(3)));
        let mut state = EngineState::new();
        state.reload(&[rule_on(named.id, reads_volume())], &[named.clone(), unnamed.clone(), bare]);
        assert_eq!(state.fps.iter().map(|f| f.id).collect::<Vec<_>>(), vec![named.id]);
        assert_eq!(state.fp_tags.keys().copied().collect::<Vec<_>>(), vec![named.id]);

        let mut track = state.new_track(ts());
        track.on_trade(TradeLite { side: Side::Buy, sol: 1.0, price: 1.0, at: ts(), ..Default::default() });
        assert_eq!(track.value(rest_buy(), Some(named.id), ts()), 1.0);
        assert!(track.value(rest_buy(), Some(unnamed.id), ts()).is_nan(), "an unnamed fingerprint opens no state");
    }

    /// A rule that reads no tag opens no tag state, even on a fingerprint that
    /// defines one: an unread tag would be a classifier folded on every trade for
    /// nothing.
    #[test]
    fn an_unread_tag_opens_no_state() {
        let named = fp(1);
        let mut state = EngineState::new();
        let plain = json!({ "enter": { "filters": [{ "metric": "m_state.age_sec", "is": [{ "operator": ">=", "value": 1.0 }] }] } });
        state.reload(&[rule_on(named.id, plain)], std::slice::from_ref(&named));
        let track = state.new_track(ts());
        assert!(!track.has_tag(named.id, TagKey::of("volume")));
    }

    /// A tag a rule reads but its fingerprint does not define reads NaN, never 0.
    #[test]
    fn a_missing_tag_reads_nan() {
        let mut plain = fp(1);
        plain.tags = json!({});
        let mut state = EngineState::new();
        state.reload(&[rule_on(plain.id, reads_volume())], std::slice::from_ref(&plain));
        let mut track = state.new_track(ts());
        track.on_trade(TradeLite { side: Side::Buy, sol: 1.0, price: 1.0, at: ts(), ..Default::default() });
        assert!(track.value(rest_buy(), Some(plain.id), ts()).is_nan());
    }

    /// Narrowing is decision-neutral: the dropped rows had no rule to arm.
    #[test]
    fn narrowing_is_decision_neutral() {
        let named = fp(1);
        let mut all = vec![named.clone()];
        all.extend((10..40).map(fp));
        let mut wide = EngineState::new();
        wide.reload(&[rule_on(named.id, reads_volume())], &all);
        let mut narrow = EngineState::new();
        narrow.reload(&[rule_on(named.id, reads_volume())], std::slice::from_ref(&named));
        let tf = TokenFingerprint { cu_limit: Some(200_001), ..Default::default() };
        let hits = |st: &EngineState| {
            crate::fingerprint::match_all(&st.fps, &tf, crate::fingerprint::MatchPhase::Full)
                .into_iter()
                .filter(|id| st.rules.values().any(|c| c.fingerprint_id == *id))
                .collect::<Vec<_>>()
        };
        assert_eq!(hits(&wide), hits(&narrow));
        assert_eq!(hits(&narrow), vec![named.id]);
    }

    /// A reload that starts reading a tag window adds to what tracked coins folded.
    #[test]
    fn a_new_tag_window_adds_to_the_requirements() {
        let named = fp(1);
        let mut state = EngineState::new();
        state.reload(&[rule_on(named.id, reads_volume())], std::slice::from_ref(&named));
        let before = state.track_requirements();
        let windowed = json!({ "enter": { "filters": [
            { "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{ "operator": ">=", "value": 0.0 }] }
        ] } });
        state.reload(&[rule_on(named.id, windowed)], std::slice::from_ref(&named));
        assert!(state.track_requirements().adds_to(&before));
        assert!(!before.adds_to(&before));
    }
}
