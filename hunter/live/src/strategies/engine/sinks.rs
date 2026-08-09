//! Effect sinks (plan 4.5) — the consumers of the engine's *side-effect* effects:
//! `PositionUpdate` → the `strategy_positions` PG writer, and `ArmedChanged` →
//! SSE. All PG lifecycle writes and all SSE emission for the generic engine live
//! here (the executor only stashes fill signatures via [`FillSigStore`]).
//!
//! The engine speaks in opaque [`PositionId`]s; the sink owns the mapping to the
//! durable `strategy_positions.id` (via [`PositionRegistry`]) and lazily creates
//! one `strategy_runs` row per rule (the run is the parent FK positions need).
//!
//! **Latency + durability (A0):** `BuySubmitted` upserts the in-memory registry
//! (so Pass-2 can read the frozen trade_mode / spawn buy immediately) and
//! **backgrounds** `insert_position` — the handle is kept so later transitions
//! chain after the row exists. `ExitPending` is fire-and-forget (awaits any
//! pending write only inside the spawn) so `SubmitSell` is not gated on PG.
//! `Holding` updates the registry synchronously (sell sizing) and backgrounds
//! the fill persist. Terminal handlers (`End` / `EntryFailed` / `ExitStuck` /
//! `ExitUnconfirmed`) follow the same rule — they chain-spawn their write instead
//! of awaiting it, so **no** sink transition blocks the decision loop on PG. Order
//! per position is still total (each task awaits that row's previous write first);
//! what a terminal write does not guarantee is landing before its SSE frame, so
//! a client must trust the frame's payload over an immediate refetch.
//! Crash recovery: process-local `SubmittedBuyJournal` + BuySubmitted/ExitPending
//! reapers + boot Holding adopt / externally-cleared reconcile. Runs are warmed
//! on rule reload so the first buy of a rule rarely blocks on `ensure_run`. Cold
//! miss reuses the latest still-`Running` DB run (and collapses empty leading
//! shells) so a process restart does not orphan the paper scoreboard onto a new
//! empty `run_seq`.
//!
//! **Run lifecycle.** A `strategy_runs` row is one *activation* of a rule, not the
//! rule's whole life: the sink mints it on first need while the rule is active and
//! ends it ([`Sink::close_stale_runs`]) the moment a reload reports the rule can
//! no longer feed it — pause, disable, Stop, Stop All, delete, **or an edit to
//! `trade_mode`** (a run belongs to exactly one mode). Re-activating mints the next
//! `run_seq`, which is what makes the Evidence pane's "current run" vs "history"
//! split move. Positions already open keep writing to the run they were born in
//! (the registry carries `run_id`), so a run finalized mid-drain is re-rolled as
//! its stragglers settle.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashSet;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use hunter_engine::event::{
    ArmedDelta, ArmedStateTag, DisarmReason, LoadedRule, PositionDelta, PositionStatus, RuleId,
    TradeMode,
};

use trading_core::models::ingest::SseEvent;
use trading_core::models::{StrategyPosition, StrategyRun};
use trading_core::state::token_cache::TokenCache;
use trading_core::storage::repositories::strategy_repo::{RunFinalize, StrategyRepo};

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

/// The run a rule is currently writing into, **with the mode it was opened for**.
///
/// The mode is not decoration. A run belongs to exactly one mode (`run_seq` is
/// monotonic per `(rule, mode)`) but `strategy_rules.trade_mode` is editable, so a
/// rule-id-only cache hands a paper position the id of the rule's *real* run — the
/// row then reads `strategy_positions.mode = 'paper'` inside `strategy_runs.mode =
/// 'real'`, which silently mis-scopes every run-scoped read and hides a real-money
/// position from the real scoreboard. A mode flip therefore ends the run exactly
/// like a pause does.
#[derive(Debug, Clone, Copy)]
struct CachedRun {
    mode: TradeMode,
    id: uuid::Uuid,
}

/// The `strategy_id` the generic engine stamps on every run/position (the column
/// lost its meaning under the redesign; dropped in a later cleanup migration).
const GENERIC_STRATEGY_ID: &str = "generic";

/// The `strategy_positions.wallet` sentinel for paper positions (no real wallet).
const PAPER_WALLET: &str = "paper";

/// The `strategy_runs.strategy_id` of the ONE manual-position run (rule_id NULL).
const MANUAL_STRATEGY_ID: &str = "manual";

/// Adapter-side facts for one staged manual-buy episode, keyed by the fresh
/// per-episode `RuleId` the command minted. The HTTP handler pre-mints the PG
/// row id (so it can 202 `{position_id}` immediately); the sink consumes this
/// when the `BuySubmitted` delta arrives and stamps `origin='manual'` +
/// `manual_exit` onto the row it creates.
#[derive(Debug, Clone)]
pub struct StagedManual {
    pub pg_id: uuid::Uuid,
    pub manual_exit: Option<serde_json::Value>,
}

