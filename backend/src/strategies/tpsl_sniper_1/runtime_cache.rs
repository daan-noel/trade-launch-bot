use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{PaperRun, PaperRunStatus, Position, PositionStatus, StrategyTPSLRule};
use crate::storage::repositories::{
    tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo, tpsl1_position_repo::Tpsl1PositionRepo,
    tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo,
};

/// Pointer to a paper rule's current run — the run new paper positions are
/// stamped with and the run the result view surfaces.
#[derive(Clone, Copy)]
pub struct PaperRunRef {
    pub run_id: Uuid,
    pub run_seq: i64,
}

/// In-memory TPSL state for the strategy hot path (rules + open positions + rule counters).
///
/// Counters are mode-aware: for real rules `total_count_by_rule` is all-time
/// (from `positions`); for paper rules it is scoped to the current run (reset on
/// each run start). `holding_*` track open positions of both modes (paper rule
/// ids and real rule ids are disjoint, so the shared maps never collide).
#[derive(Clone)]
pub struct Tpsl1RuntimeCache {
    active_rules: Arc<RwLock<Vec<StrategyTPSLRule>>>,
    rules_by_id: Arc<RwLock<HashMap<Uuid, StrategyTPSLRule>>>,
    holding_by_mint: Arc<DashMap<String, Vec<Position>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
    /// Current paper run per paper rule (stamping target + result pointer).
    paper_run_by_rule: Arc<DashMap<Uuid, PaperRunRef>>,
}

impl Tpsl1RuntimeCache {
    pub fn new() -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Vec::new())),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
            paper_run_by_rule: Arc::new(DashMap::new()),
        }
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
                    run_seq: run.run_seq,
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

    fn set_rules(&self, rules: Vec<StrategyTPSLRule>) {
        let active: Vec<_> = rules.iter().filter(|r| r.is_active).cloned().collect();
        let by_id: HashMap<_, _> = rules.into_iter().map(|r| (r.id, r)).collect();
        if let Ok(mut a) = self.active_rules.write() {
            *a = active;
        }
        if let Ok(mut m) = self.rules_by_id.write() {
            *m = by_id;
        }
    }

    fn set_holding_positions(&self, positions: Vec<Position>) {
        self.holding_by_mint.clear();
        self.holding_count_by_rule.clear();

        let mut by_mint: HashMap<String, Vec<Position>> = HashMap::new();
        let mut holding_by_rule: HashMap<Uuid, i64> = HashMap::new();

        for pos in positions {
            *holding_by_rule.entry(pos.rule_id).or_insert(0) += 1;
            by_mint.entry(pos.mint.clone()).or_default().push(pos);
        }

        for (mint, list) in by_mint {
            self.holding_by_mint.insert(mint, list);
        }
        for (rule_id, count) in holding_by_rule {
            self.holding_count_by_rule.insert(rule_id, count);
        }
    }

    pub fn active_rules(&self) -> Vec<StrategyTPSLRule> {
        self.active_rules
            .read()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// O(1) lookup of a single rule by id (clones just that rule). The hot path
    /// uses this instead of cloning every rule per event.
    pub fn rule_by_id(&self, rule_id: Uuid) -> Option<StrategyTPSLRule> {
        self.rules_by_id
            .read()
            .ok()
            .and_then(|m| m.get(&rule_id).cloned())
    }

    pub fn holding_by_mint(&self, mint: &str) -> Vec<Position> {
        self.holding_by_mint
            .get(mint)
            .map(|e| e.value().clone())
            .unwrap_or_default()
    }

    /// Snapshot of every Holding position across all mints. Used by the
    /// time-driven exit sweep, which must scan all open positions on each tick
    /// (not just those of a mint that just traded). Clones out so no DashMap
    /// guard is held across the caller's awaits.
    pub fn all_holding_positions(&self) -> Vec<Position> {
        self.holding_by_mint
            .iter()
            .flat_map(|e| e.value().clone())
            .collect()
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
                run_seq: run.run_seq,
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
        for mut entry in self.holding_by_mint.iter_mut() {
            entry.value_mut().retain(|p| p.rule_id != rule_id);
            if entry.value().is_empty() {
                emptied.push(entry.key().clone());
            }
        }
        for mint in emptied {
            self.holding_by_mint.remove(&mint);
        }
    }

    /// Call after DB writes that change position status or create/delete a position.
    pub fn sync_position(&self, prev: Option<&Position>, current: &Position) {
        if let Some(p) = prev {
            if p.status == PositionStatus::Holding {
                self.remove_from_holding_index(p);
                if current.status != PositionStatus::Holding {
                    self.adjust_holding_count(p.rule_id, -1);
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
        self.adjust_total_count(position.rule_id, -1);
    }

    fn upsert_in_holding_index(&self, position: &Position) {
        let mut entry = self
            .holding_by_mint
            .entry(position.mint.clone())
            .or_insert_with(Vec::new);
        if let Some(slot) = entry.iter_mut().find(|p| p.id == position.id) {
            *slot = position.clone();
        } else {
            entry.push(position.clone());
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
