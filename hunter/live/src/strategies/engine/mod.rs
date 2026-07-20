//! Live adapters around the pure `hunter_engine` fold (strategy redesign, Phase 4).
//!
//! The engine (`hunter_engine::reduce`) is a pure `reduce(&mut EngineState, Event)
//! -> effects` fold — no clock, no I/O, no randomness. This module is the *live*
//! composition root that wraps it: it **produces** events (ingest pings + a 500 ms
//! tick + confirmed fills) and **consumes** effects (submit buy/sell on-chain or in
//! paper, persist positions to PG, push SSE). All decision logic lives in the
//! engine; these files are adapters and side-effects only.
//!
//! Module map (plan §3, `hunter/live/src/strategies/`):
//! * [`decision_loop`] — THE one serialized decision loop (`select!`); every
//!   `reduce` call happens here (plan 4.1). Replaces `runner.rs` + `service.rs`.
//! * [`producers`] — ingest `StrategyPing` + `TokenCache` → `Event`s, first-slot
//!   settlement detection, the live freshness gate (plan 4.2).
//! * [`exec_real`] — `SubmitBuy`/`SubmitSell` → executor submit-and-return; fills
//!   confirmed from the trades feed + a confirmation watchdog (plan 4.3).
//! * [`exec_paper`] — worst-case paper fill model → `FillConfirmed` (plan 4.4).
//! * [`sinks`] — `PositionUpdate` → PG writer, `ArmedChanged` → SSE (plan 4.5).
//! * [`event_log`] — event recorder + boot-recovery replay (plan 4.6).
//! * [`convert`] — DB model ↔ engine type converters (no conversion existed).

pub mod convert;
pub mod decision_loop;
pub mod event_log;
pub mod exec_paper;
pub mod exec_real;
pub mod producers;
pub mod reapers;
pub mod sinks;

use std::sync::Arc;

use dashmap::DashSet;
use tokio::sync::mpsc;
use uuid::Uuid;

use hunter_engine::event::{Fill, IntentId, PositionId, RuleId, TradeMode};

pub use decision_loop::{spawn_engine, EngineDeps, EngineHandles};

/// RAII interlock claiming a PG position id for an in-flight entry or exit task.
/// Recovery reapers skip ids present in these sets so they never race a live task.
#[derive(Debug, Clone, Default)]
pub struct InFlightGuards {
    entry: Arc<DashSet<Uuid>>,
    exit: Arc<DashSet<Uuid>>,
}

impl InFlightGuards {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `pg_id` for an entry task. Returns `None` if already claimed.
    pub fn try_begin_entry(&self, pg_id: Uuid) -> Option<EntryGuard> {
        if self.entry.insert(pg_id) {
            Some(EntryGuard { set: self.entry.clone(), pg_id })
        } else {
            None
        }
    }

    /// Claim `pg_id` for an exit task. Returns `None` if already claimed.
    pub fn try_begin_exit(&self, pg_id: Uuid) -> Option<ExitGuard> {
        if self.exit.insert(pg_id) {
            Some(ExitGuard { set: self.exit.clone(), pg_id })
        } else {
            None
        }
    }

    pub fn entry_held(&self, pg_id: Uuid) -> bool {
        self.entry.contains(&pg_id)
    }

    pub fn exit_held(&self, pg_id: Uuid) -> bool {
        self.exit.contains(&pg_id)
    }
}

/// Held for the lifetime of a real buy task — Drop frees the slot (panic-safe).
pub struct EntryGuard {
    set: Arc<DashSet<Uuid>>,
    pg_id: Uuid,
}

impl Drop for EntryGuard {
    fn drop(&mut self) {
        self.set.remove(&self.pg_id);
    }
}

/// Held for the lifetime of a real sell task — Drop frees the slot (panic-safe).
pub struct ExitGuard {
    set: Arc<DashSet<Uuid>>,
    pg_id: Uuid,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.set.remove(&self.pg_id);
    }
}

/// A command into the serialized decision loop from *outside* the ingest / tick /
/// fill paths — i.e. from HTTP handlers (rule CRUD reloads, manual position
/// closes). Kept small and `Send` so the loop's `select!` can own it.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// The active rule set changed (create/update/delete/activate/pause) — the loop
    /// reloads rules+fingerprints from PG and folds a `RulesReloaded` event.
    ReloadRules,
    /// A manual "Sell ALL" / stop-all targeting one **PG** position id. The loop
    /// resolves it to the engine `PositionId` via the sink registry, then folds a
    /// `ManualClose` (a no-op if the position isn't a live engine-held one).
    ManualClose { pg_position_id: Uuid },
    /// Force-close every open position of one **rule** (the per-row Stop). The loop
    /// resolves the rule's live engine positions via the registry and folds a
    /// `ManualClose` for each (a no-op for any not currently Holding).
    CloseRule { rule_id: Uuid },
    /// Force-close every open position of one **trade mode** (Stop All). Same as
    /// [`Self::CloseRule`] applied to every position matching `real`.
    CloseMode { real: bool },
    /// Book one **PG** position closed after its bag was cleared off-chain (an
    /// external / manual wallet sell) — the loop resolves it via the sink registry
    /// and folds an [`Event::ExternallyCleared`], which closes the position at `fill`
    /// WITHOUT emitting a sell (a no-op if the position isn't a live engine-held one).
    ReconcileCleared { pg_position_id: Uuid, fill: Fill },
}

