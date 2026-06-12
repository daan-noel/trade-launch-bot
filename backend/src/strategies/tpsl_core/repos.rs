//! Repo traits for the unified TPSL engine (Phase 5.1, step 2).
//!
//! The two strategies keep **table-level DB isolation** — each owns its own
//! `tpslN_*` tables behind a concrete repo. These traits capture the exact
//! method surface the engine calls so the (soon-to-be-shared) engine can be
//! written once against the trait while the concrete repos stay per-strategy.
//!
//! This step is purely **additive**: the traits are defined here and `impl`ed
//! for the six concrete repos; no engine code is re-pointed at them yet (that
//! is steps 3–5). The trait method names match the inherent methods verbatim,
//! so each impl is a one-line delegation (inherent methods win method
//! resolution, so `self.insert(..)` calls the concrete one, not the trait).
//!
//! `#[async_trait]` is used (rather than native AFIT) so the engine retains the
//! freedom to hold repos as either `V::PositionRepo` generics or `Arc<dyn …>`
//! when the modules move into `tpsl_core`.

// Scaffolding consumed by steps 3–5 (TpslVariant + the moved engine modules);
// unused until then by construction.
#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{PaperRun, PaperRunStatus, Position, TpslRule};
use crate::storage::repositories::tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo;
use crate::storage::repositories::tpsl1_position_repo::Tpsl1PositionRepo;
use crate::storage::repositories::tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo;
use crate::storage::repositories::tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo;
use crate::storage::repositories::tpsl2_position_repo::Tpsl2PositionRepo;
use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;

// ===========================================================================
// PositionRepo — the real `tpslN_real_positions` table.
// ===========================================================================

/// Real-position persistence surface (the `tpslN_real_positions` table). Both
/// `Tpsl1PositionRepo` and `Tpsl2PositionRepo` implement this identically; only
/// the underlying table name differs.
#[async_trait]
pub trait PositionRepo: Send + Sync {
    /// Create a new position.
    async fn insert(&self, position: &Position) -> anyhow::Result<()>;

    /// Update an existing position (e.g., close it).
    async fn update(&self, position: &Position) -> anyhow::Result<()>;

    /// Update entry fields after a fill (entry_tx, entry_amount, entry_price, entry_time).
    async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// Delete a position by ID.
    async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()>;

    /// Get a specific position by ID.
    async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>>;

    /// Get all holding positions for a specific token.
    async fn find_holding_by_mint(&self, mint: &str) -> anyhow::Result<Vec<Position>>;

    /// Get all holding positions for a wallet.
    async fn find_holding_by_wallet(&self, wallet: &str) -> anyhow::Result<Vec<Position>>;

    /// Get all positions for a specific rule.
    async fn find_by_rule(&self, rule_id: Uuid) -> anyhow::Result<Vec<Position>>;

    /// Get positions by strategy.
    async fn find_by_strategy(&self, strategy: &str) -> anyhow::Result<Vec<Position>>;

    /// All positions with status Holding (for runtime cache warm-up).
    async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>>;

    /// Total position count per rule (all statuses).
    async fn count_all_by_rule(&self) -> anyhow::Result<Vec<(Uuid, i64)>>;

    /// Terminally fail positions stuck in ExitPending past the timeout. Returns rows failed.
    async fn fail_stale_exit_pending(&self, stale_after: Duration) -> anyhow::Result<u64>;
}

macro_rules! impl_position_repo {
    ($ty:ty) => {
        #[async_trait]
        impl PositionRepo for $ty {
            async fn insert(&self, position: &Position) -> anyhow::Result<()> {
                self.insert(position).await
            }
            async fn update(&self, position: &Position) -> anyhow::Result<()> {
                self.update(position).await
            }
            async fn update_entry(
                &self,
                position_id: Uuid,
                entry_tx: &str,
                entry_amount: f64,
                entry_price: f64,
                entry_time: DateTime<Utc>,
            ) -> anyhow::Result<()> {
                self.update_entry(position_id, entry_tx, entry_amount, entry_price, entry_time)
                    .await
            }
            async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
                self.delete_position(position_id).await
            }
            async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>> {
                self.find_by_id(position_id).await
            }
            async fn find_holding_by_mint(&self, mint: &str) -> anyhow::Result<Vec<Position>> {
                self.find_holding_by_mint(mint).await
            }
            async fn find_holding_by_wallet(&self, wallet: &str) -> anyhow::Result<Vec<Position>> {
                self.find_holding_by_wallet(wallet).await
            }
            async fn find_by_rule(&self, rule_id: Uuid) -> anyhow::Result<Vec<Position>> {
                self.find_by_rule(rule_id).await
            }
            async fn find_by_strategy(&self, strategy: &str) -> anyhow::Result<Vec<Position>> {
                self.find_by_strategy(strategy).await
            }
            async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>> {
                self.find_all_holding().await
            }
            async fn count_all_by_rule(&self) -> anyhow::Result<Vec<(Uuid, i64)>> {
                self.count_all_by_rule().await
            }
            async fn fail_stale_exit_pending(&self, stale_after: Duration) -> anyhow::Result<u64> {
                self.fail_stale_exit_pending(stale_after).await
            }
        }
    };
}

