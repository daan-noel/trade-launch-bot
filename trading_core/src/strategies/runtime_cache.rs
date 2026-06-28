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
//! Decoupled from DB and SSE on purpose: it mutates only in-memory state, so it is
//! trivially unit-testable. The live edge (Phase 3) wraps DB loads + SSE emission
//! around [`set_rules`](StrategyRuntimeCache::set_rules) /
//! [`sync_position`](StrategyRuntimeCache::sync_position). The per-position
//! clock-driven exit-state memo (`exit_state_by_position`) and the time-exit
//! secondary index are a live-trade-gate optimization tied to the live token
//! cache's trade source, so they land with the Phase-3 wiring.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use dashmap::{DashMap, DashSet};
use uuid::Uuid;

use crate::models::{StrategyPosition, StrategyRule};

use super::registry::{StrategyImpl, StrategyParams};

/// Pointer to a paper rule's current run — the run new paper positions are stamped
/// with and the run the result view surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaperRunRef {
    pub run_id: Uuid,
}

/// Per-rule realized-performance counters, accumulated live on each position close
/// and warmed from the DB on boot. All-time for real rules; current-run for paper
/// rules. Raw sums only — the API layer derives win rate / average PnL %, so the
/// hot path never divides.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct RuleClosedStats {
    /// Clean `End` exits sold above entry.
    pub wins: i64,
    /// Every other closed position (breakeven, loss, failed exit).
    pub losses: i64,
    /// Sum of realized SOL PnL across all closed positions.
    pub sum_pnl_sol: f64,
    /// Sum of realized PnL % across all closed positions.
    pub sum_pnl_pct: f64,
}

impl RuleClosedStats {
    /// Number of closed positions = the win/loss denominator.
    pub fn closed(&self) -> i64 {
        self.wins + self.losses
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
        }
        if let Some(pct) = p.pnl_pct() {
            self.sum_pnl_pct += s * pct;
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
    holding_by_mint: Arc<DashMap<String, Vec<Arc<StrategyPosition>>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
    closed_stats_by_rule: Arc<DashMap<Uuid, RuleClosedStats>>,
    paper_run_by_rule: Arc<DashMap<Uuid, PaperRunRef>>,
    exiting: Arc<DashSet<Uuid>>,
    entering: Arc<DashSet<Uuid>>,
}

impl Default for StrategyRuntimeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl StrategyRuntimeCache {
    pub fn new() -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            params_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
            closed_stats_by_rule: Arc::new(DashMap::new()),
            paper_run_by_rule: Arc::new(DashMap::new()),
            exiting: Arc::new(DashSet::new()),
            entering: Arc::new(DashSet::new()),
        }
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
        let mut kept = Vec::with_capacity(rules.len());
        let mut dropped = 0usize;

