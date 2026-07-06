//! Unified, **strategy-agnostic** runtime cache — the in-memory hot-path state a
//! `StrategyRunner` reads/mutates per event, keyed by `rule_id` / `strategy_id`.
//! Replaces the hand-cloned `Tpsl1RuntimeCache` / `Tpsl2RuntimeCache`: one cache
//! serves every strategy because the per-event decision is dispatched through the
//! [`registry`](super::registry), not branched here.
//!
//! Holds:
//!   • active rules + their **parsed [`StrategyParams`]** (parsed once at load, so
//!     the hot path never re-parses the JSONB),
//!   • the open-position index by mint + per-rule cap counters
//!     (`holding_count` / `total_count`),
//!   • per-rule realized-performance counters ([`RuleClosedStats`]),
//!   • the current paper-run pointer per rule,
//!   • the in-flight entry/exit RAII guards (the no-double-buy / no-double-sell
//!     interlocks).
//!
//! DB loads stay at the live edge (it wraps DB reads around
//! [`set_rules`](StrategyRuntimeCache::set_rules) / position writes). SSE position
//! deltas, by contrast, are emitted **here** from the position-transition funnel
//! ([`sync_position`](StrategyRuntimeCache::sync_position) /
//! [`remove_position`](StrategyRuntimeCache::remove_position)) via an *optional*
//! sender ([`set_sse_sender`](StrategyRuntimeCache::set_sse_sender)): every
//! transition flows through exactly these two methods by construction, so emitting
//! at the funnel guarantees no delta is ever missed (an edge that forgot to emit
//! would silently drop one). Unit tests and the `lab` bin never set a sender, so
//! emission is a no-op there — the "trivially unit-testable, in-memory only"
//! property is preserved. All the delta needs (legacy [`Position`] adapter, rule
//! snapshot, cap counters) is already core-local.
//!
//! Also holds the live hot-path exit machinery (ported here in Phase 3): the
//! per-position clock-driven **exit-state memo** (`exit_state_by_position`, strategy-
//! dispatched via [`CachedExitStateImpl`](super::exit_state)) and the **time-exit
//! secondary index** (`time_exit_holding`), so the per-second sweep never re-walks a
//! token's retained history. Both are kept in lockstep with the holding index.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use tokio::sync::{broadcast, Semaphore};
use uuid::Uuid;

use crate::models::ingest::{RuleNotifSnapshot, SseEvent};
use crate::models::position::Position;
use crate::models::{StrategyPosition, StrategyRule, StrategyRun};
use crate::state::token_cache::CachedTrade;
use crate::storage::repositories::strategy_repo::StrategyRepo;

use super::exit_state::{clock_entry_time, CachedExitStateImpl, LadderParamsImpl};
use super::registry::{StrategyImpl, StrategyParams};

/// Max concurrent paper fill-poll tasks (entry + exit) across all strategies — the
/// shared semaphore the live paper executor acquires before each fill-poll spawn, so
/// a fill burst can't spawn an unbounded number of feed-polling DB tasks at once.
const PAPER_POLL_CONCURRENCY: usize = 64;

/// Max concurrent **until-dead** scalp armers (real + paper) — entry watches with
/// no `p_entry_max_age_secs` ceiling. A healthy pumping token never satisfies
/// `is_dead`, so an until-dead watch on one could pin its `token_cache` entry
/// forever; this caps how many such watches run at once. When full,
/// [`begin_until_dead_armer`](StrategyRuntimeCache::begin_until_dead_armer) evicts
/// the oldest un-entered armer. Bounded (max-age) armers don't count against this.
const MAX_UNTIL_DEAD_ARMERS: usize = 32;

/// Max concurrent deferred first-slot entry gates waiting for the creation-slot
/// window to close. Mirrors [`MAX_UNTIL_DEAD_ARMERS`]'s bounded pending-set shape.
const MAX_FIRST_SLOT_PENDING: usize = 32;

/// Backstop timeout for deferred first-slot entry gates (seconds). Resolves with
/// whatever first-slot totals have accumulated so far.
pub const FIRST_SLOT_GATE_TIMEOUT_SECS: i64 = 5;

/// Pointer to a rule's current run — the run new positions are stamped with and the
/// run the result view surfaces. Both modes carry a run now (real *and* paper), so
/// this is mode-agnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRef {
    pub run_id: Uuid,
}

/// Per-rule realized-performance counters, accumulated live on each position close
/// and warmed from the DB on boot. All-time for real rules; current-run for paper
/// rules. Raw sums only — the API layer derives win rate / return % via
/// [`avg_pnl_pct`](Self::avg_pnl_pct), so the hot path never divides.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct RuleClosedStats {
    /// Clean `End` exits sold above entry.
    pub wins: i64,
    /// Every other closed position (breakeven, loss, failed exit).
    pub losses: i64,
    /// Sum of realized SOL PnL across all closed positions.
    pub sum_pnl_sol: f64,
    /// Sum of SOL **deployed** (entry cost) across the same closed positions — the
    /// capital base for the canonical capital-weighted [`avg_pnl_pct`](Self::avg_pnl_pct).
    /// Kept paired with `sum_pnl_sol` (accumulated in the same branch) so numerator
    /// and denominator always cover the identical trade set.
    pub sum_entry_sol: f64,
}

impl RuleClosedStats {
    /// Number of closed positions = the win/loss denominator.
    pub fn closed(&self) -> i64 {
        self.wins + self.losses
    }

    /// Canonical capital-weighted realized return % (`Σ pnl_sol / Σ entry_sol × 100`)
    /// — the single [`weighted_return_pct`](crate::strategies::kernel::weighted_return_pct)
    /// definition, so this figure's sign always matches `sum_pnl_sol`.
    pub fn avg_pnl_pct(&self) -> f64 {
        crate::strategies::kernel::weighted_return_pct(self.sum_pnl_sol, self.sum_entry_sol)
    }

    /// Fold one freshly-closed position into the counters. `sign = 1` adds it,
    /// `-1` backs it out (a closed position was removed).
    fn apply(&mut self, p: &StrategyPosition, sign: i64) {
        if p.is_win() {
            self.wins = (self.wins + sign).max(0);
        } else {
            self.losses = (self.losses + sign).max(0);
        }
        let s = sign as f64;
        if let Some(sol) = p.realized_pnl_sol() {
            self.sum_pnl_sol += s * sol;
            // `realized_pnl_sol()` is `Some` only when `entry_sol` is, so the capital
            // base stays paired with the PnL sum (same trade set, same sign path).
            self.sum_entry_sol += s * p.entry_sol.unwrap_or(0.0);
        }
    }
}

/// RAII claim on an in-flight **exit**. While held, `position_id` stays in the
/// `exiting` set (the no-double-sell guard); dropping it — on normal return, early
/// return, OR a panic that unwinds the holding task — frees the slot automatically.
pub struct ExitGuard {
    exiting: Arc<DashSet<Uuid>>,
    position_id: Uuid,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.exiting.remove(&self.position_id);
    }
}

/// RAII claim on an in-flight **entry** (a real snipe buy in progress) — the
/// buy-side twin of [`ExitGuard`]. While held, the buy-recovery reaper skips this
/// position; the slot frees on drop (incl. a panic), so after a crash the set is
/// empty and every reloaded `BuySubmitted` row is recoverable.
pub struct EntryGuard {
    entering: Arc<DashSet<Uuid>>,
    position_id: Uuid,
}

impl Drop for EntryGuard {
    fn drop(&mut self) {
        self.entering.remove(&self.position_id);
    }
}

/// RAII slot for an **until-dead** scalp armer — an entry watch with no
/// `p_entry_max_age_secs` ceiling, which would otherwise pin a never-dying token
/// (and its `token_cache` entry / `paper_poll_sem` permit) indefinitely. While
/// held, `position_id` occupies one of the [`MAX_UNTIL_DEAD_ARMERS`] slots; the
/// armer polls [`is_cancelled`](UntilDeadArmerGuard::is_cancelled) each tick and
/// bails if a newer armer evicted it. Dropping it — normal return, eviction, or a
/// panic — frees the slot. A max-age-bounded armer is self-limiting (its deadline)
/// and takes no slot. Strategy-agnostic in mechanism, used only by the tpsl2 scalp
/// entry watch today (tpsl1 buys immediately and never arms).
pub struct UntilDeadArmerGuard {
    registry: Arc<DashMap<Uuid, UntilDeadArmerSlot>>,
    position_id: Uuid,
    cancelled: Arc<AtomicBool>,
}

impl UntilDeadArmerGuard {
    /// True once a newer armer evicted this one because the cap was reached. The
    /// watch loop checks this each tick and returns (dropping its unentered
    /// position), so the freed slot is reclaimed promptly.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for UntilDeadArmerGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.position_id);
    }
}

/// Registry value behind an [`UntilDeadArmerGuard`]: a monotonic `seq` (the
/// eviction order — smallest = oldest) and the shared cancel flag.
#[derive(Clone)]
struct UntilDeadArmerSlot {
    seq: u64,
    cancelled: Arc<AtomicBool>,
}