/// The "drop this mint's LaserStream pool retention" step of a terminal
/// transition, packaged so the sink's background write task can run it (it needs
/// a PG round trip of its own). `gate: None` = nothing to do — paper mode, or a
/// bin with no ingest gate.
struct HeldPoolRelease {
    gate: Option<HeldPoolGate>,
    repo: StrategyRepo,
    wallet: String,
    mint: String,
    exclude_id: uuid::Uuid,
}

impl HeldPoolRelease {
    /// Release only once no *other* open real position still needs the pool feed.
    /// A failed check keeps the subscription — an extra sub costs bandwidth, a
    /// missing one costs a sell.
    async fn run(self) {
        let Some(gate) = self.gate else { return };
        match self
            .repo
            .has_other_open_position_on_mint(&self.wallet, &self.mint, "real", self.exclude_id)
            .await
        {
            Ok(true) => {}
            Ok(false) => gate.release(&self.mint),
            Err(e) => warn!(
                mint = %self.mint,
                "engine sink: held-pool release check failed (keeping subscription): {e}"
            ),
        }
    }
}

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
    /// Per-rule info, refreshed on reload. Holds **active** rules plus the drain
    /// set (paused rules still carrying open positions), so a draining rule's
    /// transitions still resolve a mode/name.
    rules: HashMap<RuleId, RuleInfo>,
    /// The subset of [`Self::rules`] whose entries are enabled (`is_active` on the
    /// row → `LoadedRule::entry_enabled`). Only these may own a run; everything
    /// else in `rules` is draining and its run is being closed.
    active: HashSet<RuleId>,
    /// One live run per active rule (lazily created on the first position).
    run_cache: HashMap<RuleId, CachedRun>,
    /// Runs this process has finalized. `ensure_run` refuses to reuse one even if
    /// its `Stopped` write has not landed yet — the finalize is backgrounded, so
    /// without this a fast pause→activate could re-adopt the run being closed.
    closed_runs: HashSet<uuid::Uuid>,
    /// Finalized runs that still had unsettled positions when they closed: their
    /// metrics are provisional and get re-rolled as each straggler settles.
    /// Shared with the background finalize/rollup tasks. Entries go inert (never
    /// matched again) once the run is fully settled, so the set stays tiny.
    draining_runs: Arc<DashSet<uuid::Uuid>>,
    /// Staged manual-buy episodes awaiting their `BuySubmitted` delta.
    manual_pending: HashMap<RuleId, StagedManual>,
    /// Live manual episode rule ids (for the SSE `rule_name` = "manual" label).
    manual_episodes: HashSet<RuleId>,
    /// The one manual run row (rule_id NULL), lazily ensured.
    manual_run: Option<uuid::Uuid>,
    /// In-flight PG lifecycle tasks keyed by position PG id. Later sink
    /// transitions take/await these so a fast fill cannot write before the row
    /// exists, without gating buy/sell *spawn* on the decision loop.
    pending_pg: HashMap<uuid::Uuid, JoinHandle<()>>,
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
            active: HashSet::new(),
            run_cache: HashMap::new(),
            closed_runs: HashSet::new(),
            draining_runs: Arc::new(DashSet::new()),
            manual_pending: HashMap::new(),
            manual_episodes: HashSet::new(),
            manual_run: None,
            pending_pg: HashMap::new(),
        }
    }

    /// Stage a manual-buy episode (called by the loop's command handler BEFORE the
    /// `ManualBuy` event folds, so the `BuySubmitted` delta finds it).
    pub fn stage_manual(&mut self, rule: RuleId, staged: StagedManual) {
        self.manual_pending.insert(rule, staged);
    }

    /// Refresh the per-rule info from a reload (mode, name, cap, params snapshot).
    /// `names` is `(RuleId, rule_name)` parallel to `rules` — `LoadedRule` does not
    /// carry the human label, but SSE notifications need it.
    ///
    /// `rules` carries the active set **and** the drain set (paused rules with
    /// open positions, loaded so their exits keep running). Which is which comes
    /// off `LoadedRule::entry_enabled` — the same flag the engine's arm gate reads,
    /// so "may own a run" and "may take an entry" can never disagree. A rule that
    /// may not enter may not own a run: see [`Self::close_stale_runs`].
    pub fn set_rules(&mut self, rules: &[LoadedRule], names: &[(RuleId, String)]) {
        self.active = rules.iter().filter(|r| r.entry_enabled).map(|r| r.id).collect();
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
                        // `None` = unlimited, decoded through the ONE reader (`Cap`)
                        // rather than an ad-hoc `!= 0` here.
                        max_total: r.total_cap().bounded().map(i64::from),
                        params: r.params.to_value(),
                    },
                )
            })
            .collect();
    }

    /// Ensure every **active** rule has a cached `strategy_runs` id so the first
    /// `BuySubmitted` of a rule does not await `insert_run` on the hot path.
    ///
    /// Draining rules are deliberately skipped: they cannot enter, so warming them
    /// would mint a fresh empty run for a rule that is on its way out — exactly the
    /// shell [`Self::close_stale_runs`] just deleted. Call that first.
    pub async fn warm_runs(&mut self) {
        let rules: Vec<(RuleId, RuleInfo)> = self
            .rules
            .iter()
            .filter(|(id, _)| self.active.contains(id))
            .map(|(id, info)| (*id, info.clone()))
            .collect();
        for (rule, info) in rules {
            if let Err(e) = self.ensure_run(rule, &info).await {
                warn!(rule = %rule.0, "engine sink: warm_runs failed: {e}");
            }
        }
    }

    /// End every cached run that is no longer the rule's live run — the
    /// counterpart to `ensure_run`. Called on each reload, so it covers pause,
    /// disable, per-rule Stop, Stop All, and delete through the one path they all
    /// already take (`schedule_engine_reload`).
    ///
    /// Two ways a run stops being live, and both must end it:
    /// * the rule is no longer active — it can take no further entry;
    /// * the rule's `trade_mode` was edited — the run belongs to the *old* mode and
    ///   can never legally receive another position (see [`CachedRun`]).
    ///
    /// Only the cache eviction happens here; the rollup + status write are spawned,
    /// because this runs **on the serialized decision loop** and a run's rollup
    /// reads all of its positions. Correctness does not depend on that write
    /// landing before the next reload: `closed_runs` (RAM, checked by `ensure_run`)
    /// is what stops a re-activation from re-adopting the run being closed.
    pub fn close_stale_runs(&mut self) {
        let closing: Vec<(RuleId, uuid::Uuid)> = self
            .run_cache
            .iter()
            .filter(|(rule, cached)| {
                !self.active.contains(*rule)
                    || self.rules.get(*rule).is_some_and(|i| i.mode != cached.mode)
            })
            .map(|(rule, cached)| (*rule, cached.id))
            .collect();
        for (rule, run_id) in closing {
            self.run_cache.remove(&rule);
            // Positions the engine still holds for this rule may have a PG insert
            // in flight (`BuySubmitted` is written in the background), so the run
            // can read as empty while a row naming it is about to commit — the
            // finalize must not delete the parent out from under it.
            let in_flight = !self.registry.positions_for_rule(rule).is_empty();
            self.spawn_finalize(run_id, "paused", in_flight);
        }
    }

    /// Finalize runs left `Running` by a deactivation this process never saw — rows
    /// predating the run lifecycle, a DB sync from another box, a hand-edited
    /// `is_active`. Normally a no-op; without it the next activation of such a rule
    /// would be handed the stale run back. Boot-only, one indexed query.
    pub async fn close_orphan_runs(&mut self) {
        let ids = match self.repo.orphan_running_runs().await {
            Ok(ids) => ids,
            Err(e) => {
                warn!("engine sink: orphan_running_runs failed: {e}");
                return;
            }
        };
        for run_id in ids {
            // Boot-only, and `adopt_from_db` has not run yet, so the registry is
            // empty by construction — nothing can be mid-insert.
            self.spawn_finalize(run_id, "orphaned", false);
        }
    }

    /// Mark a run closed (so `ensure_run` can never re-adopt it) and background the
    /// rollup + terminal-status write. Never awaited: this is called from the
    /// serialized decision loop, and a rollup reads all of the run's positions.
    ///
    /// `in_flight` = the engine still holds positions of this run, so it is
    /// draining: the run keeps its row (never deleted) and its metrics are re-rolled
    /// as those positions settle.
    fn spawn_finalize(&mut self, run_id: uuid::Uuid, why: &'static str, in_flight: bool) {
        self.closed_runs.insert(run_id);
        if in_flight {
            self.draining_runs.insert(run_id);
        }
        let repo = self.repo.clone();
        let draining = self.draining_runs.clone();
        tokio::spawn(async move {
            match repo.finalize_run(run_id, "Stopped", !in_flight).await {
                Ok(RunFinalize::Deleted) => {
                    info!(run = %run_id, why, "engine sink: dropped empty run");
                }
                Ok(RunFinalize::Finalized { unsettled }) => {
                    // Still-open positions keep writing to this run, so its rollup
                    // is provisional until they settle — see `reroll_draining_run`.
                    if unsettled > 0 {
                        draining.insert(run_id);
                    }
                    info!(run = %run_id, why, unsettled, "engine sink: run finalized");
                }
                Err(e) => warn!(run = %run_id, why, "engine sink: finalize_run failed: {e}"),
            }
        });
    }

    /// Rebuild the re-roll set at boot: runs finalized before this process started
    /// whose positions had not all settled. Without it a restart mid-drain would
    /// freeze a finished run's metrics at the moment it was stopped. One indexed
    /// round trip returning a handful of ids — safe inside the boot gate.
    pub async fn load_draining_runs(&mut self) {
        match self.repo.draining_finalized_runs().await {
            Ok(ids) => {
                for id in ids {
                    self.draining_runs.insert(id);
                }
            }
            Err(e) => warn!("engine sink: load_draining_runs failed: {e}"),
        }
    }

    /// Consume one `PositionUpdate`: persist the transition to PG, keep the
    /// registry current, and push the SSE delta.
    ///
    /// Engine-final handlers return whether the registry row should be dropped
    /// (the engine no longer owns the position — for `ExitStuck`/`ExitUnconfirmed`
    /// the PG row stays OPEN and the reaper/manual actions own it from here). SSE
    /// is emitted **before** `registry.remove` so clients always get a real
    /// `position_id` (and entry price) on the final frame.
    pub async fn on_position_update(&mut self, delta: PositionDelta) {
        // Captured before the handlers run: a terminal transition drops the
        // registry row, and the re-roll below needs the run this position belonged
        // to (positions keep writing to the run they were born in).
        let owner = self.registry.get(delta.position).map(|m| (m.run_id, m.pg_id));
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
            PositionStatus::EntryFailed => self.on_entry_failed(&delta).await,
            PositionStatus::ExitStuck => self.on_exit_no_fill(&delta, "ExitStuck").await,
            PositionStatus::ExitUnconfirmed => {
                self.on_exit_no_fill(&delta, "ExitUnconfirmed").await
            }
        };
        self.emit_position_sse(&delta);
        if finalized {
            self.registry.remove(delta.position);
            self.manual_episodes.remove(&delta.rule);
            if let Some((run_id, pg_id)) = owner {
                self.reroll_draining_run(run_id, pg_id);
            }
            self.prune_finished_pg();
        }
    }

    /// A straggler of an already-finalized run just settled — re-roll that run's
    /// metrics so its numbers stop being the provisional snapshot taken when the
    /// rule was paused.
    ///
    /// No-op for a live run (its rollup happens at finalize) — the common path is
    /// a single `DashSet` miss. Chains onto this position's pending PG write so it
    /// reads the row *after* the terminal write commits, and is spawned so the
    /// decision loop never waits on it.
    fn reroll_draining_run(&mut self, run_id: uuid::Uuid, pg_id: uuid::Uuid) {
        if !self.draining_runs.contains(&run_id) {
            return;
        }
        let prev = self.pending_pg.remove(&pg_id);
        let repo = self.repo.clone();
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Err(e) = repo.roll_up_run(run_id).await {
                warn!(run = %run_id, "engine sink: run re-roll failed: {e}");
            }
        });
        self.pending_pg.insert(pg_id, handle);
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
        // A staged manual episode (fresh per-episode rule id, never in `rules`)
        // gets the fixed manual identity: real mode, the one manual run, and the
        // handler's pre-minted PG id.
        let staged = self.manual_pending.remove(&delta.rule);
        let info = match (self.rules.get(&delta.rule).cloned(), &staged) {
            (Some(info), _) => info,
            (None, Some(_)) => RuleInfo {
                mode: TradeMode::Real,
                name: MANUAL_STRATEGY_ID.to_string(),
                max_total: None,
                params: serde_json::json!({}),
            },
            (None, None) => {
                warn!(rule = %delta.rule.0, "engine sink: BuySubmitted for unknown rule — skipping");
                return;
            }
        };
        // Usually a run_cache hit after `warm_runs`; cold miss still awaits once.
        let run_id = if staged.is_some() {
            match self.ensure_manual_run().await {
                Ok(id) => id,
                Err(e) => {
                    warn!("engine sink: manual run creation failed: {e}");
                    return;
                }
            }
        } else {
            match self.ensure_run(delta.rule, &info).await {
                Ok(id) => id,
                Err(e) => {
                    warn!(rule = %delta.rule.0, "engine sink: run creation failed: {e}");
                    return;
                }
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
        if let Some(m) = &staged {
            // The handler already 202'd this id — the row must be born with it.
            pos.id = m.pg_id;
            pos.origin = MANUAL_STRATEGY_ID.to_string();
            pos.manual_exit = m.manual_exit.clone();
            self.manual_episodes.insert(delta.rule);
        }

        // Registry first so Pass-2 can spawn the buy immediately; PG insert is
        // backgrounded. Later transitions chain via `pending_pg`.
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
                sold_token_amount: 0,
                scale_stage: 0,
                token_account: None,
                entry_price: None,
                entry_sol: None,
                entry_time: None,
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
        self.pending_pg.insert(pg_id, handle);
        // Leave Waiting: Enter does not emit ArmedChanged(Disarmed). Clear the
        // armed registry (+ SSE) so snapshots cannot re-inject a stale Waiting row.
        self.leave_waiting(delta.rule.0, delta.mint.as_str());
    }

    async fn on_holding(&mut self, delta: &PositionDelta) {
        // Safety net if BuySubmitted skipped leave_waiting (unknown rule / crash recovery).
        self.leave_waiting(delta.rule.0, delta.mint.as_str());
        let Some(meta) = self.registry.get(delta.position) else { return };
        let Some(fill) = delta.fill else { return };
        self.retain_held_pool(&meta.mint, meta.trade_mode);

        // Mid-ladder partial fill: entry already recorded. Do NOT overwrite entry_*.
        if meta.entry_token_amount.is_some() {
            self.on_partial_fill(delta, &meta, fill).await;
            return;
        }

        // Fill signatures + token account stashed by the executor, keyed by intent.
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let entry_tx = fs.sigs.first().cloned().unwrap_or_default();
        let token_account = fs.token_account.clone();
        let paper_target = meta.paper_target.clone();

        // Registry first so Pass-2 / next Trade can size a sell without waiting on PG.
        self.registry.update(delta.position, |m| {
            m.entry_token_amount = Some(fill.token_amount);
            m.entry_price = Some(fill.price);
            m.entry_sol = Some(fill.sol);
            m.entry_time = Some(fill.at);
            m.sold_token_amount = 0;
            m.scale_stage = 0;
            m.paper_target = None;
            if token_account.is_some() {
                m.token_account = token_account.clone();
            }
        });

        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let pg_id = meta.pg_id;
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            // Paper: persist the trigger (`target_*`) before the worst-case entry fill
            // so the UI can show the modeled adverse-slippage gap.
            if let Some(target) = paper_target {
                if let Err(e) = repo
                    .record_target(
                        pg_id,
                        target.price,
                        target.token_amount,
                        target.time,
                        &target.tx,
                    )
                    .await
                {
                    warn!(pg = %pg_id, "engine sink: record_target failed: {e}");
                }
            }
            if let Err(e) = repo
                .record_entry_fill(
                    pg_id,
                    &entry_tx,
                    fill.token_amount,
                    fill.price,
                    fill.sol,
                    fill.at,
                    token_account.as_deref(),
                )
                .await
            {
                warn!(pg = %pg_id, "engine sink: record_entry_fill failed: {e}");
            }
        });
        self.pending_pg.insert(pg_id, handle);
    }

    /// Holding-preserving sell fill (scale-out leg): append ledger + bump aggregates;
    /// registry tracks sold so the next `SubmitSell` sizes the remainder correctly.
    async fn on_partial_fill(
        &mut self,
        delta: &PositionDelta,
        meta: &PositionMeta,
        fill: hunter_engine::event::Fill,
    ) {
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let reason = delta.reason.map(|r| r.label().into_owned());
        let stage = delta.stage;
        self.registry.update(delta.position, |m| {
            m.sold_token_amount = m.sold_token_amount.saturating_add(fill.token_amount);
            if let Some(s) = stage {
                m.scale_stage = s;
            }
            m.inflight_intent = None;
        });
        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let pg_id = meta.pg_id;
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Err(e) = repo
                .record_sell_fill(
                    pg_id,
                    fill.price,
                    fill.sol,
                    fill.token_amount,
                    fill.at,
                    reason.as_deref(),
                    stage,
                    &fs.sigs,
                    false,
                )
                .await
            {
                warn!(pg = %pg_id, "engine sink: record_sell_fill (partial) failed: {e}");
            }
        });
        self.pending_pg.insert(pg_id, handle);
    }

    /// Status-only transition (`ExitPending`): fire-and-forget PG so Pass-2
    /// `SubmitSell` is not gated on the write. Chains after any pending insert /
    /// Holding persist inside the spawn. Mid-sell crash recovery: ExitPending
    /// reaper re-drives when the row lands; externally-cleared Holding reconcile
    /// covers a landed sell whose status write never committed.
    async fn on_status_only(&mut self, delta: &PositionDelta, status: &str) {
        let Some(meta) = self.registry.get(delta.position) else { return };
        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let pg_id = meta.pg_id;
        let status = status.to_string();
        let reason = delta.reason.map(|r| r.label().into_owned());
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Err(e) = repo.mark_status(pg_id, &status, reason.as_deref()).await {
                warn!(pg = %pg_id, "engine sink: mark_status({status}) failed: {e}");
            }
        });
        self.pending_pg.insert(pg_id, handle);
    }

    /// Returns `true` when finalized — caller emits SSE then drops the registry row.
    ///
    /// The PG write is **chained-spawned**, not awaited: this runs on the one
    /// serialized decision loop, and awaiting a terminal write blocks it on PG
    /// (`await_pending_pg` + `record_sell_fill` + the real-mode held-pool check,
    /// three round trips). A Stop closes every position of a rule at once, so those
    /// serialize head-to-head while ingest is also writing — and a blocked loop
    /// folds *nothing* else: no ticks, no pings, no other fills. Per-position write
    /// ORDER is still guaranteed (the task
    /// awaits this row's previous write first); only the loop's wait is gone.
    async fn on_end(&mut self, delta: &PositionDelta) -> bool {
        let Some(meta) = self.registry.get(delta.position) else {
            return false;
        };
        let Some(fill) = delta.fill else {
            return false;
        };
        let fs = delta.intent.as_ref().and_then(|i| self.fill_sigs.take(i)).unwrap_or_default();
        let reason = delta
            .reason
            .map(|r| r.label().into_owned())
            .unwrap_or_else(|| "Metrics".to_string());
        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let release = self.held_pool_release(&meta);
        let pg_id = meta.pg_id;
        let stage = delta.stage;
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Err(e) = repo
                .record_sell_fill(
                    pg_id,
                    fill.price,
                    fill.sol,
                    fill.token_amount,
                    fill.at,
                    Some(&reason),
                    stage,
                    &fs.sigs,
                    true,
                )
                .await
            {
                warn!(pg = %pg_id, "engine sink: record_sell_fill (final) failed: {e}");
            }
            release.run().await;
        });
        self.pending_pg.insert(pg_id, handle);
        true
    }

    /// The buy never filled (entry exhausted / fatal) — terminal `EntryFailed`.
    /// Deliberately stamps NO hypothetical exit price (there was never a
    /// position; the row is excluded from PnL). Releases the SOL commitment so
    /// the unentered buy cannot strand the budget tracker.
    ///
    /// Returns `true` when finalized — caller emits SSE then drops the registry row.
    async fn on_entry_failed(&mut self, delta: &PositionDelta) -> bool {
        let Some(meta) = self.registry.get(delta.position) else {
            return false;
        };
        // Freeing the SOL commitment is in-memory and must not wait on PG — a
        // stop force-fails every EntryPending arm at once and each one holds
        // budget until this runs.
        if let Some(trader) = &self.trader {
            trader.release_sol_for_position(&meta.pg_id.to_string());
        }
        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let release = self.held_pool_release(&meta);
        let pg_id = meta.pg_id;
        let reason = delta.reason.map(|r| r.label().into_owned());
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                pos.mark_entry_failed();
                if let Some(r) = reason {
                    pos.exit_reason = Some(r);
                }
                if let Err(e) = repo.update_position(&pos).await {
                    warn!(pg = %pg_id, "engine sink: EntryFailed update failed: {e}");
                }
            }
            release.run().await;
        });
        self.pending_pg.insert(pg_id, handle);
        true
    }

    /// An exit that ended without a confirmed fill (ExitStuck / ExitUnconfirmed):
    /// the engine drops the arm (returns `true` → registry row removed) but the PG
    /// row stays **open** — no exit price is stamped (nothing confirmed sold; the
    /// row keeps marking to market) and the reaper / manual actions own it from
    /// here. Releases any SOL commitment (idempotent).
    async fn on_exit_no_fill(&mut self, delta: &PositionDelta, status: &str) -> bool {
        let Some(meta) = self.registry.get(delta.position) else {
            return false;
        };
        if let Some(trader) = &self.trader {
            trader.release_sol_for_position(&meta.pg_id.to_string());
        }
        // Sells the executor actually submitted before giving up. Without these the
        // row lands in the attention lane with `exit_tx_signatures = []`, and neither
        // a manual Verify nor the reaper can tell "landed late" from "never landed".
        let submitted_sells = delta
            .intent
            .as_ref()
            .and_then(|i| self.fill_sigs.take(i))
            .map(|fs| fs.sigs)
            .unwrap_or_default();

        let prev = self.pending_pg.remove(&meta.pg_id);
        let repo = self.repo.clone();
        let release = self.held_pool_release(&meta);
        let pg_id = meta.pg_id;
        let status = status.to_string();
        let reason = delta.reason.map(|r| r.label().into_owned());
        let handle = tokio::spawn(async move {
            if let Some(h) = prev {
                let _ = h.await;
            }
            if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                match status.as_str() {
                    "ExitUnconfirmed" => pos.mark_exit_unconfirmed(),
                    _ => pos.mark_exit_stuck(),
                }
                if let Some(r) = reason {
                    pos.exit_reason = Some(r);
                }
                pos.add_submitted_exit_sigs(submitted_sells);
                if let Err(e) = repo.update_position(&pos).await {
                    warn!(pg = %pg_id, "engine sink: {status} update failed: {e}");
                }
            }
            release.run().await;
        });
        self.pending_pg.insert(pg_id, handle);
        true
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Drop the `pending_pg` entries whose task already finished.
    ///
    /// A terminal transition parks its write handle under a `pg_id` that will
    /// never be looked up again (the position leaves the registry), so without
    /// this the map would grow by one entry per closed position for the life of
    /// the process. Called only on finalize — the only place entries become
    /// garbage — and `is_finished` never blocks.
    fn prune_finished_pg(&mut self) {
        self.pending_pg.retain(|_, h| !h.is_finished());
    }

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

    /// The held-pool release for a finalizing position, as an owned job the
    /// terminal write task runs after its PG write. Built here (needs `&self`)
    /// and executed there so its `has_other_open_position_on_mint` round trip
    /// stays off the decision loop, and so it observes the write that just landed.
    /// Paper (and a live bin with no gate) yields an inert job.
    fn held_pool_release(&self, meta: &PositionMeta) -> HeldPoolRelease {
        let gate = self
            .held_pools
            .clone()
            .filter(|_| meta.trade_mode == TradeMode::Real);
        HeldPoolRelease {
            gate,
            repo: self.repo.clone(),
            wallet: self.wallet.clone(),
            mint: meta.mint.clone(),
            exclude_id: meta.pg_id,
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

    /// The one manual-position run (`strategy_id='manual'`, `rule_id` NULL) —
    /// reused across restarts so the manual book never fragments across runs.
    async fn ensure_manual_run(&mut self) -> anyhow::Result<uuid::Uuid> {
        if let Some(id) = self.manual_run {
            return Ok(id);
        }
        let id = self.repo.ensure_manual_run().await?;
        self.manual_run = Some(id);
        Ok(id)
    }

    async fn ensure_run(&mut self, rule: RuleId, info: &RuleInfo) -> anyhow::Result<uuid::Uuid> {
        // Mode-checked, not just rule-checked: a cache entry opened for the other
        // trade_mode must never be handed out (see `CachedRun`). `close_stale_runs`
        // normally evicts it on the reload that changed the mode — this is the
        // hot-path guarantee that a buy racing that reload cannot file itself into
        // the wrong mode's run.
        if let Some(cached) = self.run_cache.get(&rule) {
            if cached.mode == info.mode {
                return Ok(cached.id);
            }
            self.run_cache.remove(&rule);
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
        // latest was finalized (Stopped/Finished/Cancelled). `closed_runs` covers
        // the in-flight case: this process closed the run on a pause and the
        // background `Stopped` write may not have committed yet, so the row can
        // still read `Running` — re-adopting it is exactly the bug the run
        // lifecycle exists to fix.
        if let Some(existing) = self.repo.latest_run(rule.0, mode).await? {
            if existing.status == "Running" && !self.closed_runs.contains(&existing.id) {
                self.run_cache.insert(rule, CachedRun { mode: info.mode, id: existing.id });
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
        self.run_cache.insert(rule, CachedRun { mode: info.mode, id: run.id });
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
        // Registry is updated before SSE on Holding (entry or partial); fall back to
        // the fill price only on a first entry if meta somehow lagged.
        // The entry fill is only in `delta` on the first-entry Holding frame, so the
        // registry is the source and the fill is the lag fallback — all three fields
        // resolve off the same predicate so they can never disagree.
        let entry_fill = delta
            .fill
            .filter(|_| delta.status == PositionStatus::Holding && delta.reason.is_none());
        let entry_price = meta.entry_price.or_else(|| entry_fill.map(|f| f.price));
        let entry_sol = meta.entry_sol.or_else(|| entry_fill.map(|f| f.sol));
        let entry_time = meta.entry_time.or_else(|| entry_fill.map(|f| f.at));
        let exit_price = delta
            .fill
            .filter(|_| delta.status == PositionStatus::End)
            .map(|f| f.price);
        let rule_name = self
            .rules
            .get(&delta.rule)
            .map(|i| i.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                self.manual_episodes
                    .contains(&delta.rule)
                    .then(|| MANUAL_STRATEGY_ID.to_string())
            });
        // Mid-ladder banked fraction for the FE chip (phase 5); omitted when zero.
        let sold_token_amount = (meta.sold_token_amount > 0).then_some(meta.sold_token_amount);
        let sold_bps = if meta.sold_token_amount > 0 {
            let entry = meta.entry_token_amount.unwrap_or(0);
            if entry == 0 {
                None
            } else {
                Some(
                    ((u128::from(meta.sold_token_amount) * 10_000) / u128::from(entry)).min(10_000)
                        as u16,
                )
            }
        } else {
            None
        };
        let scale_stage = (meta.scale_stage > 0 || meta.sold_token_amount > 0)
            .then_some(meta.scale_stage);
        let _ = self.sse_tx.send(SseEvent::StrategyPositionUpdate {
            rule_id: delta.rule.0,
            mint_address: delta.mint.to_string(),
            position_id: meta.pg_id,
            status: position_status_str(delta.status).to_string(),
            exit_reason: delta.reason.map(|r| r.label().into_owned()),
            entry_price,
            entry_sol,
            entry_time,
            exit_price,
            trade_mode,
            rule_name,
            needs_review: None,
            sold_token_amount,
            sold_bps,
            scale_stage,
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
        PositionStatus::EntryFailed => "EntryFailed",
        PositionStatus::ExitStuck => "ExitStuck",
        PositionStatus::ExitUnconfirmed => "ExitUnconfirmed",
    }
}

fn disarm_reason_str(r: DisarmReason) -> &'static str {
    match r {
        DisarmReason::Dead => "dead",
        DisarmReason::Migrated => "migrated",
        DisarmReason::Unsatisfiable => "unsatisfiable",
        DisarmReason::Paused => "paused",
        DisarmReason::DuplicateIdentity => "duplicate_identity",
    }
}

#[cfg(test)]
mod status_partition_guard {
    //! SSOT guard for the position-status wire contract. The wire strings AND the
    //! open/terminal partition below MUST mirror the frontend
    //! `OPEN_STATUSES` / `TERMINAL_STATUSES` in
    //! `frontend/src/live/slices/liveStatusSlice.ts` (and its vitest guard).
    //!
    //! Adding a `PositionStatus` variant breaks the exhaustive `match` in
    //! [`classify`] → this file stops compiling. When you fix it: (1) map the wire
    //! string in [`position_status_str`], (2) classify it open/terminal here, and
    //! (3) add it to the matching frontend set — a wire status in NEITHER frontend
    //! set is silently `delete`d from Live Status with no close recorded.
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Class {
        /// Snapshot-open partition (`find_open_positions`: NOT IN End/EntryFailed).
        /// A row with SOL possibly still in it (`ExitStuck`/`ExitUnconfirmed`) is
        /// OPEN — it must never land in a closed/actionless table.
        Open,
        /// Closed partition (`find_recent_closed`: status IN End/EntryFailed).
        Terminal,
    }

    /// Exhaustive over `PositionStatus` — the compile-time forcing function.
    fn classify(s: PositionStatus) -> (&'static str, Class) {
        match s {
            PositionStatus::BuySubmitted => ("BuySubmitted", Class::Open),
            PositionStatus::Holding => ("Holding", Class::Open),
            PositionStatus::ExitPending => ("ExitPending", Class::Open),
            PositionStatus::ExitStuck => ("ExitStuck", Class::Open),
            PositionStatus::ExitUnconfirmed => ("ExitUnconfirmed", Class::Open),
            PositionStatus::End => ("End", Class::Terminal),
            PositionStatus::EntryFailed => ("EntryFailed", Class::Terminal),
        }
    }

    const ALL: [PositionStatus; 7] = [
        PositionStatus::BuySubmitted,
        PositionStatus::Holding,
        PositionStatus::ExitPending,
        PositionStatus::ExitStuck,
        PositionStatus::ExitUnconfirmed,
        PositionStatus::End,
        PositionStatus::EntryFailed,
    ];

    #[test]
    fn classify_wire_string_matches_emitter() {
        for s in ALL {
            assert_eq!(classify(s).0, position_status_str(s), "wire string drift for {s:?}");
        }
    }

    #[test]
    fn only_end_and_entry_failed_are_terminal() {
        for s in ALL {
            let expected_terminal = matches!(s, PositionStatus::End | PositionStatus::EntryFailed);
            let is_terminal = classify(s).1 == Class::Terminal;
            assert_eq!(is_terminal, expected_terminal, "partition drift for {s:?}");
        }
    }

    /// The Stop action's watch partition, tied to the emitter that feeds it.
    ///
    /// A stop waits on exactly the statuses this sink will emit *again* for.
    /// `ExitStuck`/`ExitUnconfirmed` are `Open` above (the row keeps its SOL and
    /// its attention-lane actions) but engine-**terminal**: the arm is dropped and
    /// no further `strategy_position_update` ever fires. A new variant that lands
    /// on the wrong side here re-creates the forever-spinning "Stopping x/N".
    #[test]
    fn stop_watch_set_is_exactly_the_statuses_the_sink_still_emits_for() {
        use crate::api::handlers::strategies::action_progress::stop_in_flight;
        for s in ALL {
            let engine_still_owns = matches!(
                s,
                PositionStatus::BuySubmitted
                    | PositionStatus::Holding
                    | PositionStatus::ExitPending
            );
            assert_eq!(
                stop_in_flight(position_status_str(s)),
                engine_still_owns,
                "stop watch partition drift for {s:?}"
            );
        }
    }
}