/// A cheap, cloneable handle the HTTP layer holds to talk to the running engine
/// loop. All it can do is enqueue [`EngineCommand`]s — the loop owns all state.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    cmd_tx: mpsc::Sender<EngineCommand>,
}

impl EngineHandle {
    pub fn new(cmd_tx: mpsc::Sender<EngineCommand>) -> Self {
        Self { cmd_tx }
    }

    /// Ask the loop to reload rules from PG. Best-effort: a full queue (loop busy)
    /// drops the request — the next reload subsumes it.
    pub async fn reload_rules(&self) {
        let _ = self.cmd_tx.send(EngineCommand::ReloadRules).await;
    }

    /// Ask the loop to close a PG position id manually. Returns `false` only if the
    /// loop channel is closed (shutting down).
    pub async fn manual_close(&self, pg_position_id: Uuid) -> bool {
        self.cmd_tx
            .send(EngineCommand::ManualClose { pg_position_id })
            .await
            .is_ok()
    }

    /// Ask the loop to force-close every open position of `rule_id` (per-row Stop).
    /// Returns `false` only if the loop channel is closed (shutting down).
    pub async fn close_rule(&self, rule_id: Uuid) -> bool {
        self.cmd_tx
            .send(EngineCommand::CloseRule { rule_id })
            .await
            .is_ok()
    }

    /// Ask the loop to force-close every open position of one trade mode (Stop All).
    /// Returns `false` only if the loop channel is closed (shutting down).
    pub async fn close_mode(&self, real: bool) -> bool {
        self.cmd_tx
            .send(EngineCommand::CloseMode { real })
            .await
            .is_ok()
    }

    /// Ask the loop to book a PG position closed at `fill` after its bag was cleared
    /// off-chain (manual wallet sell) — no on-chain sell is fired. Returns `false`
    /// only if the loop channel is closed (shutting down).
    pub async fn reconcile_cleared(&self, pg_position_id: Uuid, fill: Fill) -> bool {
        self.cmd_tx
            .send(EngineCommand::ReconcileCleared { pg_position_id, fill })
            .await
            .is_ok()
    }
}

/// Per-position metadata the sink owns and the executor reads — the bridge between
/// the engine's opaque [`PositionId`] and the concrete PG row + on-chain facts a
/// sell needs (mint, held token amount, token account). Populated by the sink on
/// `BuySubmitted` (identity) and enriched on `Holding` (fill economics).
#[derive(Debug, Clone)]
pub struct PositionMeta {
    /// `strategy_positions.id` (the durable row).
    pub pg_id: Uuid,
    /// Owning run row (`strategy_runs.id`) — lazily created per rule.
    pub run_id: Uuid,
    pub rule_id: RuleId,
    pub mint: String,
    pub trade_mode: TradeMode,
    pub token_program_id: Option<String>,
    pub creator: Option<String>,
    /// Held raw token units, set from the entry fill (needed to size the sell).
    pub entry_token_amount: Option<u64>,
    /// Persisted wallet token account for the mint (set after entry fill).
    pub token_account: Option<String>,
    pub entry_price: Option<f64>,
    /// Paper-only: trigger-trade snapshot for `target_*` (armed signal, distinct
    /// from the worst-case entry fill). Consumed by the sink on `Holding`.
    pub paper_target: Option<PaperTarget>,
    pub cashback_enabled: bool,
    /// The intent currently in flight for this position (entry or exit). The exit
    /// reaper uses it to emit `FillFailed` back into the engine when a sell task
    /// dies but the process is still up.
    pub inflight_intent: Option<IntentId>,
}

/// Trigger-trade snapshot the paper fill model arms from (→ DB `target_*`).
#[derive(Debug, Clone)]
pub struct PaperTarget {
    pub price: f64,
    pub token_amount: u64,
    pub time: chrono::DateTime<chrono::Utc>,
    pub tx: String,
}

