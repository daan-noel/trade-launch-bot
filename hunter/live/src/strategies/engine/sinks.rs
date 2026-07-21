//! Effect sinks (plan 4.5) — the consumers of the engine's *side-effect* effects:
//! `PositionUpdate` → the `strategy_positions` PG writer, and `ArmedChanged` →
//! SSE. All PG lifecycle writes and all SSE emission for the generic engine live
//! here (the executor only stashes fill signatures via [`FillSigStore`]).
//!
//! The engine speaks in opaque [`PositionId`]s; the sink owns the mapping to the
//! durable `strategy_positions.id` (via [`PositionRegistry`]) and lazily creates
//! one `strategy_runs` row per rule (the run is the parent FK positions need).
//!
//! **Latency:** `BuySubmitted` upserts the in-memory registry **before** the PG
//! insert (insert is spawned; later transitions await that handle so fill writes
//! never race a missing row). `ExitPending` status flips are fire-and-forget so
//! `SubmitSell` is not gated on PG. Runs are warmed on rule reload so the first
//! buy of a rule rarely blocks on `ensure_run`. Cold miss reuses the latest
//! still-`Running` DB run (and collapses empty leading shells) so a process
//! restart does not orphan the paper scoreboard onto a new empty `run_seq`.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::warn;

use hunter_engine::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, LoadedRule, PositionDelta, PositionStatus, RuleId,
    TradeMode,
};

use trading_core::models::ingest::SseEvent;
use trading_core::models::{StrategyPosition, StrategyRun};
use trading_core::state::token_cache::TokenCache;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

use crate::ingest::HeldPoolGate;
use crate::trader::PumpFunTrader;

use super::{ArmedRegistry, FillSigStore, PositionMeta, PositionRegistry};

/// Per-rule facts the sink needs when it materializes a position/run (mode, the
/// lifetime cap for the run row, the frozen params snapshot). Refreshed on every
/// rule reload via [`Sink::set_rules`].
#[derive(Debug, Clone)]
struct RuleInfo {
    mode: TradeMode,
    name: String,
    max_total: Option<i64>,
    params: serde_json::Value,
}

/// The `strategy_id` the generic engine stamps on every run/position (the column
/// lost its meaning under the redesign; dropped in a later cleanup migration).
const GENERIC_STRATEGY_ID: &str = "generic";

/// The `strategy_positions.wallet` sentinel for paper positions (no real wallet).
const PAPER_WALLET: &str = "paper";

pub struct Sink {
    repo: StrategyRepo,
    token_cache: Arc<TokenCache>,
    sse_tx: broadcast::Sender<SseEvent>,
    registry: PositionRegistry,
    armed: ArmedRegistry,
    fill_sigs: FillSigStore,
    /// Real trader — used to release SOL commitments on terminal unentered exits.
    trader: Option<Arc<PumpFunTrader>>,
    /// Retains LaserStream AMM pool subs for unsettled real positions.
    held_pools: Option<HeldPoolGate>,
    /// The real trading wallet address (position owner for real mode).
    wallet: String,
    /// Per-rule info, refreshed on reload.
    rules: HashMap<RuleId, RuleInfo>,
    /// One live run per rule (lazily created on the first position).
    run_cache: HashMap<RuleId, uuid::Uuid>,
    /// In-flight `insert_position` tasks keyed by PG id — later sink transitions
    /// await these so a fast fill cannot `record_entry_fill` before the row exists.
    pending_inserts: HashMap<uuid::Uuid, JoinHandle<()>>,
}