/// A deferred first-slot entry gate queued at token creation.
#[derive(Clone, Debug)]
pub struct PendingFirstSlotEntry {
    pub queued_at: DateTime<Utc>,
    seq: u64,
}

/// In-memory strategy state for the hot path (rules + parsed params + open
/// positions + per-rule counters), shared across all strategies.
///
/// Counters are mode-aware: for real rules `total_count_by_rule` is all-time; for
/// paper rules it is scoped to the current run (reset on `clear_rule`). Paper and
/// real rule ids are disjoint, so the shared maps never collide.
#[derive(Clone)]
pub struct StrategyRuntimeCache {
    active_rules: Arc<RwLock<Arc<Vec<StrategyRule>>>>,
    rules_by_id: Arc<RwLock<HashMap<Uuid, StrategyRule>>>,
    /// Each active rule's params, parsed once at [`set_rules`] time so the hot
    /// path reads a typed [`StrategyParams`] with zero per-event JSON cost.
    params_by_id: Arc<RwLock<HashMap<Uuid, StrategyParams>>>,
    /// Each active rule's resolved exit ladder ([`LadderParamsImpl`]), built once at
    /// [`set_rules`] time so the per-trade exit gate + the clock sweep read it with a
    /// cheap clone (scalar fields) instead of rebuilding it from the rule per event.
    ladder_by_id: Arc<RwLock<HashMap<Uuid, LadderParamsImpl>>>,
    holding_by_mint: Arc<DashMap<String, Vec<Arc<StrategyPosition>>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    /// Not-yet-filled positions per rule (`Arming` | `BuySubmitted`) — the
    /// "pending entry" count surfaced next to `open_positions` in the rules table.
    pending_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
    closed_stats_by_rule: Arc<DashMap<Uuid, RuleClosedStats>>,
    current_run_by_rule: Arc<DashMap<Uuid, RunRef>>,
    /// Memoized clock-driven exit walk state per holding position (strategy-dispatched
    /// via [`CachedExitStateImpl`]). Seeded once and advanced as trades print, so the
    /// per-second time-exit sweep never re-walks a token's full history. An entry is
    /// dropped when its position leaves the holding index.
    exit_state_by_position: Arc<DashMap<Uuid, CachedExitStateImpl>>,
    /// Secondary index over the holdings: only the positions whose rule currently
    /// carries a time-based exit (TimeStop / Stall). The per-second sweep iterates
    /// *this* set (usually far smaller, often empty) instead of cloning every holding
    /// `Arc` each tick. Kept in lockstep with the holding index on every add/remove,
    /// and rebuilt wholesale on a rule reload (a rule's time-exit config can change).
    time_exit_holding: Arc<DashMap<Uuid, Arc<StrategyPosition>>>,
    /// Shared semaphore bounding concurrent paper fill-poll tasks (see
    /// [`PAPER_POLL_CONCURRENCY`]).
    paper_poll_sem: Arc<Semaphore>,
    exiting: Arc<DashSet<Uuid>>,
    entering: Arc<DashSet<Uuid>>,
    /// Active **until-dead** scalp armers (entry watches with no `max_age`
    /// ceiling), keyed by position id. Bounded to [`MAX_UNTIL_DEAD_ARMERS`]: when
    /// full, the oldest un-entered armer is evicted so a never-dying token can't pin
    /// an unbounded number of watches (and their `token_cache` entries). A
    /// max-age-bounded armer is self-limiting and never registers here. Used only by
    /// the tpsl2 scalp entry watch; tpsl1 buys immediately and never arms.
    until_dead_armers: Arc<DashMap<Uuid, UntilDeadArmerSlot>>,
    /// Monotonic counter assigning each until-dead armer its eviction-order `seq`.
    armer_seq: Arc<AtomicU64>,
    /// Deferred first-slot entry gates keyed by `(mint, rule_id)`. Bounded to
    /// [`MAX_FIRST_SLOT_PENDING`]: when full, the oldest pending entry is evicted.
    pending_first_slot: Arc<DashMap<(String, Uuid), PendingFirstSlotEntry>>,
    /// Monotonic counter assigning each pending first-slot entry its eviction `seq`.
    first_slot_seq: Arc<AtomicU64>,
    /// Optional cold-lane SSE sender for `tpsl_positions_changed` deltas, set once at
    /// boot by the live edge ([`set_sse_sender`](Self::set_sse_sender)). Unset in unit
    /// tests and the `lab` bin, where [`emit_position_delta`](Self::emit_position_delta)
    /// is a no-op — keeping the cache usable with no SSE plumbing.
    sse_tx: OnceLock<broadcast::Sender<SseEvent>>,
}

impl Default for StrategyRuntimeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Stamp harvested swing legs onto a position's `extra` JSONB, without clobbering
/// any other key already there. No-op if `legs` is empty (nothing harvested — not
/// a swing1 position, or its memo had no legs yet). Callers merge this in before
/// their own `update_position` write, right after `sync_position` returns the legs.
pub fn merge_swing_legs_into_extra(
    position: &mut StrategyPosition,
    legs: Vec<crate::strategies::swing_1::swing::SwingLeg>,
) {
    if legs.is_empty() {
        return;
    }
    let Some(obj) = position.extra.as_object_mut() else {
        position.extra = serde_json::json!({ "swing_legs": legs });
        return;
    };
    obj.insert("swing_legs".to_string(), serde_json::json!(legs));
}

