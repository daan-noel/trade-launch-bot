use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Position, PositionStatus, StrategyTPSLRule};
use crate::storage::repositories::{
    position_repo::PositionRepo, strategy_tpsl_rule_repo::StrategyTPSLRuleRepo,
};

/// In-memory TPSL state for the strategy hot path (rules + open positions + rule counters).
#[derive(Clone)]
pub struct TpslRuntimeCache {
    active_rules: Arc<RwLock<Vec<StrategyTPSLRule>>>,
    rules_by_id: Arc<RwLock<HashMap<Uuid, StrategyTPSLRule>>>,
    holding_by_mint: Arc<DashMap<String, Vec<Position>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
}

impl TpslRuntimeCache {
    pub fn new() -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Vec::new())),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
        }
    }

    pub async fn load_from_db(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rule_repo = StrategyTPSLRuleRepo::new(pool.clone());
        let position_repo = PositionRepo::new(pool.clone());

        self.set_rules(rule_repo.find_all().await?);
        self.set_holding_positions(position_repo.find_all_holding().await?);

        self.total_count_by_rule.clear();
        for (rule_id, count) in position_repo.count_all_by_rule().await? {
            self.total_count_by_rule.insert(rule_id, count);
        }

        Ok(())
    }

    pub async fn reload_rules(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rules = StrategyTPSLRuleRepo::new(pool.clone()).find_all().await?;
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

    pub fn all_rules_vec(&self) -> Vec<StrategyTPSLRule> {
        self.rules_by_id
            .read()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn holding_by_mint(&self, mint: &str) -> Vec<Position> {
        self.holding_by_mint
            .get(mint)
            .map(|e| e.value().clone())
            .unwrap_or_default()
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
        let holding = PositionRepo::new(pool.clone()).find_all_holding().await?;
        self.set_holding_positions(holding);
        Ok(())
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