impl_position_repo!(Tpsl1PositionRepo);
impl_position_repo!(Tpsl2PositionRepo);

// ===========================================================================
// PaperRepo — the `tpslN_paper_test_run` + `tpslN_paper_positions` tables.
// ===========================================================================

/// Paper-trading persistence surface (runs + paper positions). Both
/// `Tpsl1PaperTradingRepo` and `Tpsl2PaperTradingRepo` implement this
/// identically; only the underlying table names differ.
#[async_trait]
pub trait PaperRepo: Send + Sync {
    // ---- runs -------------------------------------------------------------

    /// Start a fresh run for a rule (deletes the rule's prior run + positions).
    async fn start_run(
        &self,
        rule_id: Uuid,
        max_total_tokens: Option<u64>,
    ) -> anyhow::Result<PaperRun>;

    /// The rule's latest run (the only one retained), if any.
    async fn current_run(&self, rule_id: Uuid) -> anyhow::Result<Option<PaperRun>>;

    /// The latest run for every rule that has one (used to warm the cache).
    async fn find_all_runs(&self) -> anyhow::Result<Vec<PaperRun>>;

    /// Set a run's status, optionally stamping `finished_at = now()`.
    async fn mark_run_status(
        &self,
        run_id: Uuid,
        status: PaperRunStatus,
        finished: bool,
    ) -> anyhow::Result<()>;

    /// Resume a paused/finished run (back to `Running`, clear `finished_at`).
    async fn resume_run(&self, run_id: Uuid) -> anyhow::Result<()>;

    // ---- positions --------------------------------------------------------

    /// Create a paper position bound to a run.
    async fn insert(&self, position: &Position, run_id: Uuid) -> anyhow::Result<()>;

    /// Update entry fields after a (simulated) fill is recorded.
    async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// Close a position with the (simulated) exit fill + reason.
    async fn update_exit(
        &self,
        position_id: Uuid,
        exit_tx: &str,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        exit_reason: &str,
    ) -> anyhow::Result<()>;

    /// Persist status/exit fields of an existing position (e.g. mark ExitPending).
    async fn update(&self, position: &Position) -> anyhow::Result<()>;

    /// Terminally mark an ExitPending position as ExitFailed (no confirming exit).
    async fn mark_exit_failed(
        &self,
        position_id: Uuid,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        exit_reason: &str,
    ) -> anyhow::Result<()>;

    /// Delete a paper position (e.g. a 0-entry row that never filled).
    async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()>;

    /// Get a specific paper position by ID.
    async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>>;

    /// All positions in a run, oldest first (for the run result aggregation).
    async fn find_by_run(&self, run_id: Uuid) -> anyhow::Result<Vec<Position>>;

    /// All Holding paper positions across every (current) run — warms the cache.
    async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>>;

    /// Number of positions recorded in a run (the per-run total-cap counter).
    async fn count_by_run(&self, run_id: Uuid) -> anyhow::Result<i64>;

    /// Position counts for every run in a single GROUP BY (avoids the N+1).
    async fn count_by_run_all(&self) -> anyhow::Result<HashMap<Uuid, i64>>;

    /// Terminally fail paper positions stuck in ExitPending past the timeout. Returns rows failed.
    async fn fail_stale_exit_pending(&self, stale_after: Duration) -> anyhow::Result<u64>;
}