impl StrategyRuntimeCache {
    pub fn new() -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            params_by_id: Arc::new(RwLock::new(HashMap::new())),
            ladder_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            pending_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
            closed_stats_by_rule: Arc::new(DashMap::new()),
            current_run_by_rule: Arc::new(DashMap::new()),
            exit_state_by_position: Arc::new(DashMap::new()),
            time_exit_holding: Arc::new(DashMap::new()),
            paper_poll_sem: Arc::new(Semaphore::new(PAPER_POLL_CONCURRENCY)),
            exiting: Arc::new(DashSet::new()),
            entering: Arc::new(DashSet::new()),
            until_dead_armers: Arc::new(DashMap::new()),
            armer_seq: Arc::new(AtomicU64::new(0)),
            pending_first_slot: Arc::new(DashMap::new()),
            first_slot_seq: Arc::new(AtomicU64::new(0)),
            sse_tx: OnceLock::new(),
        }
    }

    /// Install the cold-lane SSE sender so position transitions broadcast
    /// `tpsl_positions_changed` deltas. Called once at boot by the live edge (before
    /// the cache is shared); idempotent (a second call is ignored). Leaving it unset
    /// (tests, `lab`) makes [`emit_position_delta`](Self::emit_position_delta) a no-op.
    pub fn set_sse_sender(&self, sse_tx: broadcast::Sender<SseEvent>) {
        let _ = self.sse_tx.set(sse_tx);
    }

    /// Shared semaphore bounding concurrent paper fill-poll tasks.
    pub fn paper_poll_sem(&self) -> Arc<Semaphore> {
        self.paper_poll_sem.clone()
    }

    /// Number of live exit-state memos (test/observability helper).
    #[cfg(test)]
    pub fn exit_state_len(&self) -> usize {
        self.exit_state_by_position.len()
    }

    // ── Rules + parsed params ─────────────────────────────────────────────────

    /// Replace the active rule set, parsing each rule's `params` JSONB once into a
    /// typed [`StrategyParams`]. A rule with an unknown `strategy_id` or
    /// unparseable params is dropped from the active set (and skipped in the params
    /// map) rather than poisoning the whole reload; the count of dropped rules is
    /// returned so the caller can log it.
    pub fn set_rules(&self, rules: Vec<StrategyRule>) -> usize {
        let mut by_id = HashMap::with_capacity(rules.len());
        let mut params = HashMap::with_capacity(rules.len());
        let mut ladders = HashMap::with_capacity(rules.len());
        let mut kept = Vec::with_capacity(rules.len());
        let mut dropped = 0usize;

        for rule in rules {
            let parsed = StrategyImpl::from_id(&rule.strategy_id)
                .and_then(|s| s.parse_params(&rule.params).ok());
            match parsed {
                Some(p) => {
                    ladders.insert(rule.id, LadderParamsImpl::from_params(&p));
                    params.insert(rule.id, p);
                    by_id.insert(rule.id, rule.clone());
                    kept.push(rule);
                }
                None => dropped += 1,
            }
        }

        *self.active_rules.write().unwrap() = Arc::new(kept);
        *self.rules_by_id.write().unwrap() = by_id;
        *self.params_by_id.write().unwrap() = params;
        *self.ladder_by_id.write().unwrap() = ladders;
        // A rule's time-exit config may have changed, flipping membership for its
        // open positions — rebuild the time-exit index against the new ladders.
        self.rebuild_time_exit_index();
        dropped
    }

    /// Snapshot of the active rules (cheap `Arc` clone — no Vec copy).
    pub fn active_rules(&self) -> Arc<Vec<StrategyRule>> {
        self.active_rules.read().unwrap().clone()
    }

    /// A single rule by id (a clone — callers shouldn't hold the lock).
    pub fn rule_by_id(&self, rule_id: Uuid) -> Option<StrategyRule> {
        self.rules_by_id.read().unwrap().get(&rule_id).cloned()
    }

    /// The pre-parsed params for a rule (clone). `None` if the rule isn't active or
    /// had unparseable params.
    pub fn params_by_id(&self, rule_id: Uuid) -> Option<StrategyParams> {
        self.params_by_id.read().unwrap().get(&rule_id).cloned()
    }

    /// The [`StrategyImpl`] a rule dispatches to (from its `strategy_id`).
    pub fn strategy_of(&self, rule_id: Uuid) -> Option<StrategyImpl> {
        self.rules_by_id
            .read()
            .unwrap()
            .get(&rule_id)
            .and_then(|r| StrategyImpl::from_id(&r.strategy_id))
    }

    /// The resolved exit ladder for a rule (a cheap clone — scalar fields). `None`
    /// if the rule isn't active. The per-trade exit gate + clock sweep read this
    /// instead of rebuilding the ladder from the rule per event.
    pub fn ladder_params_by_id(&self, rule_id: Uuid) -> Option<LadderParamsImpl> {
        self.ladder_by_id.read().unwrap().get(&rule_id).cloned()
    }

    /// Whether a rule currently carries a time-based exit (TimeStop / Stall) — the
    /// time-exit secondary-index membership gate.
    fn rule_has_time_exit(&self, rule_id: Uuid) -> bool {
        self.ladder_by_id
            .read()
            .unwrap()
            .get(&rule_id)
            .map(LadderParamsImpl::has_time_exit)
            .unwrap_or(false)
    }

    // ── Clock-driven exit memo + time-exit index ──────────────────────────────

    /// Snapshot of only the holdings whose rule carries a time-based exit. The
    /// per-second sweep iterates this (usually small/empty) set instead of every
    /// holding. Pointer-clones, no DashMap guard held across the await that follows.
    pub fn time_exit_holding_positions(&self) -> Vec<Arc<StrategyPosition>> {
        self.time_exit_holding.iter().map(|e| e.value().clone()).collect()
    }

    /// Rebuild the time-exit index from the current holdings + ladders. Cheap
    /// (holdings are small); called on bulk holding loads and rule reloads.
    fn rebuild_time_exit_index(&self) {
        self.time_exit_holding.clear();
        for entry in self.holding_by_mint.iter() {
            for pos in entry.value() {
                if pos.rule_id.is_some_and(|r| self.rule_has_time_exit(r)) {
                    self.time_exit_holding.insert(pos.id, pos.clone());
                }
            }
        }
    }

    /// **Trade-gate**: fold the newly-printed trades into the position's memoized
    /// exit-walk state (seeding it on first sight) and evaluate the exit ladder
    /// against only those new trades, returning the first exit reason that fires.
    /// Strategy-dispatched via the rule's stored ladder; `None` if the rule is
    /// inactive. Replaces the per-ping full re-walk with an incremental,
    /// decision-equivalent fold (see [`CachedExitStateImpl`]).
    pub fn exit_state_advance_and_find_exit(
        &self,
        position_id: Uuid,
        rule_id: Uuid,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        trades: &[CachedTrade],
        trades_base: u64,
    ) -> Option<&'static str> {
        let ladder = self.ladder_params_by_id(rule_id)?;
        use dashmap::mapref::entry::Entry;
        match self.exit_state_by_position.entry(position_id) {
            Entry::Occupied(mut e) => {
                e.get_mut().advance_and_find_exit(trades, trades_base, &ladder)
            }
            Entry::Vacant(v) => {
                // First sight: an unfolded state at the window front, then fold +
                // evaluate the whole retained window in one pass (reproducing a full
                // re-walk on the position's first ping while memoizing for later).
                let mut cached =
                    CachedExitStateImpl::build_unfolded(trades_base, entry_price, entry_time, &ladder);
                let reason = cached.advance_and_find_exit(trades, trades_base, &ladder);
                v.insert(cached);
                reason
            }
        }
    }

    /// **Clock sweep**: should this Holding position exit on the wall-clock tick
    /// (E2 TimeStop / E3 Stall)? Reads the memoized peaks (seeding from `trades` only
    /// on first sight) so the sweep never re-walks history. Cheap-exits when the rule
    /// has no time features or the entry fill isn't indexed yet. `None` if the
    /// position has no owning rule / inactive rule.
    pub fn exit_state_clock_check(
        &self,
        position: &StrategyPosition,
        trades: &[CachedTrade],
        trades_base: u64,
        now: DateTime<Utc>,
    ) -> Option<&'static str> {
        let rule_id = position.rule_id?;
        let ladder = self.ladder_params_by_id(rule_id)?;
        if !ladder.has_time_exit() {
            return None;
        }
        let entry_time = clock_entry_time(position)?;
        let entry_price = position.entry_price.unwrap_or(0.0);
        use dashmap::mapref::entry::Entry;
        match self.exit_state_by_position.entry(position.id) {
            Entry::Occupied(e) => e.get().clock_exit_reason(entry_time, &ladder, now),
            Entry::Vacant(v) => {
                let cached = CachedExitStateImpl::build(
                    trades,
                    trades_base,
                    entry_price,
                    entry_time,
                    &ladder,
                );
                let reason = cached.clock_exit_reason(entry_time, &ladder, now);
                v.insert(cached);
                reason
            }
        }
    }

    // ── In-flight guards ──────────────────────────────────────────────────────

    /// Claim `position_id` for an in-flight exit. `Some(ExitGuard)` if newly
    /// claimed (hold it for the whole exit); `None` if an exit is already running
    /// — the caller MUST skip (no-double-sell). The slot frees on drop incl. panic.
    pub fn try_begin_exit(&self, position_id: Uuid) -> Option<ExitGuard> {
        self.exiting
            .insert(position_id)
            .then(|| ExitGuard { exiting: self.exiting.clone(), position_id })
    }

    /// Whether an exit is in flight for `position_id`.
    pub fn is_exiting(&self, position_id: Uuid) -> bool {
        self.exiting.contains(&position_id)
    }

    /// Claim `position_id` for an in-flight entry (a real snipe buy). Same
    /// semantics as [`try_begin_exit`], buy-side.
    pub fn try_begin_entry(&self, position_id: Uuid) -> Option<EntryGuard> {
        self.entering
            .insert(position_id)
            .then(|| EntryGuard { entering: self.entering.clone(), position_id })
    }

    /// Whether a buy is in flight for `position_id`.
    pub fn is_entering(&self, position_id: Uuid) -> bool {
        self.entering.contains(&position_id)
    }

    /// Claim an **until-dead** scalp-armer slot for `position_id` (an entry watch
    /// with no `max_age` ceiling). Bounded to [`MAX_UNTIL_DEAD_ARMERS`]: when full,
    /// the OLDEST un-entered armer is evicted — its
    /// [`is_cancelled`](UntilDeadArmerGuard::is_cancelled) flips true so its watch
    /// loop bails and drops its unentered position — and the eviction is logged (no
    /// silent cap; data-scale guardrail). The returned guard frees this slot on drop
    /// (normal return, eviction, or panic). Hold it only for the watch's duration:
    /// once armed (an entry is found), drop it so the slot frees while the buy runs.
    pub fn begin_until_dead_armer(&self, position_id: Uuid) -> UntilDeadArmerGuard {
        // A position re-arming reuses its own slot — clear any stale entry first so
        // it never evicts itself or double-counts against the cap.
        self.until_dead_armers.remove(&position_id);
        // Evict oldest while at capacity. Copy out the key + cancel flag before
        // `remove`, so no DashMap ref is held across the mutation.
        while self.until_dead_armers.len() >= MAX_UNTIL_DEAD_ARMERS {
            let oldest = self
                .until_dead_armers
                .iter()
                .min_by_key(|e| e.value().seq)
                .map(|e| (*e.key(), e.value().cancelled.clone()));
            match oldest {
                Some((id, cancelled)) => {
                    cancelled.store(true, Ordering::Release);
                    self.until_dead_armers.remove(&id);
                    tracing::warn!(
                        evicted_position = %id,
                        cap = MAX_UNTIL_DEAD_ARMERS,
                        "until-dead scalp-armer cap reached; evicting the oldest un-entered armer"
                    );
                }
                None => break,
            }
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let seq = self.armer_seq.fetch_add(1, Ordering::Relaxed);
        self.until_dead_armers
            .insert(position_id, UntilDeadArmerSlot { seq, cancelled: cancelled.clone() });
        UntilDeadArmerGuard {
            registry: self.until_dead_armers.clone(),
            position_id,
            cancelled,
        }
    }

    // ── Deferred first-slot entry gate ────────────────────────────────────────

    /// Whether any deferred first-slot entry gate is queued.
    pub fn has_first_slot_pending(&self) -> bool {
        !self.pending_first_slot.is_empty()
    }

    /// Whether this mint has at least one deferred first-slot entry gate queued.
    pub fn has_first_slot_pending_for_mint(&self, mint: &str) -> bool {
        self.pending_first_slot.iter().any(|e| e.key().0 == mint)
    }

    /// Queue a deferred first-slot entry gate for `(mint, rule_id)`. Re-queueing the
    /// same key refreshes its slot. Bounded to [`MAX_FIRST_SLOT_PENDING`]: when full,
    /// the oldest pending entry is evicted.
    pub fn queue_first_slot_pending(&self, mint: &str, rule_id: Uuid) {
        let key = (mint.to_string(), rule_id);
        self.pending_first_slot.remove(&key);
        while self.pending_first_slot.len() >= MAX_FIRST_SLOT_PENDING {
            let oldest = self
                .pending_first_slot
                .iter()
                .min_by_key(|e| e.value().seq)
                .map(|e| e.key().clone());
            match oldest {
                Some(k) => {
                    self.pending_first_slot.remove(&k);
                    tracing::warn!(
                        mint = %k.0,
                        rule_id = %k.1,
                        cap = MAX_FIRST_SLOT_PENDING,
                        "first-slot pending cap reached; evicting the oldest pending entry"
                    );
                }
                None => break,
            }
        }
        let seq = self.first_slot_seq.fetch_add(1, Ordering::Relaxed);
        self.pending_first_slot.insert(
            key,
            PendingFirstSlotEntry { queued_at: Utc::now(), seq },
        );
    }

    /// Remove and return every pending first-slot rule id for `mint`.
    pub fn take_first_slot_pending(&self, mint: &str) -> Vec<Uuid> {
        let keys: Vec<(String, Uuid)> = self
            .pending_first_slot
            .iter()
            .filter(|e| e.key().0 == mint)
            .map(|e| e.key().clone())
            .collect();
        keys.into_iter()
            .filter_map(|k| {
                self.pending_first_slot.remove(&k)?;
                Some(k.1)
            })
            .collect()
    }

    /// Expire pending first-slot entries older than `timeout_secs`, removing them
    /// from the map and returning the evicted `(mint, rule_id)` pairs.
    pub fn expire_first_slot_pending(
        &self,
        now: DateTime<Utc>,
        timeout_secs: i64,
    ) -> Vec<(String, Uuid)> {
        let cutoff = now - chrono::Duration::seconds(timeout_secs);
        let stale: Vec<(String, Uuid)> = self
            .pending_first_slot
            .iter()
            .filter(|e| e.value().queued_at <= cutoff)
            .map(|e| e.key().clone())
            .collect();
        stale
            .into_iter()
            .filter_map(|k| {
                self.pending_first_slot.remove(&k)?;
                Some(k)
            })
            .collect()
    }

    // ── Holding index + counters ──────────────────────────────────────────────

    /// Open positions (in the holding index) for a mint.
    pub fn holding_by_mint(&self, mint: &str) -> Vec<Arc<StrategyPosition>> {
        self.holding_by_mint.get(mint).map(|e| e.value().clone()).unwrap_or_default()
    }

    /// Whether any position for this mint is in the holding index.
    pub fn is_mint_held(&self, mint: &str) -> bool {
        self.holding_by_mint.get(mint).is_some_and(|e| !e.value().is_empty())
    }

    /// Every open position across all mints.
    pub fn all_holding_positions(&self) -> Vec<Arc<StrategyPosition>> {
        self.holding_by_mint.iter().flat_map(|e| e.value().clone()).collect()
    }

    /// Open (Holding, entered) positions for a rule — the concurrency-cap count.
    pub fn holding_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.holding_count_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or(0)
    }

    /// Not-yet-filled positions for a rule (`Arming` | `BuySubmitted`) — armed or
    /// buy-in-flight, but not yet holding.
    pub fn pending_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.pending_count_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or(0)
    }

    /// Total entered positions for a rule — the total-token-cap count.
    pub fn total_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.total_count_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or(0)
    }

    /// Realized-performance counters for a rule.
    pub fn closed_stats_by_rule(&self, rule_id: Uuid) -> RuleClosedStats {
        self.closed_stats_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or_default()
    }

    // ── Current-run pointer ───────────────────────────────────────────────────

    /// The current run for a rule (None until a run has started). Mode-agnostic.
    pub fn current_run(&self, rule_id: Uuid) -> Option<RunRef> {
        self.current_run_by_rule.get(&rule_id).map(|e| *e.value())
    }

    /// Point a rule at its current run (set on activation / resume).
    pub fn set_current_run(&self, rule_id: Uuid, run_id: Uuid) {
        self.current_run_by_rule.insert(rule_id, RunRef { run_id });
    }

    /// Drop every in-memory trace of a rule's run — holdings, counters, stats, and
    /// the run pointer — so a fresh run starts from zero. The in-memory twin of
    /// [`start_run`](Self::start_run)'s reset.
    pub fn clear_rule(&self, rule_id: Uuid) {
        self.purge_rule_from_holding_index(rule_id);
        self.holding_count_by_rule.remove(&rule_id);
        self.pending_count_by_rule.remove(&rule_id);
        self.total_count_by_rule.remove(&rule_id);
        self.closed_stats_by_rule.remove(&rule_id);
        self.current_run_by_rule.remove(&rule_id);
    }

    // ── DB load / reload (boot + rule-change edges) ───────────────────────────

    /// Hydrate the whole cache from the DB: active rules (+ parsed params/ladders),
    /// each active rule's current run, and that run's positions (holding index + cap
    /// counters + realized stats). Caps are per current run for **both** modes (every
    /// position carries a run now), so counters warm from the rule's latest `Running`
    /// run only — positions from prior stopped/finished runs are history and never
    /// gate new entries.
    pub async fn load_from_db(&self, repo: &StrategyRepo) -> anyhow::Result<()> {
        let rules = repo.find_active_rules().await?;
        self.set_rules(rules.clone());

        // Reset the per-run counters/stats/run-pointers before warming.
        self.total_count_by_rule.clear();
        self.closed_stats_by_rule.clear();
        self.current_run_by_rule.clear();

        let mut open: Vec<StrategyPosition> = Vec::new();
        for rule in &rules {
            let Some(run) = repo.latest_run(rule.id, &rule.trade_mode).await? else {
                continue;
            };
            if run.status != "Running" {
                continue;
            }
            self.set_current_run(rule.id, run.id);

            let mut entered = 0i64;
            let mut stats = RuleClosedStats::default();
            for p in repo.find_positions_by_run(run.id).await? {
                if p.is_entered() {
                    entered += 1;
                }
                if p.is_closed() {
                    stats.apply(&p, 1);
                }
                if p.is_in_holding_index() {
                    open.push(p);
                }
            }
            if entered > 0 {
                self.total_count_by_rule.insert(rule.id, entered);
            }
            if stats.closed() > 0 {
                self.closed_stats_by_rule.insert(rule.id, stats);
            }
        }

        // Rebuild the holding index (+ holding_count + time-exit index) from the
        // collected current-run open positions.
        self.set_holding_positions(open);
        Ok(())
    }

    /// Reload just the active rule set (on rule CRUD). Rebuilds parsed params,
    /// ladders, and the time-exit index against the new rules.
    pub async fn reload_rules(&self, repo: &StrategyRepo) -> anyhow::Result<()> {
        self.set_rules(repo.find_active_rules().await?);
        Ok(())
    }

    /// Reload the holding index from each active rule's current run (on external
    /// position changes — e.g. the reaper). Counters/stats are left as-is (carried
    /// live); only the open-position index is refreshed.
    pub async fn reload_holding(&self, repo: &StrategyRepo) -> anyhow::Result<()> {
        let runs: Vec<Uuid> = self
            .current_run_by_rule
            .iter()
            .map(|e| e.value().run_id)
            .collect();
        let mut open: Vec<StrategyPosition> = Vec::new();
        for run_id in runs {
            for p in repo.find_positions_by_run(run_id).await? {
                if p.is_in_holding_index() {
                    open.push(p);
                }
            }
        }
        self.set_holding_positions(open);
        Ok(())
    }

    /// Replace the holding index (and its derived holding_count + time-exit index)
    /// from a fresh open-position set, dropping memos for positions no longer held.
    /// Does not touch total_count / closed_stats (warmed separately).
    fn set_holding_positions(&self, positions: Vec<StrategyPosition>) {
        self.holding_by_mint.clear();
        self.holding_count_by_rule.clear();
        self.pending_count_by_rule.clear();

        let mut by_mint: HashMap<String, Vec<Arc<StrategyPosition>>> = HashMap::new();
        let mut holding_by_rule: HashMap<Uuid, i64> = HashMap::new();
        let mut pending_by_rule: HashMap<Uuid, i64> = HashMap::new();
        for pos in positions {
            // holding_count tracks entered + Holding positions (cap enforcement).
            if let Some(rid) = pos.rule_id {
                if pos.is_entered() && pos.is_holding() {
                    *holding_by_rule.entry(rid).or_insert(0) += 1;
                } else if pos.is_in_holding_index() && !pos.is_holding() {
                    // Arming / BuySubmitted — armed or buy in flight, not yet filled.
                    *pending_by_rule.entry(rid).or_insert(0) += 1;
                }
            }
            by_mint.entry(pos.mint_address.clone()).or_default().push(Arc::new(pos));
        }

        let live_ids: std::collections::HashSet<Uuid> = by_mint
            .values()
            .flat_map(|l| l.iter().map(|p| p.id))
            .collect();
        for (mint, list) in by_mint {
            self.holding_by_mint.insert(mint, list);
        }
        for (rid, count) in holding_by_rule {
            self.holding_count_by_rule.insert(rid, count);
        }
        for (rid, count) in pending_by_rule {
            self.pending_count_by_rule.insert(rid, count);
        }
        // Drop memos for positions no longer holding; survivors keep theirs.
        self.exit_state_by_position.retain(|id, _| live_ids.contains(id));
        self.rebuild_time_exit_index();
    }

    // ── Run lifecycle ─────────────────────────────────────────────────────────

    /// Finalize a run to `status`: if it never held any position, delete it outright
    /// (safe — nothing references an empty run) instead of leaving an empty
    /// Stopped/Finished stub. Otherwise a normal status update, as before.
    async fn finalize_run(
        &self,
        repo: &StrategyRepo,
        rule_id: Uuid,
        run_id: Uuid,
        status: &str,
    ) -> anyhow::Result<()> {
        if repo.run_position_count(run_id).await? == 0 {
            repo.delete_run(run_id).await?;
            self.current_run_by_rule.remove(&rule_id); // drop the now-dangling pointer
        } else {
            repo.set_run_status(run_id, status, Some(Utc::now())).await?;
        }
        Ok(())
    }

    /// Start a fresh run for a rule: persist a new `Running` run (params frozen),
    /// reset the rule's in-memory run state, and point the rule at it. Runs are
    /// immutable history — prior non-empty runs are kept; a run that never held a
    /// position is deleted instead of finalized, not history.
    pub async fn start_run(
        &self,
        repo: &StrategyRepo,
        rule: &StrategyRule,
    ) -> anyhow::Result<StrategyRun> {
        let run_seq = repo.next_run_seq(rule.id, &rule.trade_mode).await?;
        let now = Utc::now();
        let run = StrategyRun {
            id: Uuid::new_v4(),
            strategy_id: rule.strategy_id.clone(),
            rule_id: Some(rule.id),
            mode: rule.trade_mode.clone(),
            run_seq,
            status: "Running".to_string(),
            params_snapshot: rule.params.clone(),
            max_total_tokens: rule.max_total_tokens,
            started_at: now,
            finished_at: None,
        };
        repo.insert_run(&run).await?;
        // Fresh run ⇒ fresh in-memory state (the prior run's counters/holdings are
        // history); then point the rule at the new run.
        self.clear_rule(rule.id);
        self.set_current_run(rule.id, run.id);
        Ok(run)
    }

    /// Mark a rule's current run `Stopped` (manual deactivation). Open positions are
    /// left to drain. No-op if there's no current `Running` run.
    pub async fn stop_run(&self, repo: &StrategyRepo, rule_id: Uuid) -> anyhow::Result<()> {
        if let Some(r) = self.current_run(rule_id) {
            if let Some(run) = repo.find_run(r.run_id).await? {
                if run.status == "Running" {
                    self.finalize_run(repo, rule_id, run.id, "Stopped").await?;
                }
            }
        }
        Ok(())
    }

    /// Resume a rule's latest run (flip back to `Running`) and point the rule at it,
    /// preserving its recorded positions/counters. Returns the run, or `None` if the
    /// rule has no prior run (caller should [`start_run`](Self::start_run) instead).
    /// `mode` is the rule's `trade_mode`.
    pub async fn resume_run(
        &self,
        repo: &StrategyRepo,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<Option<StrategyRun>> {
        let Some(run) = repo.latest_run(rule_id, mode).await? else {
            return Ok(None);
        };
        if run.status != "Running" {
            repo.set_run_status(run.id, "Running", None).await?;
        }
        self.set_current_run(rule_id, run.id);
        Ok(Some(run))
    }

    /// Mark a rule's current run `Finished` (cap reached + all exited). Returns the
    /// run if it transitioned, else `None`.
    pub async fn finish_run(
        &self,
        repo: &StrategyRepo,
        rule_id: Uuid,
    ) -> anyhow::Result<Option<StrategyRun>> {
        if let Some(r) = self.current_run(rule_id) {
            if let Some(run) = repo.find_run(r.run_id).await? {
                if run.status == "Running" {
                    // `run` is captured before finalizing (which may delete it if it
                    // never held a position) so the caller still gets its `run_seq` for
                    // the `PaperTestFinished` notification either way.
                    self.finalize_run(repo, rule_id, run.id, "Finished").await?;
                    return Ok(Some(run));
                }
            }
        }
        Ok(None)
    }

    // ── Position transitions ──────────────────────────────────────────────────

    /// Non-evicting read of a swing1 position's currently-memoized swing legs — for
    /// stamping onto `extra` BEFORE the closing `update_position` write (so it lands
    /// in the same DB write as the status change, no second round-trip), without
    /// disturbing the memo itself. The real eviction still happens at the following
    /// [`sync_position`](Self::sync_position) call once the write is confirmed —
    /// this is purely a peek, safe to call even if the write later fails (the memo
    /// is untouched either way). `None` for non-swing1 positions or if no memo is
    /// cached yet (e.g. never pinged).
    pub fn peek_swing_legs(&self, position_id: Uuid) -> Option<Vec<crate::strategies::swing_1::swing::SwingLeg>> {
        self.exit_state_by_position.get(&position_id)?.swing_legs()
    }

    /// Apply a position transition to the in-memory index + counters. Call after
    /// the DB write that changed status / created the position. `prev` is the
    /// pre-transition row (None when the position is brand new). Returns the
    /// harvested swing legs when a swing1 position's exit memo was just evicted
    /// (leaving the holding index) — `None` for every other strategy/transition.
    /// Callers closing a swing1 position should merge this into `extra.swing_legs`
    /// before their own `update_position` write; the memo is gone after this call.
    pub fn sync_position(
        &self,
        prev: Option<&StrategyPosition>,
        current: &StrategyPosition,
    ) -> Option<Vec<crate::strategies::swing_1::swing::SwingLeg>> {
        let prev_in_index = prev.map(StrategyPosition::is_in_holding_index).unwrap_or(false);
        let curr_in_index = current.is_in_holding_index();

        let mut harvested_swing_legs = None;
        if prev_in_index {
            self.remove_from_holding_index(prev.unwrap());
            if !curr_in_index {
                // Position is leaving the holding index — its memoized exit state
                // (and time-exit index entry, dropped in `remove_from_holding_index`)
                // is dead. Harvest swing legs from it before the memo is gone for
                // good (a no-op read for tpsl1/tpsl2 memos).
                if let Some((_, evicted)) = self.exit_state_by_position.remove(&prev.unwrap().id) {
                    harvested_swing_legs = evicted.swing_legs();
                }
            }
        }
        if curr_in_index {
            self.upsert_in_holding_index(current);
        }

        let Some(rule_id) = current.rule_id else {
            // No owning rule (deleted) → nothing to count.
            return harvested_swing_legs;
        };

        // total_count: bump exactly once when a position first takes a real entry.
        let prev_entered = prev.map(StrategyPosition::is_entered).unwrap_or(false);
        let curr_entered = current.is_entered();
        if curr_entered && !prev_entered {
            self.adjust_total_count(rule_id, 1);
        }

        // holding_count: entered AND Holding (Arming/BuySubmitted haven't deployed).
        let prev_holding = prev
            .map(|p| p.is_entered() && p.is_holding())
            .unwrap_or(false);
        let curr_holding = curr_entered && current.is_holding();
        if curr_holding && !prev_holding {
            self.adjust_holding_count(rule_id, 1);
        } else if prev_holding && !curr_holding {
            self.adjust_holding_count(rule_id, -1);
        }

        // pending_count: in the holding index but not yet filled (Arming/BuySubmitted).
        let prev_pending = prev
            .map(|p| p.is_in_holding_index() && !p.is_holding())
            .unwrap_or(false);
        let curr_pending = curr_in_index && !current.is_holding();
        if curr_pending && !prev_pending {
            self.adjust_pending_count(rule_id, 1);
        } else if prev_pending && !curr_pending {
            self.adjust_pending_count(rule_id, -1);
        }

        // Realized stats: accumulate once on the transition into a closed state.
        let prev_closed = prev.map(StrategyPosition::is_closed).unwrap_or(false);
        if current.is_closed() && !prev_closed {
            self.closed_stats_by_rule.entry(rule_id).or_default().apply(current, 1);
        }

        // Cold-lane: broadcast the post-transition row + refreshed cap counters so
        // SSE clients patch one row + the badge in place. No-op without a sender.
        self.emit_position_delta(current, false);

        harvested_swing_legs
    }

    /// Roll a position fully out of the cache (a failed insert rollback, or a
    /// deleted row). The exact inverse of the claims `sync_position` made.
    pub fn remove_position(&self, position: &StrategyPosition) {
        if position.is_in_holding_index() {
            self.remove_from_holding_index(position);
        }
        // Drop the memoized exit state + time-exit index entry (the latter is also
        // cleared in `remove_from_holding_index`, harmless to repeat).
        self.exit_state_by_position.remove(&position.id);
        self.time_exit_holding.remove(&position.id);
        let Some(rule_id) = position.rule_id else {
            return;
        };
        if position.is_entered() {
            if position.is_holding() {
                self.adjust_holding_count(rule_id, -1);
            }
            self.adjust_total_count(rule_id, -1);
        } else if position.is_in_holding_index() {
            // Not yet entered (Arming/BuySubmitted) — pending, not holding.
            self.adjust_pending_count(rule_id, -1);
        }
        if position.is_closed() {
            if let Some(mut e) = self.closed_stats_by_rule.get_mut(&rule_id) {
                e.apply(position, -1);
            }
        }

        // Cold-lane: tell SSE clients to drop this row (`removed`). No-op without a sender.
        self.emit_position_delta(position, true);
    }

    /// Broadcast a `tpsl_positions_changed` delta for `position` — the changed row
    /// (adapted to the legacy [`Position`] wire shape so the frontend contract is
    /// unchanged) plus the owning rule's snapshot + live cap counters. `removed`
    /// flags a row the client should drop. No-op when no sender is installed
    /// (tests / `lab`), no SSE client is connected, or the position carries no rule.
    /// Called only from the position-transition funnel, after all in-memory state is
    /// updated and every internal lock/guard is released.
    fn emit_position_delta(&self, position: &StrategyPosition, removed: bool) {
        let Some(tx) = self.sse_tx.get() else {
            return;
        };
        if tx.receiver_count() == 0 {
            return;
        }
        let Some(rule_id) = position.rule_id else {
            return;
        };
        let rule_snapshot = self.rule_by_id(rule_id).map(|rule| {
            Box::new(rule_notif_snapshot(&rule, self.params_by_id(rule_id).as_ref()))
        });
        let _ = tx.send(SseEvent::TpslPositionsChanged {
            strategy: fe_strategy_label(&position.strategy_id).to_string(),
            rule_id,
            rule_snapshot,
            position: Some(Box::new(Position::from(position))),
            removed,
            open_positions: self.holding_count_by_rule(rule_id),
            pending_positions: self.pending_count_by_rule(rule_id),
            total_positions: self.total_count_by_rule(rule_id),
        });
    }

    fn purge_rule_from_holding_index(&self, rule_id: Uuid) {
        let mut emptied: Vec<String> = Vec::new();
        let mut purged_ids: Vec<Uuid> = Vec::new();
        for mut entry in self.holding_by_mint.iter_mut() {
            entry.value_mut().retain(|p| {
                if p.rule_id == Some(rule_id) {
                    purged_ids.push(p.id);
                    false
                } else {
                    true
                }
            });
            if entry.value().is_empty() {
                emptied.push(entry.key().clone());
            }
        }
        for mint in emptied {
            self.holding_by_mint.remove(&mint);
        }
        // The purged positions are gone from the holding index — drop their memoized
        // exit states + time-exit index entries so neither map leaks across runs.
        for id in purged_ids {
            self.exit_state_by_position.remove(&id);
            self.time_exit_holding.remove(&id);
        }
    }

    fn upsert_in_holding_index(&self, position: &StrategyPosition) {
        let arc = Arc::new(position.clone());
        {
            let mut entry = self.holding_by_mint.entry(position.mint_address.clone()).or_default();
            if let Some(slot) = entry.iter_mut().find(|p| p.id == position.id) {
                *slot = arc.clone();
            } else {
                entry.push(arc.clone());
            }
        }
        // Keep the time-exit index in lockstep with the holding entry.
        if position.rule_id.is_some_and(|r| self.rule_has_time_exit(r)) {
            self.time_exit_holding.insert(position.id, arc);
        } else {
            self.time_exit_holding.remove(&position.id);
        }
    }

    fn remove_from_holding_index(&self, position: &StrategyPosition) {
        self.time_exit_holding.remove(&position.id);
        if let Some(mut entry) = self.holding_by_mint.get_mut(&position.mint_address) {
            entry.retain(|p| p.id != position.id);
            if entry.is_empty() {
                drop(entry);
                self.holding_by_mint.remove(&position.mint_address);
            }
        }
    }

    fn adjust_holding_count(&self, rule_id: Uuid, delta: i64) {
        let mut entry = self.holding_count_by_rule.entry(rule_id).or_insert(0);
        *entry = (*entry + delta).max(0);
        if *entry == 0 {
            drop(entry);
            self.holding_count_by_rule.remove(&rule_id);
        }
    }

    fn adjust_pending_count(&self, rule_id: Uuid, delta: i64) {
        let mut entry = self.pending_count_by_rule.entry(rule_id).or_insert(0);
        *entry = (*entry + delta).max(0);
        if *entry == 0 {
            drop(entry);
            self.pending_count_by_rule.remove(&rule_id);
        }
    }

    fn adjust_total_count(&self, rule_id: Uuid, delta: i64) {
        let mut entry = self.total_count_by_rule.entry(rule_id).or_insert(0);
        *entry = (*entry + delta).max(0);
        if *entry == 0 {
            drop(entry);
            self.total_count_by_rule.remove(&rule_id);
        }
    }
}

