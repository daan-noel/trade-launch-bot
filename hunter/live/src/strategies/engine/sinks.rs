//! Effect sinks (plan 4.5) — the consumers of the engine's *side-effect* effects:
//! `PositionUpdate` → the `strategy_positions` PG writer, and `ArmedChanged` →
//! SSE. All PG lifecycle writes and all SSE emission for the generic engine live
//! here (the executor only stashes fill signatures via [`FillSigStore`]).
//!
//! The engine speaks in opaque [`PositionId`]s; the sink owns the mapping to the
//! durable `strategy_positions.id` (via [`PositionRegistry`]) and lazily creates
//! one `strategy_runs` row per rule (the run is the parent FK positions need).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::broadcast;
use tracing::warn;

use hunter_engine::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, ExitReason, LoadedRule, PositionDelta, PositionStatus,
    RuleId, TradeMode,
};

use trading_core::models::ingest::SseEvent;
use trading_core::models::{StrategyPosition, StrategyRun};
use trading_core::state::token_cache::TokenCache;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;

use crate::trader::PumpFunTrader;

use super::{ArmedRegistry, FillSigStore, PositionMeta, PositionRegistry};

/// Per-rule facts the sink needs when it materializes a position/run (mode, the
/// lifetime cap for the run row, the frozen params snapshot). Refreshed on every
/// rule reload via [`Sink::set_rules`].
#[derive(Debug, Clone)]
struct RuleInfo {
    mode: TradeMode,
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
    /// The real trading wallet address (position owner for real mode).
    wallet: String,
    /// Per-rule info, refreshed on reload.
    rules: HashMap<RuleId, RuleInfo>,
    /// One live run per rule (lazily created on the first position).
    run_cache: HashMap<RuleId, uuid::Uuid>,
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
    ) -> Self {
        Self {
            repo,
            token_cache,
            sse_tx,
            registry,
            armed,
            fill_sigs,
            trader,
            wallet,
            rules: HashMap::new(),
            run_cache: HashMap::new(),
        }
    }

    /// Refresh the per-rule info from a reload (mode, cap, params snapshot).
    pub fn set_rules(&mut self, rules: &[LoadedRule]) {
        self.rules = rules
            .iter()
            .map(|r| {
                (
                    r.id,
                    RuleInfo {
                        mode: r.trade_mode,
                        max_total: (r.max_total_tokens != 0).then_some(r.max_total_tokens as i64),
                        params: r.params.to_value(),
                    },
                )
            })
            .collect();
    }

    /// Consume one `PositionUpdate`: persist the transition to PG, keep the
    /// registry current, and push the SSE delta.
    pub async fn on_position_update(&mut self, delta: PositionDelta) {
        match delta.status {
            PositionStatus::BuySubmitted => self.on_buy_submitted(&delta).await,
            PositionStatus::Holding => self.on_holding(&delta).await,
            PositionStatus::ExitPending => self.on_status_only(&delta, "ExitPending").await,
            PositionStatus::End => self.on_end(&delta).await,
            PositionStatus::ExitFailed => self.on_terminal_no_fill(&delta, "ExitFailed").await,
            PositionStatus::ExitUnconfirmed => {
                self.on_terminal_no_fill(&delta, "ExitUnconfirmed").await
            }
        }
        self.emit_position_sse(&delta);
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
        let _ = self.sse_tx.send(SseEvent::StrategyArmedChanged {
            rule_id: delta.rule.0,
            mint_address: delta.mint.to_string(),
            state: state.to_string(),
            reason: reason.map(str::to_string),
        });
    }

    // ── PositionUpdate handlers ───────────────────────────────────────────────

    async fn on_buy_submitted(&mut self, delta: &PositionDelta) {
        let Some(info) = self.rules.get(&delta.rule).cloned() else {
            warn!(rule = %delta.rule.0, "engine sink: BuySubmitted for unknown rule — skipping");
            return;
        };
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
        if let Err(e) = self.repo.insert_position(&pos).await {
            warn!(mint = %mint, "engine sink: insert_position failed: {e}");
            return;
        }
        self.registry.upsert(
            delta.position,
            PositionMeta {
                pg_id: pos.id,
                run_id,
                rule_id: delta.rule,
                mint,
                trade_mode: info.mode,
                token_program_id,
                creator,
                entry_token_amount: None,
                token_account: None,
                entry_price: None,
                cashback_enabled: cashback,
                inflight_intent: None,
            },
        );
    }

    async fn on_holding(&mut self, delta: &PositionDelta) {
        let Some(meta) = self.registry.get(delta.position) else { return };
        let Some(fill) = delta.fill else { return };
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
            if fs.token_account.is_some() {
                m.token_account = fs.token_account.clone();
            }
        });
    }

    /// A status-only transition (ExitPending): load the row, flip status, persist.
    async fn on_status_only(&mut self, delta: &PositionDelta, status: &str) {
        let Some(meta) = self.registry.get(delta.position) else { return };
        match self.repo.find_position(meta.pg_id).await {
            Ok(Some(mut pos)) => {
                pos.status = status.to_string();
                if let Some(r) = delta.reason {
                    pos.exit_reason = Some(exit_reason_str(r).to_string());
                }
                pos.updated_at = Utc::now();
                if let Err(e) = self.repo.update_position(&pos).await {
                    warn!(pg = %meta.pg_id, "engine sink: update_position({status}) failed: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => warn!(pg = %meta.pg_id, "engine sink: find_position failed: {e}"),
        }
    }

    async fn on_end(&mut self, delta: &PositionDelta) {
        let Some(meta) = self.registry.get(delta.position) else { return };
        let Some(fill) = delta.fill else { return };
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let reason = delta.reason.map(exit_reason_str).unwrap_or("Metrics");
        if let Ok(Some(mut pos)) = self.repo.find_position(meta.pg_id).await {
            pos.close(fill.price, fill.sol, fill.token_amount, fs.sigs, fill.at, reason);
            if let Err(e) = self.repo.update_position(&pos).await {
                warn!(pg = %meta.pg_id, "engine sink: close update failed: {e}");
            }
        }
        self.registry.remove(delta.position);
    }

    /// A terminal transition with no confirmed fill (ExitFailed / ExitUnconfirmed):
    /// stamp a hypothetical exit price (last known spot) so the row still carries a
    /// PnL for analysis, then persist. Also releases any SOL commitment (idempotent)
    /// so an unentered buy that exhausted retries cannot strand the budget tracker.
    async fn on_terminal_no_fill(&mut self, delta: &PositionDelta, status: &str) {
        let Some(meta) = self.registry.get(delta.position) else { return };
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
                pos.exit_reason = Some(exit_reason_str(r).to_string());
            }
            if let Err(e) = self.repo.update_position(&pos).await {
                warn!(pg = %meta.pg_id, "engine sink: {status} update failed: {e}");
            }
        }
        self.registry.remove(delta.position);
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    async fn ensure_run(&mut self, rule: RuleId, info: &RuleInfo) -> anyhow::Result<uuid::Uuid> {
        if let Some(id) = self.run_cache.get(&rule) {
            return Ok(*id);
        }
        let mode = mode_str(info.mode);
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
        let entry_price = delta
            .fill
            .filter(|_| delta.status == PositionStatus::Holding)
            .map(|f| f.price)
            .or_else(|| self.registry.get(delta.position).and_then(|m| m.entry_price));
        let exit_price = delta
            .fill
            .filter(|_| delta.status == PositionStatus::End)
            .map(|f| f.price);
        let pg_id = self.registry.get(delta.position).map(|m| m.pg_id);
        let _ = self.sse_tx.send(SseEvent::StrategyPositionUpdate {
            rule_id: delta.rule.0,
            mint_address: delta.mint.to_string(),
            position_id: pg_id.unwrap_or_default(),
            status: position_status_str(delta.status).to_string(),
            exit_reason: delta.reason.map(|r| exit_reason_str(r).to_string()),
            entry_price,
            exit_price,
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

fn exit_reason_str(r: ExitReason) -> &'static str {
    match r {
        ExitReason::TakeProfit => "TakeProfit",
        ExitReason::StopLoss => "StopLoss",
        ExitReason::Metrics => "Metrics",
        ExitReason::Dead => "Dead",
        ExitReason::Manual => "Manual",
        ExitReason::Migrated => "Migrated",
    }
}

fn disarm_reason_str(r: DisarmReason) -> &'static str {
    match r {
        DisarmReason::Dead => "dead",
        DisarmReason::Migrated => "migrated",
        DisarmReason::Unsatisfiable => "unsatisfiable",
    }
}
