use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::exit::{CachedExitState, ExitWalkState};
use crate::models::trade::Trade;
use crate::models::{PaperRun, PaperRunStatus, Position, PositionStatus, Tpsl1StrategyRule};
use crate::storage::repositories::{
    tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo, tpsl1_position_repo::Tpsl1PositionRepo,
    tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo,
};

/// Pointer to a paper rule's current run — the run new paper positions are
/// stamped with and the run the result view surfaces.
#[derive(Clone, Copy)]
pub struct PaperRunRef {
    pub run_id: Uuid,
}

/// In-memory TPSL state for the strategy hot path (rules + open positions + rule counters).
///
/// Counters are mode-aware: for real rules `total_count_by_rule` is all-time
/// (from `positions`); for paper rules it is scoped to the current run (reset on
/// each run start). `holding_*` track open positions of both modes (paper rule
/// ids and real rule ids are disjoint, so the shared maps never collide).
#[derive(Clone)]
pub struct Tpsl1RuntimeCache {
    active_rules: Arc<RwLock<Arc<Vec<Tpsl1StrategyRule>>>>,
    rules_by_id: Arc<RwLock<HashMap<Uuid, Tpsl1StrategyRule>>>,
    holding_by_mint: Arc<DashMap<String, Vec<Arc<Position>>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
    /// Current paper run per paper rule (stamping target + result pointer).
    paper_run_by_rule: Arc<DashMap<Uuid, PaperRunRef>>,
    /// Memoized clock-driven exit walk state per holding position, keyed by
    /// position id. Seeded once and advanced as trades print, so the per-second
    /// time-exit sweep never re-walks a token's full history. Lifecycle is tied
    /// to the holding index: an entry is dropped when its position leaves Holding.
    exit_state_by_position: Arc<DashMap<Uuid, CachedExitState>>,
    /// Caps concurrent paper entry/exit fill-poll tasks. Each spawn acquires a
    /// permit before doing DB work, so a burst of fills can't spawn an unbounded
    /// number of feed-polling tasks all hammering the DB at once.
    paper_poll_sem: Arc<Semaphore>,
}

/// Max concurrent paper fill-poll tasks (entry + exit) for this strategy.
const PAPER_POLL_CONCURRENCY: usize = 64;