/// Frontend strategy label ("tpsl1" / "tpsl2") for the SSE `strategy` field. The
/// frontend mode strings are intentionally unchanged in this remake phase.
fn fe_strategy_label(strategy_id: &str) -> &'static str {
    match StrategyImpl::from_id(strategy_id) {
        Some(StrategyImpl::Tpsl2) => "tpsl2",
        // swing1's own frontend page filters SSE frames on `swing_1`, so it must
        // NOT collapse into the tpsl1 catch-all (that would misroute its deltas to
        // the tpsl1 page and starve the swing1 page).
        Some(StrategyImpl::Swing1) => "swing_1",
        _ => "tpsl1",
    }
}

/// Build the compact rule snapshot attached to each position delta from the rule's
/// typed columns (`rule_name` / `trade_mode`) + its parsed params. Both param
/// variants carry the same `p_*` notification fields; an absent/unparsed params map
/// degrades to neutral defaults rather than dropping the snapshot.
fn rule_notif_snapshot(rule: &StrategyRule, params: Option<&StrategyParams>) -> RuleNotifSnapshot {
    macro_rules! from_params {
        ($p:expr) => {
            RuleNotifSnapshot {
                rule_name: rule.rule_name.clone(),
                trade_mode: rule.trade_mode.clone(),
                p_token_initial_buy_sol: $p.p_token_initial_buy_sol,
                tolerance_pct: $p.tolerance_pct,
                p_token_cu_limit: $p.p_token_cu_limit,
                p_token_cu_price: $p.p_token_cu_price,
                p_token_max_sol_cost: $p.p_token_max_sol_cost,
                p_token_spendable_sol_in: $p.p_token_spendable_sol_in,
                p_token_ix_labels: json_str_array(&$p.p_token_ix_labels),
                p_exit_take_profit: $p.p_exit_take_profit,
                p_exit_stop_loss: $p.p_exit_stop_loss,
            }
        };
    }
    match params {
        Some(StrategyParams::Tpsl1(p)) => from_params!(p),
        Some(StrategyParams::Tpsl2(p)) => from_params!(p),
        Some(StrategyParams::Swing1(p)) => from_params!(p),
        None => RuleNotifSnapshot {
            rule_name: rule.rule_name.clone(),
            trade_mode: rule.trade_mode.clone(),
            p_token_initial_buy_sol: None,
            tolerance_pct: 0.0,
            p_token_cu_limit: None,
            p_token_cu_price: None,
            p_token_max_sol_cost: None,
            p_token_spendable_sol_in: None,
            p_token_ix_labels: Vec::new(),
            p_exit_take_profit: 0.0,
            p_exit_stop_loss: 0.0,
        },
    }
}