/// Shared engine-position ↔ PG-row registry. The sink writes it; the executor and
/// the manual-close path read it. `by_id` is the forward map; `by_pg` resolves a
/// PG uuid back to the engine id for manual close.
#[derive(Debug, Clone, Default)]
pub struct PositionRegistry {
    by_id: Arc<dashmap::DashMap<PositionId, PositionMeta>>,
    by_pg: Arc<dashmap::DashMap<Uuid, PositionId>>,
}

impl PositionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert/replace a position's metadata (called on `BuySubmitted`).
    pub fn upsert(&self, id: PositionId, meta: PositionMeta) {
        self.by_pg.insert(meta.pg_id, id);
        self.by_id.insert(id, meta);
    }

    /// Mutate an existing entry in place (called on `Holding` to add fill economics).
    pub fn update<F: FnOnce(&mut PositionMeta)>(&self, id: PositionId, f: F) {
        if let Some(mut m) = self.by_id.get_mut(&id) {
            f(m.value_mut());
        }
    }

    pub fn get(&self, id: PositionId) -> Option<PositionMeta> {
        self.by_id.get(&id).map(|m| m.value().clone())
    }

    /// Resolve a PG uuid to the live engine position id (manual close).
    pub fn engine_id(&self, pg_id: Uuid) -> Option<PositionId> {
        self.by_pg.get(&pg_id).map(|e| *e.value())
    }

    /// Every live engine position id currently held under `rule_id` (per-row Stop).
    pub fn positions_for_rule(&self, rule_id: RuleId) -> Vec<PositionId> {
        self.by_id
            .iter()
            .filter(|e| e.value().rule_id == rule_id)
            .map(|e| *e.key())
            .collect()
    }

    /// Every live engine position id currently held in `mode` (Stop All).
    pub fn positions_for_mode(&self, mode: TradeMode) -> Vec<PositionId> {
        self.by_id
            .iter()
            .filter(|e| e.value().trade_mode == mode)
            .map(|e| *e.key())
            .collect()
    }

    /// Whether any live engine position holds this mint — the token-cache eviction
    /// consults this so an engine-held token is never evicted (its price feed must
    /// stay live for the exit). Scans the (cap-bounded) held set.
    pub fn is_mint_held(&self, mint: &str) -> bool {
        self.by_id.iter().any(|e| e.value().mint == mint)
    }

    /// Drop a closed position (called on a terminal `PositionUpdate`).
    pub fn remove(&self, id: PositionId) {
        if let Some((_, meta)) = self.by_id.remove(&id) {
            self.by_pg.remove(&meta.pg_id);
        }
    }
}

/// A live "armed" snapshot entry for the monitor endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArmedEntry {
    pub rule_id: Uuid,
    pub mint_address: String,
    /// `"armed"` while watching for entry; a disarm removes the entry.
    pub state: String,
}

/// Shared snapshot of currently-armed (token, rule) pairs — the sink writes it on
/// every `ArmedChanged`, `GET /api/strategies/armed` reads it. Keyed by
/// `(rule_id, mint)`; a disarm removes the key (the SSE delta carries the reason).
#[derive(Debug, Clone, Default)]
pub struct ArmedRegistry(Arc<dashmap::DashMap<(Uuid, String), ArmedEntry>>);

impl ArmedRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_armed(&self, rule_id: Uuid, mint: String) {
        self.0.insert(
            (rule_id, mint.clone()),
            ArmedEntry { rule_id, mint_address: mint, state: "armed".to_string() },
        );
    }

    pub fn clear(&self, rule_id: Uuid, mint: &str) {
        self.0.remove(&(rule_id, mint.to_string()));
    }

    /// A cloned snapshot of all currently-armed entries (for the HTTP endpoint).
    pub fn snapshot(&self) -> Vec<ArmedEntry> {
        self.0.iter().map(|e| e.value().clone()).collect()
    }
}

/// Confirmed-fill signatures + token account, stashed by the executor (which knows
/// them) keyed by the fill's `IntentId`, for the sink to persist onto the PG row.
/// The engine's pure `Fill` deliberately carries no signature, so this side-channel
/// carries the on-chain identity the durable row needs without touching the engine.
#[derive(Debug, Clone, Default)]
pub struct FillSigs {
    pub sigs: Vec<String>,
    pub token_account: Option<String>,
}

/// Shared intent → [`FillSigs`] store (executor writes, sink reads-and-clears).
#[derive(Debug, Clone, Default)]
pub struct FillSigStore(Arc<dashmap::DashMap<IntentId, FillSigs>>);

impl FillSigStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the on-chain identity of a confirmed fill for `intent`.
    pub fn put(&self, intent: IntentId, sigs: FillSigs) {
        self.0.insert(intent, sigs);
    }

    /// Take (and remove) the fill identity for `intent`, if the executor stashed it.
    pub fn take(&self, intent: &IntentId) -> Option<FillSigs> {
        self.0.remove(intent).map(|(_, v)| v)
    }
}