impl Tpsl1RuntimeCache {
    pub fn new() -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
            paper_run_by_rule: Arc::new(DashMap::new()),
            exit_state_by_position: Arc::new(DashMap::new()),
            paper_poll_sem: Arc::new(Semaphore::new(PAPER_POLL_CONCURRENCY)),
        }
    }

    /// Shared semaphore bounding concurrent paper fill-poll tasks.
    pub fn paper_poll_sem(&self) -> Arc<Semaphore> {
        self.paper_poll_sem.clone()
    }

    pub async fn load_from_db(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rule_repo = Tpsl1StrategyRuleRepo::new(pool.clone());
        let position_repo = Tpsl1PositionRepo::new(pool.clone());
        let paper_repo = Tpsl1PaperTradingRepo::new(pool.clone());

        self.set_rules(rule_repo.find_all().await?);
        // Holding index (real + paper) — rebuilt from both tables.
        self.load_holdings(pool).await?;

        let paper_ids = self.paper_rule_ids();

        // Total counts: real rules all-time from `positions` (exclude any legacy
        // paper rows still keyed to paper rules); paper rules per current run.
        self.total_count_by_rule.clear();
        for (rule_id, count) in position_repo.count_all_by_rule().await? {
            if !paper_ids.contains(&rule_id) {
                self.total_count_by_rule.insert(rule_id, count);
            }
        }

        self.paper_run_by_rule.clear();
        for run in paper_repo.find_all_runs().await? {
            let count = paper_repo.count_by_run(run.id).await?;
            if count > 0 {
                self.total_count_by_rule.insert(run.rule_id, count);
            }
            self.paper_run_by_rule.insert(
                run.rule_id,
                PaperRunRef {
                    run_id: run.id,
                },
            );
        }

        Ok(())
    }

    /// Rule ids whose `trade_mode == "paper"`, from the loaded rule set.
    fn paper_rule_ids(&self) -> HashSet<Uuid> {
        self.rules_by_id
            .read()
            .map(|m| {
                m.values()
                    .filter(|r| r.trade_mode == "paper")
                    .map(|r| r.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Rebuild the holding index (and holding counts) from both the real
    /// `tpsl1_real_positions` table (excluding paper-rule rows) and `tpsl1_paper_positions`.
    async fn load_holdings(&self, pool: &PgPool) -> anyhow::Result<()> {
        let paper_ids = self.paper_rule_ids();
        let mut all: Vec<Position> = Tpsl1PositionRepo::new(pool.clone())
            .find_all_holding()
            .await?
            .into_iter()
            .filter(|p| !paper_ids.contains(&p.rule_id))
            .collect();
        all.extend(Tpsl1PaperTradingRepo::new(pool.clone()).find_all_holding().await?);
        self.set_holding_positions(all);
        Ok(())
    }

    pub async fn reload_rules(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rules = Tpsl1StrategyRuleRepo::new(pool.clone()).find_all().await?;
        self.set_rules(rules);
        Ok(())
    }

    fn set_rules(&self, rules: Vec<Tpsl1StrategyRule>) {
        let active: Vec<_> = rules.iter().filter(|r| r.is_active).cloned().collect();
        let by_id: HashMap<_, _> = rules.into_iter().map(|r| (r.id, r)).collect();
        if let Ok(mut a) = self.active_rules.write() {
            *a = Arc::new(active);
        }
        if let Ok(mut m) = self.rules_by_id.write() {
            *m = by_id;
        }
    }

    fn set_holding_positions(&self, positions: Vec<Position>) {
        self.holding_by_mint.clear();
        self.holding_count_by_rule.clear();

        let mut by_mint: HashMap<String, Vec<Arc<Position>>> = HashMap::new();
        let mut holding_by_rule: HashMap<Uuid, i64> = HashMap::new();

        for pos in positions {
            *holding_by_rule.entry(pos.rule_id).or_insert(0) += 1;
            by_mint.entry(pos.mint.clone()).or_default().push(Arc::new(pos));
        }

        let live_ids: HashSet<Uuid> = by_mint
            .values()
            .flat_map(|list| list.iter().map(|p| p.id))
            .collect();
        for (mint, list) in by_mint {
            self.holding_by_mint.insert(mint, list);
        }
        for (rule_id, count) in holding_by_rule {
            self.holding_count_by_rule.insert(rule_id, count);
        }
        // Drop memoized exit states for positions no longer holding (e.g. closed
        // out from under a reload); the survivors keep theirs and skip re-seeding.
        self.exit_state_by_position
            .retain(|id, _| live_ids.contains(id));
    }

    /// The active rule set, shared by `Arc` (callers clone the pointer, not the
    /// rules). A new handler is built per token creation, so this is hot.
    pub fn active_rules(&self) -> Arc<Vec<Tpsl1StrategyRule>> {
        self.active_rules
            .read()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// O(1) lookup of a single rule by id (clones just that rule). The hot path
    /// uses this instead of cloning every rule per event.
    pub fn rule_by_id(&self, rule_id: Uuid) -> Option<Tpsl1StrategyRule> {
        self.rules_by_id
            .read()
            .ok()
            .and_then(|m| m.get(&rule_id).cloned())
    }

    pub fn holding_by_mint(&self, mint: &str) -> Vec<Arc<Position>> {
        self.holding_by_mint
            .get(mint)
            .map(|e| e.value().clone())
            .unwrap_or_default()
    }

    /// Snapshot of every Holding position across all mints. Used by the
    /// time-driven exit sweep, which must scan all open positions on each tick
    /// (not just those of a mint that just traded). Positions are held by `Arc`,
    /// so the per-tick snapshot is pointer-clones; a caller deep-clones only the
    /// rare position it actually acts on. No DashMap guard is held across awaits.
    pub fn all_holding_positions(&self) -> Vec<Arc<Position>> {
        self.holding_by_mint
            .iter()
            .flat_map(|e| e.value().clone())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Clock-driven exit memoization (see `exit_state_by_position`)
    // -----------------------------------------------------------------------

    /// The position's memoized walk state, if it has been seeded. The sweep uses
    /// this for already-seen positions so it never touches the trade history.
    pub fn exit_state_get(&self, position_id: Uuid) -> Option<ExitWalkState> {
        self.exit_state_by_position
            .get(&position_id)
            .map(|e| e.value().state)
    }

    /// Seed a position's walk state from its full post-entry history (one-time)
    /// and return it. Called by the sweep the first time it sees a position that
    /// the trade path hasn't already seeded.
    pub fn exit_state_build(
        &self,
        position_id: Uuid,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        trades: &[Trade],
    ) -> ExitWalkState {
        let cached = CachedExitState::build(trades, entry_price, entry_time);
        let state = cached.state;
        self.exit_state_by_position.insert(position_id, cached);
        state
    }

    /// Fold newly-printed trades into a position's walk state (seeding it first
    /// if unseen). Called by the trade path, which already holds the history, so
    /// the sweep finds the state current and never re-walks.
    pub fn exit_state_advance(
        &self,
        position_id: Uuid,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        trades: &[Trade],
    ) {
        self.exit_state_by_position
            .entry(position_id)
            .or_insert_with(|| CachedExitState::build(trades, entry_price, entry_time))
            .advance(trades);
    }

    pub fn holding_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.holding_count_by_rule
            .get(&rule_id)
            .map(|e| *e.value())
            .unwrap_or(0)
    }

    pub fn total_count_by_rule(&self, rule_id: Uuid) -> i64 {
        self.total_count_by_rule
            .get(&rule_id)
            .map(|e| *e.value())
            .unwrap_or(0)
    }

    pub async fn reload_holding(&self, pool: &PgPool) -> anyhow::Result<()> {
        self.load_holdings(pool).await
    }

    // -----------------------------------------------------------------------
    // Paper-run lifecycle (paper rules only)
    // -----------------------------------------------------------------------

    /// The current run for a paper rule (None until a run has started).
    pub fn current_paper_run(&self, rule_id: Uuid) -> Option<PaperRunRef> {
        self.paper_run_by_rule.get(&rule_id).map(|e| *e.value())
    }

    /// Begin a fresh run for a paper rule: persist it (deleting the prior run +
    /// its positions), purge any lingering holdings of this rule from the cache,
    /// and reset the per-run counters. Called on activation and lazily on the
    /// first matching token.
    pub async fn start_paper_run(
        &self,
        pool: &PgPool,
        rule_id: Uuid,
        max_total_tokens: Option<u64>,
    ) -> anyhow::Result<PaperRun> {
        let run = Tpsl1PaperTradingRepo::new(pool.clone())
            .start_run(rule_id, max_total_tokens)
            .await?;
        // The prior run's positions were deleted in the DB; drop any that linger
        // in the in-memory holding index so counts start from zero.
        self.purge_rule_from_holding_index(rule_id);
        self.holding_count_by_rule.remove(&rule_id);
        self.total_count_by_rule.remove(&rule_id);
        self.paper_run_by_rule.insert(
            rule_id,
            PaperRunRef {
                run_id: run.id,
            },
        );
        Ok(run)
    }

    /// Mark a paper rule's current run as Stopped (manual deactivation). Open
    /// positions are left to drain — `on_trade_executed` still exits them.
    pub async fn stop_paper_run(&self, pool: &PgPool, rule_id: Uuid) -> anyhow::Result<()> {
        let repo = Tpsl1PaperTradingRepo::new(pool.clone());
        if let Some(run) = repo.current_run(rule_id).await? {
            if run.status == PaperRunStatus::Running {
                repo.mark_run_status(run.id, PaperRunStatus::Stopped, true).await?;
            }
        }
        Ok(())
    }

    /// Resume the rule's prior run (manual "continue"): flip its latest run back
    /// to `Running` and keep its recorded positions + counters. Returns the run
    /// if one was resumed, or `None` when the rule has no prior run (the caller
    /// should `start_paper_run` a fresh one instead). Unlike `start_paper_run`,
    /// the in-memory holding/total counters are preserved — they were warmed on
    /// load (or carried live since the pause), so the run continues from where it
    /// left off, including its progress toward the total-token cap.
    pub async fn resume_paper_run(
        &self,
        pool: &PgPool,
        rule_id: Uuid,
    ) -> anyhow::Result<Option<PaperRun>> {
        let repo = Tpsl1PaperTradingRepo::new(pool.clone());
        let Some(run) = repo.current_run(rule_id).await? else {
            return Ok(None);
        };
        if run.status != PaperRunStatus::Running {
            repo.resume_run(run.id).await?;
        }
        self.paper_run_by_rule.insert(
            rule_id,
            PaperRunRef {
                run_id: run.id,
            },
        );
        Ok(Some(run))
    }

    /// Mark a paper rule's current run as Finished (cap reached + all exited).
    /// Returns the run if it transitioned, else None.
    pub async fn finish_paper_run(
        &self,
        pool: &PgPool,
        rule_id: Uuid,
    ) -> anyhow::Result<Option<PaperRun>> {
        let repo = Tpsl1PaperTradingRepo::new(pool.clone());
        if let Some(run) = repo.current_run(rule_id).await? {
            if run.status == PaperRunStatus::Running {
                repo.mark_run_status(run.id, PaperRunStatus::Finished, true).await?;
                return Ok(Some(run));
            }
        }
        Ok(None)
    }

    /// Drop every holding-index entry belonging to a rule (used when a new run
    /// deletes the prior run's positions out from under the cache).
    fn purge_rule_from_holding_index(&self, rule_id: Uuid) {
        let mut emptied: Vec<String> = Vec::new();
        let mut purged_ids: Vec<Uuid> = Vec::new();
        for mut entry in self.holding_by_mint.iter_mut() {
            entry.value_mut().retain(|p| {
                if p.rule_id == rule_id {
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
        // The purged positions are gone from the holding index; drop their
        // memoized exit states too so the map doesn't leak across paper runs.
        for id in purged_ids {
            self.exit_state_by_position.remove(&id);
        }
    }

    /// Call after DB writes that change position status or create/delete a position.
    pub fn sync_position(&self, prev: Option<&Position>, current: &Position) {
        if let Some(p) = prev {
            if p.status == PositionStatus::Holding {
                self.remove_from_holding_index(p);
                if current.status != PositionStatus::Holding {
                    self.adjust_holding_count(p.rule_id, -1);
                    // Position is leaving Holding — its memoized exit state is dead.
                    self.exit_state_by_position.remove(&p.id);
                }
            }
        }

        if current.status == PositionStatus::Holding {
            self.upsert_in_holding_index(current);
            if prev.map(|p| p.status != PositionStatus::Holding).unwrap_or(true) {
                self.adjust_holding_count(current.rule_id, 1);
            }
        }

        if prev.is_none() {
            self.adjust_total_count(current.rule_id, 1);
        }
    }

    pub fn remove_position(&self, position: &Position) {
        if position.status == PositionStatus::Holding {
            self.remove_from_holding_index(position);
            self.adjust_holding_count(position.rule_id, -1);
        }
        self.exit_state_by_position.remove(&position.id);
        self.adjust_total_count(position.rule_id, -1);
    }

    fn upsert_in_holding_index(&self, position: &Position) {
        let mut entry = self
            .holding_by_mint
            .entry(position.mint.clone())
            .or_insert_with(Vec::new);
        if let Some(slot) = entry.iter_mut().find(|p| p.id == position.id) {
            *slot = Arc::new(position.clone());
        } else {
            entry.push(Arc::new(position.clone()));
        }
    }

    fn remove_from_holding_index(&self, position: &Position) {
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