/// Decode a JSON string-array `Value` into a `Vec<String>` (non-strings skipped).
fn json_str_array(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::{json, Value};
    use uuid::Uuid;

    fn rule(strategy_id: &str, params: Value) -> StrategyRule {
        let now = Utc::now();
        StrategyRule {
            id: Uuid::new_v4(),
            strategy_id: strategy_id.into(),
            rule_name: "r".into(),
            buy_amount_sol: 1.0,
            trade_mode: "paper".into(),
            is_active: true,
            max_concurrent_tokens: None,
            max_total_tokens: None,
            params,
            created_at: now,
            updated_at: now,
        }
    }

    fn position(rule_id: Uuid, mint: &str, status: &str) -> StrategyPosition {
        let now = Utc::now();
        StrategyPosition {
            id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            strategy_id: "tpsl_sniper_1".into(),
            rule_id: Some(rule_id),
            mode: "paper".into(),
            mint_address: mint.into(),
            wallet: "w".into(),
            token_program_id: None,
            token_account: None,
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: None,
            entry_token_amount: None,
            entry_sol: None,
            entry_time: None,
            entry_tx_signatures: json!([]),
            exit_price: None,
            exit_token_amount: None,
            exit_sol: None,
            exit_time: None,
            exit_tx_signatures: json!([]),
            submitted_buy_signatures: vec![],
            status: status.into(),
            exit_reason: None,
            extra: json!({}),
            created_at: now,
            updated_at: now,
        }
    }

    /// An entered, Holding position with the given realized entry cost.
    fn entered(rule_id: Uuid, mint: &str, entry_price: f64, entry_sol: f64) -> StrategyPosition {
        let mut p = position(rule_id, mint, "Holding");
        p.entry_price = Some(entry_price);
        p.entry_sol = Some(entry_sol);
        p.entry_token_amount = Some((entry_sol / entry_price) as u64);
        p
    }

    // ── rules + parsed params ─────────────────────────────────────────────────

    #[test]
    fn set_rules_parses_params_once_and_drops_unknown() {
        let cache = StrategyRuntimeCache::new();
        let good = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 20.0 }),
        );
        let bad_strategy = rule("nope", json!({}));
        let bad_params = rule("tpsl_sniper_1", json!({ "p_exit_take_profit": 50.0 })); // missing stop_loss
        let good_id = good.id;

        let dropped = cache.set_rules(vec![good, bad_strategy, bad_params]);
        assert_eq!(dropped, 2, "unknown strategy + unparseable params dropped");
        assert_eq!(cache.active_rules().len(), 1);
        assert!(cache.params_by_id(good_id).is_some());
        assert_eq!(cache.strategy_of(good_id), Some(StrategyImpl::Tpsl1));
    }

    // ── holdings index + caps ─────────────────────────────────────────────────

    #[test]
    fn inline_claim_then_rollback_is_balanced() {
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();
        // Un-entered Arming claim: in the holding index, but not the cap counters.
        let pos = position(rid, "MintA", "Arming");
        cache.sync_position(None, &pos);
        assert_eq!(cache.holding_count_by_rule(rid), 0);
        assert_eq!(cache.total_count_by_rule(rid), 0);
        assert_eq!(cache.holding_by_mint("MintA").len(), 1);

        cache.remove_position(&pos);
        assert_eq!(cache.holding_by_mint("MintA").len(), 0);
        assert!(!cache.is_mint_held("MintA"));
    }

    #[test]
    fn sse_funnel_emits_delta_on_transition_and_removal() {
        let cache = StrategyRuntimeCache::new();
        let r = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 20.0 }),
        );
        let rid = r.id;
        cache.set_rules(vec![r]);

        let (tx, mut rx) = broadcast::channel::<SseEvent>(8);
        cache.set_sse_sender(tx);

        // A new Arming position → one delta, not removed, scoped to the rule, carrying
        // the rule snapshot (proves the params lookup wired through).
        let pos = position(rid, "MintA", "Arming");
        cache.sync_position(None, &pos);
        match rx.try_recv().expect("a position delta") {
            SseEvent::TpslPositionsChanged {
                rule_id,
                strategy,
                removed,
                position,
                rule_snapshot,
                ..
            } => {
                assert_eq!(rule_id, rid);
                assert_eq!(strategy, "tpsl1");
                assert!(!removed);
                let p = position.expect("position payload");
                assert_eq!(p.mint_address, "MintA");
                assert_eq!(p.status.to_string(), "Arming");
                let snap = rule_snapshot.expect("rule snapshot");
                assert_eq!(snap.p_exit_take_profit, 50.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // Removal → a `removed` delta for the same row.
        cache.remove_position(&pos);
        match rx.try_recv().expect("a removal delta") {
            SseEvent::TpslPositionsChanged { removed, .. } => assert!(removed),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn sse_funnel_is_noop_without_sender() {
        // No sender installed (the unit-test / `lab` default) → transitions don't panic
        // and emit nothing.
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();
        cache.sync_position(None, &position(rid, "MintA", "Arming"));
        cache.remove_position(&position(rid, "MintA", "Arming"));
    }

    #[test]
    fn entered_positions_count_against_caps() {
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();
        cache.sync_position(None, &entered(rid, "MintA", 0.001, 1.0));
        cache.sync_position(None, &entered(rid, "MintB", 0.001, 1.0));
        assert_eq!(cache.holding_count_by_rule(rid), 2);
        assert_eq!(cache.total_count_by_rule(rid), 2);
        assert_eq!(cache.all_holding_positions().len(), 2);
    }

    #[test]
    fn closed_stats_accumulate_once_per_close() {
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();

        // Win: enter Holding, then close above entry (End, exit_sol > entry_sol).
        let prev = entered(rid, "MintWin", 1.0, 100.0);
        cache.sync_position(None, &prev);
        let mut win = prev.clone();
        win.status = "End".into();
        win.exit_price = Some(2.0);
        win.exit_sol = Some(200.0);
        cache.sync_position(Some(&prev), &win);

        let s = cache.closed_stats_by_rule(rid);
        assert_eq!((s.wins, s.losses), (1, 0));
        assert!((s.sum_pnl_sol - 100.0).abs() < 1e-9);
        // Capital-weighted: 100 SOL profit on 100 SOL deployed → +100%.
        assert!((s.sum_entry_sol - 100.0).abs() < 1e-9);
        assert!((s.avg_pnl_pct() - 100.0).abs() < 1e-9);
        // Holding count released on close.
        assert_eq!(cache.holding_count_by_rule(rid), 0);
        assert_eq!(cache.total_count_by_rule(rid), 1);

        // Re-syncing the already-closed row must not double-count.
        cache.sync_position(Some(&win), &win);
        assert_eq!(cache.closed_stats_by_rule(rid).wins, 1);

        // Failed exit on a second position → a loss with a SOL loss.
        let prev2 = entered(rid, "MintFail", 1.0, 100.0);
        cache.sync_position(None, &prev2);
        let mut fail = prev2.clone();
        fail.status = "ExitFailed".into();
        fail.exit_sol = Some(0.0);
        cache.sync_position(Some(&prev2), &fail);

        let s = cache.closed_stats_by_rule(rid);
        assert_eq!((s.wins, s.losses), (1, 1));
        assert!(s.sum_pnl_sol.abs() < 1e-9, "+100 win, -100 failed → 0");
    }

    #[test]
    fn return_pct_sign_always_matches_sol_under_variable_sizing() {
        // The old mean-of-per-trade-% could flip sign vs the summed SOL when
        // position sizes vary. Capital-weighting makes them agree by construction:
        // here many small winners + one big loser → the SOL total is NEGATIVE, so
        // the return % must also be negative (never the `+%`/`−◎` split the UI showed).
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();

        // 5 tiny winners: 0.01 SOL each, exit 2× → +0.01 SOL apiece (+0.05 total).
        for i in 0..5 {
            let prev = entered(rid, &format!("Win{i}"), 1.0, 0.01);
            cache.sync_position(None, &prev);
            let mut win = prev.clone();
            win.status = "End".into();
            win.exit_price = Some(2.0);
            win.exit_sol = Some(0.02);
            cache.sync_position(Some(&prev), &win);
        }
        // 1 big loser: 10 SOL deployed, exit at half → −5 SOL. Dominates the SOL sum.
        let prev = entered(rid, "BigLoss", 1.0, 10.0);
        cache.sync_position(None, &prev);
        let mut loss = prev.clone();
        loss.status = "End".into();
        loss.exit_price = Some(0.5);
        loss.exit_sol = Some(5.0);
        cache.sync_position(Some(&prev), &loss);

        let s = cache.closed_stats_by_rule(rid);
        assert!(s.sum_pnl_sol < 0.0, "0.05 gains − 5 loss → negative SOL");
        assert!(s.avg_pnl_pct() < 0.0, "return % must share the SOL total's sign");
        assert_eq!(
            s.sum_pnl_sol.signum(),
            s.avg_pnl_pct().signum(),
            "capital-weighted return % is sign-locked to the SOL total"
        );
    }

    #[test]
    fn return_pct_equals_mean_of_percents_under_fixed_notional() {
        // Parity with the sweep kernel: when every trade deploys the SAME notional,
        // the capital-weighted return reduces exactly to the mean of per-trade
        // percents (what a fixed-notional backtest reports). +100% and −50% on 1 SOL
        // each → mean +25%, and 0.5 SOL profit / 2 SOL deployed = +25%.
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();

        let a = entered(rid, "A", 1.0, 1.0);
        cache.sync_position(None, &a);
        let mut a_end = a.clone();
        a_end.status = "End".into();
        a_end.exit_price = Some(2.0);
        a_end.exit_sol = Some(2.0); // +100%
        cache.sync_position(Some(&a), &a_end);

        let b = entered(rid, "B", 1.0, 1.0);
        cache.sync_position(None, &b);
        let mut b_end = b.clone();
        b_end.status = "End".into();
        b_end.exit_price = Some(0.5);
        b_end.exit_sol = Some(0.5); // −50%
        cache.sync_position(Some(&b), &b_end);

        let s = cache.closed_stats_by_rule(rid);
        assert!((s.avg_pnl_pct() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn clear_rule_wipes_all_state() {
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();
        cache.sync_position(None, &entered(rid, "MintA", 0.001, 1.0));
        cache.set_current_run(rid, Uuid::new_v4());
        assert_eq!(cache.total_count_by_rule(rid), 1);
        assert!(cache.current_run(rid).is_some());

        cache.clear_rule(rid);
        assert_eq!(cache.total_count_by_rule(rid), 0);
        assert_eq!(cache.holding_count_by_rule(rid), 0);
        assert!(cache.current_run(rid).is_none());
        assert!(cache.all_holding_positions().is_empty());
    }

    // ── guards ────────────────────────────────────────────────────────────────

    #[test]
    fn exit_guard_gates_and_frees_slot() {
        let cache = StrategyRuntimeCache::new();
        let id = Uuid::new_v4();
        let guard = cache.try_begin_exit(id).expect("first claim");
        assert!(cache.is_exiting(id));
        assert!(cache.try_begin_exit(id).is_none(), "double-claim refused");
        drop(guard);
        assert!(!cache.is_exiting(id));
        assert!(cache.try_begin_exit(id).is_some(), "re-claimable after release");
    }

    #[test]
    fn entry_guard_gates_and_frees_slot() {
        let cache = StrategyRuntimeCache::new();
        let id = Uuid::new_v4();
        let guard = cache.try_begin_entry(id).expect("first claim");
        assert!(cache.is_entering(id));
        assert!(cache.try_begin_entry(id).is_none());
        drop(guard);
        assert!(!cache.is_entering(id));
    }

    // ── exit-state memo + time-exit index ─────────────────────────────────────

    fn t0() -> chrono::DateTime<Utc> {
        chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn cached_buy(price: f64, slot: u64, secs: i64) -> CachedTrade {
        CachedTrade {
            wallet: 0,
            is_buy: true,
            amount_sol: 1.0,
            token_amount: 1.0 / price.max(1e-9),
            price_per_token: price,
            slot,
            leg_index: 0,
            block_time: t0() + chrono::Duration::seconds(secs),
            reserve_sol: Some(100.0),
            reserve_token: Some(100.0),
            real_reserve_sol: None,
        }
    }

    /// An entered, Holding position with a recorded entry price + time (so the
    /// clock/trade gates evaluate it).
    fn entered_at(rule_id: Uuid, mint: &str, entry_price: f64) -> StrategyPosition {
        let mut p = entered(rule_id, mint, entry_price, 1.0);
        p.entry_time = Some(t0());
        p
    }

    /// The trade gate folds new trades into the memo and fires the ladder: a
    /// +100% print clears a 50% take-profit and returns the reason string.
    #[test]
    fn trade_gate_advance_fires_take_profit() {
        let cache = StrategyRuntimeCache::new();
        let r = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0 }),
        );
        let rid = r.id;
        cache.set_rules(vec![r]);

        let pos_id = Uuid::new_v4();
        let reason = cache.exit_state_advance_and_find_exit(
            pos_id,
            rid,
            1.0,
            t0(),
            &[cached_buy(2.0, 3, 5)],
            0,
        );
        assert_eq!(reason, Some("TakeProfit"));
        assert_eq!(cache.exit_state_len(), 1, "memo seeded on first sight");
        // An inactive/unknown rule has no ladder → no eval, no memo.
        assert_eq!(
            cache.exit_state_advance_and_find_exit(Uuid::new_v4(), Uuid::new_v4(), 1.0, t0(), &[], 0),
            None
        );
    }

    /// A rule with a time-stop lands its entered holdings in the time-exit index;
    /// a price-only rule never does. The clock sweep fires once the deadline passes.
    #[test]
    fn time_exit_index_membership_and_clock_fire() {
        let cache = StrategyRuntimeCache::new();
        let timed = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0, "p_exit_time_stop_secs": 60 }),
        );
        let priced = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0 }),
        );
        let timed_id = timed.id;
        let priced_id = priced.id;
        cache.set_rules(vec![timed, priced]);

        let tp = entered_at(timed_id, "MintTimed", 1.0);
        let pp = entered_at(priced_id, "MintPriced", 1.0);
        cache.sync_position(None, &tp);
        cache.sync_position(None, &pp);

        let indexed: Vec<_> = cache.time_exit_holding_positions();
        assert_eq!(indexed.len(), 1, "only the time-exit rule's holding is indexed");
        assert_eq!(indexed[0].id, tp.id);

        // Before the deadline: no fire. After it: TimeStop.
        assert_eq!(
            cache.exit_state_clock_check(&tp, &[], 0, t0() + chrono::Duration::seconds(30)),
            None
        );
        assert_eq!(
            cache.exit_state_clock_check(&tp, &[], 0, t0() + chrono::Duration::seconds(120)),
            Some("TimeStop")
        );
        // The price-only position is never a clock-exit candidate.
        assert_eq!(
            cache.exit_state_clock_check(&pp, &[], 0, t0() + chrono::Duration::seconds(120)),
            None
        );
    }

    /// Closing a position drops its memo + time-exit index entry (no leak).
    #[test]
    fn memo_and_index_dropped_when_position_leaves_holding() {
        let cache = StrategyRuntimeCache::new();
        let r = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0, "p_exit_time_stop_secs": 60 }),
        );
        let rid = r.id;
        cache.set_rules(vec![r]);

        let prev = entered_at(rid, "MintX", 1.0);
        cache.sync_position(None, &prev);
        // Seed the memo via a clock check.
        let _ = cache.exit_state_clock_check(&prev, &[], 0, t0() + chrono::Duration::seconds(10));
        assert_eq!(cache.exit_state_len(), 1);
        assert_eq!(cache.time_exit_holding_positions().len(), 1);

        // Close it → leaves the holding index.
        let mut closed = prev.clone();
        closed.status = "End".into();
        closed.exit_price = Some(2.0);
        closed.exit_sol = Some(2.0);
        cache.sync_position(Some(&prev), &closed);

        assert_eq!(cache.exit_state_len(), 0, "memo dropped on close");
        assert_eq!(cache.time_exit_holding_positions().len(), 0, "index entry dropped");
    }

    /// Reloading rules so a position's rule loses its time-exit removes it from the
    /// time-exit index (and adds it when the config gains one).
    #[test]
    fn rule_reload_rebuilds_time_exit_index() {
        let cache = StrategyRuntimeCache::new();
        let mut r = rule(
            "tpsl_sniper_1",
            json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0, "p_exit_time_stop_secs": 60 }),
        );
        let rid = r.id;
        cache.set_rules(vec![r.clone()]);
        cache.sync_position(None, &entered_at(rid, "MintR", 1.0));
        assert_eq!(cache.time_exit_holding_positions().len(), 1);

        // Drop the time-stop from the rule and reload → no longer indexed.
        r.params = json!({ "p_exit_take_profit": 50.0, "p_exit_stop_loss": 90.0 });
        cache.set_rules(vec![r]);
        assert_eq!(
            cache.time_exit_holding_positions().len(),
            0,
            "rebuilt index drops the now-price-only holding"
        );
    }

    #[tokio::test]
    async fn exit_guard_frees_slot_when_task_panics() {
        let cache = StrategyRuntimeCache::new();
        let id = Uuid::new_v4();
        let guard = cache.try_begin_exit(id).expect("claim");
        let handle = tokio::spawn(async move {
            let _g = guard;
            panic!("sell task blew up");
        });
        assert!(handle.await.is_err());
        assert!(!cache.is_exiting(id), "guard freed on panic unwind");
    }

    #[test]
    fn first_slot_pending_queue_and_take() {
        let cache = StrategyRuntimeCache::new();
        let rule_a = Uuid::new_v4();
        let rule_b = Uuid::new_v4();
        cache.queue_first_slot_pending("mint1", rule_a);
        cache.queue_first_slot_pending("mint1", rule_b);
        assert!(cache.has_first_slot_pending());
        assert!(cache.has_first_slot_pending_for_mint("mint1"));

        let taken = cache.take_first_slot_pending("mint1");
        assert_eq!(taken.len(), 2);
        assert!(taken.contains(&rule_a));
        assert!(taken.contains(&rule_b));
        assert!(!cache.has_first_slot_pending_for_mint("mint1"));
        assert!(!cache.has_first_slot_pending());
    }
}