impl Sink {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo: StrategyRepo,
        token_cache: Arc<TokenCache>,
        sse_tx: broadcast::Sender<SseEvent>,
        registry: PositionRegistry,
        armed: ArmedRegistry,
        fill_sigs: FillSigStore,
        wallet: String,
        trader: Option<Arc<PumpFunTrader>>,
        held_pools: Option<HeldPoolGate>,
    ) -> Self {
        Self {
            repo,
            token_cache,
            sse_tx,
            registry,
            armed,
            fill_sigs,
            trader,
            held_pools,
            wallet,
            rules: HashMap::new(),
            run_cache: HashMap::new(),
            pending_inserts: HashMap::new(),
        }
    }

    /// Refresh the per-rule info from a reload (mode, name, cap, params snapshot).
    /// `names` is `(RuleId, rule_name)` parallel to `rules` — `LoadedRule` does not
    /// carry the human label, but SSE notifications need it.
    pub fn set_rules(&mut self, rules: &[LoadedRule], names: &[(RuleId, String)]) {
        let name_by_id: HashMap<RuleId, &str> = names
            .iter()
            .map(|(id, n)| (*id, n.as_str()))
            .collect();
        self.rules = rules
            .iter()
            .map(|r| {
                (
                    r.id,
                    RuleInfo {
                        mode: r.trade_mode,
                        name: name_by_id
                            .get(&r.id)
                            .map(|s| (*s).to_string())
                            .unwrap_or_default(),
                        max_total: (r.max_total_tokens != 0).then_some(r.max_total_tokens as i64),
                        params: r.params.to_value(),
                    },
                )
            })
            .collect();
    }

    /// Ensure every loaded rule has a cached `strategy_runs` id so the first
    /// `BuySubmitted` of a rule does not await `insert_run` on the hot path.
    pub async fn warm_runs(&mut self) {
        let rules: Vec<(RuleId, RuleInfo)> =
            self.rules.iter().map(|(id, info)| (*id, info.clone())).collect();
        for (rule, info) in rules {
            if let Err(e) = self.ensure_run(rule, &info).await {
                warn!(rule = %rule.0, "engine sink: warm_runs failed: {e}");
            }
        }
    }

    /// Consume one `PositionUpdate`: persist the transition to PG, keep the
    /// registry current, and push the SSE delta.
    ///
    /// Terminal handlers return whether the position was finalized. SSE is
    /// emitted **before** `registry.remove` so clients always get a real
    /// `position_id` (and entry price) on End / ExitFailed / ExitUnconfirmed.
    pub async fn on_position_update(&mut self, delta: PositionDelta) {
        let finalized = match delta.status {
            PositionStatus::BuySubmitted => {
                self.on_buy_submitted(&delta).await;
                false
            }
            PositionStatus::Holding => {
                self.on_holding(&delta).await;
                false
            }
            PositionStatus::ExitPending => {
                self.on_status_only(&delta, "ExitPending").await;
                false
            }
            PositionStatus::End => self.on_end(&delta).await,
            PositionStatus::ExitFailed => self.on_terminal_no_fill(&delta, "ExitFailed").await,
            PositionStatus::ExitUnconfirmed => {
                self.on_terminal_no_fill(&delta, "ExitUnconfirmed").await
            }
        };
        self.emit_position_sse(&delta);
        if finalized {
            self.registry.remove(delta.position);
        }
    }

    /// Consume one `ArmedChanged`: push the arming SSE (armed state is push-only
    /// under the redesign — there is no PG row for an un-entered arm).
    pub fn on_armed_changed(&self, delta: &ArmedDelta) {
        let (state, reason) = match delta.state {
            ArmedStateTag::Armed => {
                self.armed.set_armed(delta.rule.0, delta.mint.to_string());
                ("armed", None)
            }
            ArmedStateTag::Disarmed(r) => {
                self.armed.clear(delta.rule.0, delta.mint.as_str());
                ("disarmed", Some(disarm_reason_str(r)))
            }
        };
        let info = self.rules.get(&delta.rule);
        let _ = self.sse_tx.send(SseEvent::StrategyArmedChanged {
            rule_id: delta.rule.0,
            mint_address: delta.mint.to_string(),
            state: state.to_string(),
            reason: reason.map(str::to_string),
            trade_mode: info.map(|i| mode_str(i.mode).to_string()),
            rule_name: info
                .map(|i| i.name.clone())
                .filter(|n| !n.is_empty()),
        });
    }

    // ── PositionUpdate handlers ───────────────────────────────────────────────

    async fn on_buy_submitted(&mut self, delta: &PositionDelta) {
        let Some(info) = self.rules.get(&delta.rule).cloned() else {
            warn!(rule = %delta.rule.0, "engine sink: BuySubmitted for unknown rule — skipping");
            return;
        };
        // Usually a run_cache hit after `warm_runs`; cold miss still awaits once.
        let run_id = match self.ensure_run(delta.rule, &info).await {
            Ok(id) => id,
            Err(e) => {
                warn!(rule = %delta.rule.0, "engine sink: run creation failed: {e}");
                return;
            }
        };
        let mint = delta.mint.to_string();
        let (token_program_id, creator, cashback) = self
            .token_cache
            .get(&mint)
            .map(|e| {
                let t = &e.value().token;
                (t.token_program_id.clone(), Some(t.creator_wallet.clone()), false)
            })
            .unwrap_or((None, None, false));

        let mode_str = mode_str(info.mode);
        let wallet = match info.mode {
            TradeMode::Real => self.wallet.clone(),
            TradeMode::Paper => PAPER_WALLET.to_string(),
        };
        let mut pos = StrategyPosition::new(
            run_id,
            GENERIC_STRATEGY_ID.to_string(),
            delta.rule.0,
            mode_str.to_string(),
            mint.clone(),
            wallet,
        );
        pos.token_program_id = token_program_id.clone();
        pos.status = "BuySubmitted".to_string();

        // Registry first so Pass-2 can spawn the buy immediately; PG insert is
        // backgrounded. Later transitions await `pending_inserts` for this pg_id.
        self.registry.upsert(
            delta.position,
            PositionMeta {
                pg_id: pos.id,
                run_id,
                rule_id: delta.rule,
                mint: mint.clone(),
                trade_mode: info.mode,
                token_program_id,
                creator,
                entry_token_amount: None,
                token_account: None,
                entry_price: None,
                paper_target: None,
                cashback_enabled: cashback,
                inflight_intent: None,
            },
        );
        self.retain_held_pool(&mint, info.mode);
        let repo = self.repo.clone();
        let pg_id = pos.id;
        let handle = tokio::spawn(async move {
            if let Err(e) = repo.insert_position(&pos).await {
                warn!(mint = %mint, pg = %pg_id, "engine sink: insert_position failed: {e}");
            }
        });
        self.pending_inserts.insert(pg_id, handle);
        // Leave Waiting: Enter does not emit ArmedChanged(Disarmed). Clear the
        // armed registry (+ SSE) so snapshots cannot re-inject a stale Waiting row.
        self.leave_waiting(delta.rule.0, delta.mint.as_str());
    }

    async fn on_holding(&mut self, delta: &PositionDelta) {
        // Safety net if BuySubmitted skipped leave_waiting (unknown rule / crash recovery).
        self.leave_waiting(delta.rule.0, delta.mint.as_str());
        let Some(meta) = self.registry.get(delta.position) else { return };
        self.await_pending_insert(meta.pg_id).await;
        let Some(fill) = delta.fill else { return };
        self.retain_held_pool(&meta.mint, meta.trade_mode);
        // Paper: persist the trigger (`target_*`) before the worst-case entry fill
        // so the UI can show the modeled adverse-slippage gap.
        if let Some(target) = meta.paper_target.clone() {
            if let Err(e) = self
                .repo
                .record_target(
                    meta.pg_id,
                    target.price,
                    target.token_amount,
                    target.time,
                    &target.tx,
                )
                .await
            {
                warn!(pg = %meta.pg_id, "engine sink: record_target failed: {e}");
            }
        }
        // Fill signatures + token account stashed by the executor, keyed by intent.
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let entry_tx = fs.sigs.first().map(String::as_str).unwrap_or("");
        let token_account = fs.token_account.as_deref();
        if let Err(e) = self
            .repo
            .record_entry_fill(
                meta.pg_id,
                entry_tx,
                fill.token_amount,
                fill.price,
                fill.sol,
                fill.at,
                token_account,
            )
            .await
        {
            warn!(pg = %meta.pg_id, "engine sink: record_entry_fill failed: {e}");
        }
        self.registry.update(delta.position, |m| {
            m.entry_token_amount = Some(fill.token_amount);
            m.entry_price = Some(fill.price);
            m.paper_target = None;
            if fs.token_account.is_some() {
                m.token_account = fs.token_account.clone();
            }
        });
    }

    /// A status-only transition (ExitPending): fire-and-forget PG so `SubmitSell`
    /// is not gated on the write. Awaits a pending insert first so the row exists.
    async fn on_status_only(&mut self, delta: &PositionDelta, status: &str) {
        let Some(meta) = self.registry.get(delta.position) else { return };
        self.await_pending_insert(meta.pg_id).await;
        let repo = self.repo.clone();
        let pg_id = meta.pg_id;
        let status = status.to_string();
        let reason = delta.reason.map(|r| r.label().into_owned());
        tokio::spawn(async move {
            match repo.find_position(pg_id).await {
                Ok(Some(mut pos)) => {
                    pos.status = status.clone();
                    if let Some(r) = reason {
                        pos.exit_reason = Some(r);
                    }
                    pos.updated_at = Utc::now();
                    if let Err(e) = repo.update_position(&pos).await {
                        warn!(pg = %pg_id, "engine sink: update_position({status}) failed: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => warn!(pg = %pg_id, "engine sink: find_position failed: {e}"),
            }
        });
    }

    /// Returns `true` when finalized — caller emits SSE then drops the registry row.
    async fn on_end(&mut self, delta: &PositionDelta) -> bool {
        let Some(meta) = self.registry.get(delta.position) else {
            return false;
        };
        self.await_pending_insert(meta.pg_id).await;
        let Some(fill) = delta.fill else {
            return false;
        };
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let reason = delta
            .reason
            .map(|r| r.label().into_owned())
            .unwrap_or_else(|| "Metrics".to_string());
        if let Ok(Some(mut pos)) = self.repo.find_position(meta.pg_id).await {
            pos.close(fill.price, fill.sol, fill.token_amount, fs.sigs, fill.at, &reason);
            if let Err(e) = self.repo.update_position(&pos).await {
                warn!(pg = %meta.pg_id, "engine sink: close update failed: {e}");
            }
        }
        self.release_held_pool(&meta.mint, meta.trade_mode, meta.pg_id)
            .await;
        true
    }

    /// A terminal transition with no confirmed fill (ExitFailed / ExitUnconfirmed):
    /// stamp a hypothetical exit price (last known spot) so the row still carries a
    /// PnL for analysis, then persist. Also releases any SOL commitment (idempotent)
    /// so an unentered buy that exhausted retries cannot strand the budget tracker.
    ///
    /// Returns `true` when finalized — caller emits SSE then drops the registry row.
    async fn on_terminal_no_fill(&mut self, delta: &PositionDelta, status: &str) -> bool {
        let Some(meta) = self.registry.get(delta.position) else {
            return false;
        };
        self.await_pending_insert(meta.pg_id).await;
        if let Some(trader) = &self.trader {
            trader.release_sol_for_position(&meta.pg_id.to_string());
        }
        let exit_price = self
            .token_cache
            .get(&meta.mint)
            .and_then(|e| e.value().current_price)
            .or(meta.entry_price)
            .unwrap_or(0.0);
        if let Ok(Some(mut pos)) = self.repo.find_position(meta.pg_id).await {
            match status {
                "ExitUnconfirmed" => pos.mark_exit_unconfirmed(exit_price, Utc::now()),
                _ => pos.mark_exit_failed(exit_price, Utc::now()),
            }
            if let Some(r) = delta.reason {
                pos.exit_reason = Some(r.label().into_owned());
            }
            if let Err(e) = self.repo.update_position(&pos).await {
                warn!(pg = %meta.pg_id, "engine sink: {status} update failed: {e}");
            }
        }
        self.release_held_pool(&meta.mint, meta.trade_mode, meta.pg_id)
            .await;
        true
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Keep this mint's PumpSwap pool on the LaserStream filter for the life of
    /// the real position (independent of `track_post_migration`).
    fn retain_held_pool(&self, mint: &str, mode: TradeMode) {
        if mode != TradeMode::Real {
            return;
        }
        let Some(gate) = &self.held_pools else {
            return;
        };
        gate.note(mint);
        if self
            .token_cache
            .get(mint)
            .map(|e| e.is_migrated)
            .unwrap_or(false)
        {
            gate.track_migrated(mint);
        }
    }

    /// Drop held-pool retention once no other open real position shares the mint.
    async fn release_held_pool(&self, mint: &str, mode: TradeMode, exclude_id: uuid::Uuid) {
        if mode != TradeMode::Real {
            return;
        }
        let Some(gate) = &self.held_pools else {
            return;
        };
        match self
            .repo
            .has_other_open_position_on_mint(&self.wallet, mint, "real", exclude_id)
            .await
        {
            Ok(true) => {}
            Ok(false) => gate.release(mint),
            Err(e) => warn!(
                mint = %mint,
                "engine sink: held-pool release check failed (keeping subscription): {e}"
            ),
        }
    }

    /// Wait for a background `insert_position` (if any) so later PG writes see the row.
    async fn await_pending_insert(&mut self, pg_id: uuid::Uuid) {
        if let Some(handle) = self.pending_inserts.remove(&pg_id) {
            let _ = handle.await;
        }
    }

    /// Drop (rule, mint) from the armed Waiting set and notify clients. Enter does
    /// not emit `ArmedChanged(Disarmed)` from the pure engine — without this,
    /// `GET /api/strategies/armed` keeps the mint and every snapshot re-injects
    /// a stale Waiting row next to Open.
    fn leave_waiting(&self, rule_id: uuid::Uuid, mint: &str) {
        self.armed.clear(rule_id, mint);
        let info = self.rules.get(&RuleId(rule_id));
        let _ = self.sse_tx.send(SseEvent::StrategyArmedChanged {
            rule_id,
            mint_address: mint.to_string(),
            state: "disarmed".to_string(),
            reason: Some("entered".to_string()),
            trade_mode: info.map(|i| mode_str(i.mode).to_string()),
            rule_name: info
                .map(|i| i.name.clone())
                .filter(|n| !n.is_empty()),
        });
    }

    async fn ensure_run(&mut self, rule: RuleId, info: &RuleInfo) -> anyhow::Result<uuid::Uuid> {
        if let Some(id) = self.run_cache.get(&rule) {
            return Ok(*id);
        }
        let mode = mode_str(info.mode);
        // Drop empty newest shells left by the old always-insert warm path so
        // latest-run scoreboard / position reads land on the real bag again.
        match self.repo.delete_empty_leading_runs(rule.0, mode).await {
            Ok(0) => {}
            Ok(n) => {
                warn!(
                    rule = %rule.0,
                    mode,
                    deleted = n,
                    "engine sink: collapsed empty leading strategy_runs"
                );
            }
            Err(e) => {
                warn!(
                    rule = %rule.0,
                    mode,
                    "engine sink: delete_empty_leading_runs failed: {e}"
                );
            }
        }
        // Reuse the open run across restarts. Only mint when none exists or the
        // latest was finalized (Stopped/Finished/Cancelled).
        if let Some(existing) = self.repo.latest_run(rule.0, mode).await? {
            if existing.status == "Running" {
                self.run_cache.insert(rule, existing.id);
                return Ok(existing.id);
            }
        }
        let seq = self.repo.next_run_seq(rule.0, mode).await?;
        let run = StrategyRun {
            id: uuid::Uuid::new_v4(),
            strategy_id: GENERIC_STRATEGY_ID.to_string(),
            rule_id: Some(rule.0),
            mode: mode.to_string(),
            run_seq: seq,
            status: "Running".to_string(),
            params_snapshot: info.params.clone(),
            max_total_tokens: info.max_total,
            started_at: Utc::now(),
            finished_at: None,
        };
        self.repo.insert_run(&run).await?;
        self.run_cache.insert(rule, run.id);
        Ok(run.id)
    }

    fn emit_position_sse(&self, delta: &PositionDelta) {
        let Some(meta) = self.registry.get(delta.position) else {
            // No registry row (duplicate terminal / unknown id) — skip rather
            // than broadcast a nil position_id that cannot patch Live Status.
            warn!(
                rule = %delta.rule.0,
                mint = %delta.mint,
                status = ?delta.status,
                "engine sink: skip position SSE — no registry meta"
            );
            return;
        };
        // Frozen per-position mode (set at BuySubmitted) — not the live rule table.
        let trade_mode = Some(mode_str(meta.trade_mode).to_string());
        let entry_price = delta
            .fill
            .filter(|_| delta.status == PositionStatus::Holding)
            .map(|f| f.price)
            .or(meta.entry_price);
        let exit_price = delta
            .fill
            .filter(|_| delta.status == PositionStatus::End)
            .map(|f| f.price);
        let info = self.rules.get(&delta.rule);
        let _ = self.sse_tx.send(SseEvent::StrategyPositionUpdate {
            rule_id: delta.rule.0,
            mint_address: delta.mint.to_string(),
            position_id: meta.pg_id,
            status: position_status_str(delta.status).to_string(),
            exit_reason: delta.reason.map(|r| r.label().into_owned()),
            entry_price,
            exit_price,
            trade_mode,
            rule_name: info
                .map(|i| i.name.clone())
                .filter(|n| !n.is_empty()),
        });
    }
}

fn mode_str(mode: TradeMode) -> &'static str {
    match mode {
        TradeMode::Real => "real",
        TradeMode::Paper => "paper",
    }
}

fn position_status_str(s: PositionStatus) -> &'static str {
    match s {
        PositionStatus::BuySubmitted => "BuySubmitted",
        PositionStatus::Holding => "Holding",
        PositionStatus::ExitPending => "ExitPending",
        PositionStatus::End => "End",
        PositionStatus::ExitFailed => "ExitFailed",
        PositionStatus::ExitUnconfirmed => "ExitUnconfirmed",
    }
}

fn disarm_reason_str(r: DisarmReason) -> &'static str {
    match r {
        DisarmReason::Dead => "dead",
        DisarmReason::Migrated => "migrated",
        DisarmReason::Unsatisfiable => "unsatisfiable",
    }
}