macro_rules! impl_paper_repo {
    ($ty:ty) => {
        #[async_trait]
        impl PaperRepo for $ty {
            async fn start_run(
                &self,
                rule_id: Uuid,
                max_total_tokens: Option<u64>,
            ) -> anyhow::Result<PaperRun> {
                self.start_run(rule_id, max_total_tokens).await
            }
            async fn current_run(&self, rule_id: Uuid) -> anyhow::Result<Option<PaperRun>> {
                self.current_run(rule_id).await
            }
            async fn find_all_runs(&self) -> anyhow::Result<Vec<PaperRun>> {
                self.find_all_runs().await
            }
            async fn mark_run_status(
                &self,
                run_id: Uuid,
                status: PaperRunStatus,
                finished: bool,
            ) -> anyhow::Result<()> {
                self.mark_run_status(run_id, status, finished).await
            }
            async fn resume_run(&self, run_id: Uuid) -> anyhow::Result<()> {
                self.resume_run(run_id).await
            }
            async fn insert(&self, position: &Position, run_id: Uuid) -> anyhow::Result<()> {
                self.insert(position, run_id).await
            }
            async fn update_entry(
                &self,
                position_id: Uuid,
                entry_tx: &str,
                entry_amount: f64,
                entry_price: f64,
                entry_time: DateTime<Utc>,
            ) -> anyhow::Result<()> {
                self.update_entry(position_id, entry_tx, entry_amount, entry_price, entry_time)
                    .await
            }
            async fn update_exit(
                &self,
                position_id: Uuid,
                exit_tx: &str,
                exit_price: f64,
                exit_time: DateTime<Utc>,
                exit_reason: &str,
            ) -> anyhow::Result<()> {
                self.update_exit(position_id, exit_tx, exit_price, exit_time, exit_reason)
                    .await
            }
            async fn update(&self, position: &Position) -> anyhow::Result<()> {
                self.update(position).await
            }
            async fn mark_exit_failed(
                &self,
                position_id: Uuid,
                exit_price: f64,
                exit_time: DateTime<Utc>,
                exit_reason: &str,
            ) -> anyhow::Result<()> {
                self.mark_exit_failed(position_id, exit_price, exit_time, exit_reason)
                    .await
            }
            async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
                self.delete_position(position_id).await
            }
            async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>> {
                self.find_by_id(position_id).await
            }
            async fn find_by_run(&self, run_id: Uuid) -> anyhow::Result<Vec<Position>> {
                self.find_by_run(run_id).await
            }
            async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>> {
                self.find_all_holding().await
            }
            async fn count_by_run(&self, run_id: Uuid) -> anyhow::Result<i64> {
                self.count_by_run(run_id).await
            }
            async fn count_by_run_all(&self) -> anyhow::Result<HashMap<Uuid, i64>> {
                self.count_by_run_all().await
            }
            async fn fail_stale_exit_pending(&self, stale_after: Duration) -> anyhow::Result<u64> {
                self.fail_stale_exit_pending(stale_after).await
            }
        }
    };
}

impl_paper_repo!(Tpsl1PaperTradingRepo);
impl_paper_repo!(Tpsl2PaperTradingRepo);

// ===========================================================================
// RuleRepo — the `tpslN_strategy_rules` table.
// ===========================================================================

/// Strategy-rule persistence surface (the `tpslN_strategy_rules` table). Both
/// repos map rows through their own per-strategy `…DbRow` (tpsl1 fills the
/// scalp fields `None`), so the shared `TpslRule` model serves both while the
/// columns/tables stay isolated.
#[async_trait]
pub trait RuleRepo: Send + Sync {
    /// Insert a new TPSL rule.
    async fn insert(&self, rule: &TpslRule) -> anyhow::Result<()>;

    /// Get all TPSL rules (active and inactive).
    async fn find_all(&self) -> anyhow::Result<Vec<TpslRule>>;

    /// Get a specific rule by ID.
    async fn find_by_id(&self, rule_id: Uuid) -> anyhow::Result<Option<TpslRule>>;

    /// Update an existing TPSL rule.
    async fn update(&self, rule: &TpslRule) -> anyhow::Result<()>;

    /// Delete a rule by ID.
    async fn delete(&self, rule_id: Uuid) -> anyhow::Result<()>;
}

macro_rules! impl_rule_repo {
    ($ty:ty) => {
        #[async_trait]
        impl RuleRepo for $ty {
            async fn insert(&self, rule: &TpslRule) -> anyhow::Result<()> {
                self.insert(rule).await
            }
            async fn find_all(&self) -> anyhow::Result<Vec<TpslRule>> {
                self.find_all().await
            }
            async fn find_by_id(&self, rule_id: Uuid) -> anyhow::Result<Option<TpslRule>> {
                self.find_by_id(rule_id).await
            }
            async fn update(&self, rule: &TpslRule) -> anyhow::Result<()> {
                self.update(rule).await
            }
            async fn delete(&self, rule_id: Uuid) -> anyhow::Result<()> {
                self.delete(rule_id).await
            }
        }
    };
}

impl_rule_repo!(Tpsl1StrategyRuleRepo);
impl_rule_repo!(Tpsl2StrategyRuleRepo);