        for rule in rules {
            let parsed = StrategyImpl::from_id(&rule.strategy_id)
                .and_then(|s| s.parse_params(&rule.params).ok());
            match parsed {
                Some(p) => {
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

    /// Total entered positions for a rule — the total-token-cap count.
    pub fn total_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.total_count_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or(0)
    }

    /// Realized-performance counters for a rule.
    pub fn closed_stats_by_rule(&self, rule_id: Uuid) -> RuleClosedStats {
        self.closed_stats_by_rule.get(&rule_id).map(|e| *e.value()).unwrap_or_default()
    }

    // ── Paper-run pointer ─────────────────────────────────────────────────────

    /// The current run for a paper rule (None until a run has started).
    pub fn current_paper_run(&self, rule_id: Uuid) -> Option<PaperRunRef> {
        self.paper_run_by_rule.get(&rule_id).map(|e| *e.value())
    }

    /// Point a rule at its current run (set on activation / resume).
    pub fn set_paper_run(&self, rule_id: Uuid, run_id: Uuid) {
        self.paper_run_by_rule.insert(rule_id, PaperRunRef { run_id });
    }

    /// Drop every in-memory trace of a rule's run — holdings, counters, stats, and
    /// the run pointer — so a fresh run starts from zero (the prior run's positions
    /// were deleted in the DB). The in-memory twin of `start_paper_run`'s purge.
    pub fn clear_rule(&self, rule_id: Uuid) {
        self.purge_rule_from_holding_index(rule_id);
        self.holding_count_by_rule.remove(&rule_id);
        self.total_count_by_rule.remove(&rule_id);
        self.closed_stats_by_rule.remove(&rule_id);
        self.paper_run_by_rule.remove(&rule_id);
    }

    // ── Position transitions ──────────────────────────────────────────────────

    /// Apply a position transition to the in-memory index + counters. Call after
    /// the DB write that changed status / created the position. `prev` is the
    /// pre-transition row (None when the position is brand new).
    pub fn sync_position(&self, prev: Option<&StrategyPosition>, current: &StrategyPosition) {
        let prev_in_index = prev.map(StrategyPosition::is_in_holding_index).unwrap_or(false);
        let curr_in_index = current.is_in_holding_index();

        if prev_in_index {
            self.remove_from_holding_index(prev.unwrap());
        }
        if curr_in_index {
            self.upsert_in_holding_index(current);
        }

        let Some(rule_id) = current.rule_id else {
            // No owning rule (deleted) → nothing to count.
            return;
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

        // Realized stats: accumulate once on the transition into a closed state.
        let prev_closed = prev.map(StrategyPosition::is_closed).unwrap_or(false);
        if current.is_closed() && !prev_closed {
            self.closed_stats_by_rule.entry(rule_id).or_default().apply(current, 1);
        }
    }

    /// Roll a position fully out of the cache (a failed insert rollback, or a
    /// deleted row). The exact inverse of the claims `sync_position` made.
    pub fn remove_position(&self, position: &StrategyPosition) {
        if position.is_in_holding_index() {
            self.remove_from_holding_index(position);
        }
        let Some(rule_id) = position.rule_id else {
            return;
        };
        if position.is_entered() {
            if position.is_holding() {
                self.adjust_holding_count(rule_id, -1);
            }
            self.adjust_total_count(rule_id, -1);
        }
        if position.is_closed() {
            if let Some(mut e) = self.closed_stats_by_rule.get_mut(&rule_id) {
                e.apply(position, -1);
            }
        }
    }

    fn purge_rule_from_holding_index(&self, rule_id: Uuid) {
        let mut emptied: Vec<String> = Vec::new();
        for mut entry in self.holding_by_mint.iter_mut() {
            entry.value_mut().retain(|p| p.rule_id != Some(rule_id));
            if entry.value().is_empty() {
                emptied.push(entry.key().clone());
            }
        }
        for mint in emptied {
            self.holding_by_mint.remove(&mint);
        }
    }

    fn upsert_in_holding_index(&self, position: &StrategyPosition) {
        let arc = Arc::new(position.clone());
        let mut entry = self.holding_by_mint.entry(position.mint.clone()).or_default();
        if let Some(slot) = entry.iter_mut().find(|p| p.id == position.id) {
            *slot = arc;
        } else {
            entry.push(arc);
        }
    }

    fn remove_from_holding_index(&self, position: &StrategyPosition) {
        if let Some(mut entry) = self.holding_by_mint.get_mut(&position.mint) {
            entry.retain(|p| p.id != position.id);
            if entry.is_empty() {
                drop(entry);
                self.holding_by_mint.remove(&position.mint);
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

    fn adjust_total_count(&self, rule_id: Uuid, delta: i64) {
        let mut entry = self.total_count_by_rule.entry(rule_id).or_insert(0);
        *entry = (*entry + delta).max(0);
        if *entry == 0 {
            drop(entry);
            self.total_count_by_rule.remove(&rule_id);
        }
    }
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
            buy_amount: 1.0,
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
            mint: mint.into(),
            wallet: "w".into(),
            token_program_id: None,
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
        p.entry_token_amount = Some(entry_sol / entry_price);
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
        assert!((s.sum_pnl_pct - 100.0).abs() < 1e-9);
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
    fn clear_rule_wipes_all_state() {
        let cache = StrategyRuntimeCache::new();
        let rid = Uuid::new_v4();
        cache.sync_position(None, &entered(rid, "MintA", 0.001, 1.0));
        cache.set_paper_run(rid, Uuid::new_v4());
        assert_eq!(cache.total_count_by_rule(rid), 1);
        assert!(cache.current_paper_run(rid).is_some());

        cache.clear_rule(rid);
        assert_eq!(cache.total_count_by_rule(rid), 0);
        assert_eq!(cache.holding_count_by_rule(rid), 0);
        assert!(cache.current_paper_run(rid).is_none());
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
}
