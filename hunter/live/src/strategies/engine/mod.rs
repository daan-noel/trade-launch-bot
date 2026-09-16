//! Live adapters around the pure `hunter_engine` fold.
//!
//! The engine (`hunter_engine::reduce`) is a pure `reduce(&mut EngineState, Event)
//! -> effects` fold — no clock, no I/O, no randomness. This module is the *live*
//! composition root that wraps it: it **produces** events (ingest pings + a
//! `TICK_MS` clock tick + confirmed fills) and **consumes** effects (submit
//! buy/sell on-chain or in paper, persist positions to PG, push SSE). All
//! decision logic lives in the engine; these files are adapters and side-effects only.
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
//! * [`run_config`] — the diffable digest of what a run is running under, so a rule
//!   edited mid-run is stamped on the run instead of silently re-defining it.
//! * [`arm_ledger`] — `ArmedChanged` → batched `strategy_arms` rows, off the hot
//!   path (`docs/plans/strategies/arm-ledger.md`).
//! * [`event_log`] — event recorder + boot-recovery replay (plan 4.6).
//! * [`convert`] — DB model ↔ engine type converters (no conversion existed).

pub mod arm_ledger;
pub mod boot;
pub mod convert;
pub mod decision_loop;
pub mod event_log;
pub mod exec_paper;
pub mod exec_real;
pub mod hydrate;
pub mod orphan_exit;
pub mod producers;
pub mod reapers;
pub mod breadth_refresh;
pub mod door_refresh;
pub mod reload_scheduler;
pub mod run_config;
pub mod sell_backfill;
pub mod sinks;

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashSet;
use tokio::sync::{broadcast, mpsc, oneshot};
use trading_core::models::ingest::SseEvent;
use uuid::Uuid;

use hunter_engine::event::{Fill, IntentId, ManualExit, PositionId, RuleId, TradeMode};
use hunter_engine::readout::RuleReadout;

pub use decision_loop::{spawn_engine, EngineDeps, EngineHandles};

/// How long a reload ack waits on the decision loop. Must cover PG load +
/// `warm_runs` on the 2vCPU EC2 box under ingest load. HTTP rule mutations
/// schedule reloads in the background instead of blocking on this.
pub(crate) const RELOAD_ACK_TIMEOUT: Duration = Duration::from_secs(30);
/// Admin cache reseed waits longer — PG adopt + rule reload under load.
pub(crate) const RESEED_ACK_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a rule readout waits on the loop. Short on purpose: it is a UI poll, so
/// a loop busy enough to miss this window has better things to do than answer it,
/// and the caller degrades to "unavailable" rather than queueing.
pub(crate) const READ_RULE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// One FIFO exit lock per mint, present only while a guard or a waiter holds it.
type ExitMintLocks = Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>;

/// RAII interlock claiming a PG position id (and optionally a mint) for an
/// in-flight entry or exit task. Recovery reapers skip ids/mints present in these
/// sets so they never race a live task. The mint lock serializes exits that share
/// one ATA so sibling positions don't fan out parallel sell RPCs.
#[derive(Debug, Clone, Default)]
pub struct InFlightGuards {
    entry: Arc<DashSet<Uuid>>,
    exit: Arc<DashSet<Uuid>>,
    /// Mints with an exit in flight or queued (shared-ATA coordination).
    exit_mints: ExitMintLocks,
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

    /// The mint's lock, created on first use. The map guard drops inside this call,
    /// so no caller holds it across an await.
    fn exit_mint_lock(&self, mint: &str) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(
            self.exit_mints
                .entry(mint.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .value(),
        )
    }

    fn exit_mint_guard(&self, mint: &str, guard: tokio::sync::OwnedMutexGuard<()>) -> ExitMintGuard {
        ExitMintGuard { guard: Some(guard), locks: self.exit_mints.clone(), mint: mint.to_string() }
    }

    /// Claim `mint` for an exit task (one sell per mint at a time). Returns `None`
    /// if another exit already owns the mint.
    pub fn try_begin_exit_mint(&self, mint: &str) -> Option<ExitMintGuard> {
        let guard = self.exit_mint_lock(mint).try_lock_owned().ok()?;
        Some(self.exit_mint_guard(mint, guard))
    }

    /// Claim `mint`, queueing behind the exit that owns it (FIFO). `None` when it
    /// is still owned after `wait`.
    pub async fn begin_exit_mint(&self, mint: &str, wait: Duration) -> Option<ExitMintGuard> {
        let lock = self.exit_mint_lock(mint);
        match tokio::time::timeout(wait, lock.lock_owned()).await {
            Ok(guard) => Some(self.exit_mint_guard(mint, guard)),
            Err(_) => {
                prune_exit_mint(&self.exit_mints, mint);
                None
            }
        }
    }

    pub fn entry_held(&self, pg_id: Uuid) -> bool {
        self.entry.contains(&pg_id)
    }

    pub fn exit_held(&self, pg_id: Uuid) -> bool {
        self.exit.contains(&pg_id)
    }

    pub fn exit_mint_held(&self, mint: &str) -> bool {
        self.exit_mints.get(mint).is_some_and(|lock| lock.try_lock().is_err())
    }
}

/// Drop `mint`'s lock once only the map references it: no guard, no waiter.
/// `remove_if` decides under the shard lock that `exit_mint_lock` clones under,
/// so a waiter that already holds a clone keeps the entry alive.
fn prune_exit_mint(locks: &ExitMintLocks, mint: &str) {
    locks.remove_if(mint, |_, lock| Arc::strong_count(lock) == 1);
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

/// Held for the lifetime of a mint-scoped exit — Drop frees the mint (panic-safe)
/// and hands it to the next queued exit.
pub struct ExitMintGuard {
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
    locks: ExitMintLocks,
    mint: String,
}

impl Drop for ExitMintGuard {
    fn drop(&mut self) {
        // Release before pruning, so the prune sees this guard's reference gone.
        drop(self.guard.take());
        prune_exit_mint(&self.locks, &self.mint);
    }
}

/// A command into the serialized decision loop from *outside* the ingest / tick /
/// fill paths — i.e. from HTTP handlers (rule CRUD reloads, manual position
/// closes). Kept small and `Send` so the loop's `select!` can own it.
#[derive(Debug)]
pub enum EngineCommand {
    /// The active rule set changed (create/update/delete/activate/pause) — the loop
    /// reloads rules+fingerprints from PG and folds a `RulesReloaded` event.
    ReloadRules {
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// The UTC day rolled over (or the process booted): the loop recomputes today's
    /// launch-build stats and folds a `LaunchBuildStatsReloaded`. Creation-time input
    /// only — nothing about a tracked token changes.
    ReloadLaunchBuildStats {
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// The UTC day rolled over: swap in the day's build-breadth table, computed off
    /// the loop by the refresh task. The loop folds a `BuildBreadthReloaded` and
    /// nothing else.
    SetBuildBreadth {
        breadth: std::sync::Arc<[hunter_engine::event::BuildBreadth]>,
    },
    /// A manual "Sell ALL" / "Sell N%" targeting one **PG** position id. The loop
    /// resolves it to the engine `PositionId` via the sink registry, then folds a
    /// `ManualClose` (a no-op if the position isn't a live engine-held one).
    /// `portion: All` = full close; `BpsOfInitial` = Console partial.
    ManualClose {
        pg_position_id: Uuid,
        portion: hunter_engine::event::Portion,
    },
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
    /// A Console manual buy: the loop mints a fresh per-episode rule id, stages
    /// the pre-minted PG row id + exit config with the sink, and folds a
    /// [`Event::ManualBuy`] — from there it is a bot buy (same retry / fill /
    /// reaper machinery). The handler already returned 202 `{position_id: pg_id}`.
    ManualBuy {
        pg_id: Uuid,
        mint: String,
        lamports: u64,
        exit: Option<ManualExit>,
    },
    /// Set / replace / clear a manual position's TP/SL post-entry — the loop
    /// resolves the engine position via the registry and folds a
    /// [`Event::SetManualExit`] (the handler persists the JSONB separately).
    SetManualExit { pg_position_id: Uuid, exit: Option<ManualExit> },
    /// Admin reseed: reload rules from PG, then adopt open positions + episode
    /// counters from PG (no event-log replay; does not clear armed state).
    ReseedFromDb {
        ack: oneshot::Sender<Result<EngineReseedReport, String>>,
    },
    /// **Read-only.** What the fold currently reads for one (token, rule) arm —
    /// each of the rule's conditions with its live value and whether it holds
    /// ([`hunter_engine::readout`]). Folds nothing and emits no effect.
    ///
    /// A command rather than a registry the loop publishes into: the readout is for
    /// a modal that is usually closed, and publishing it would allocate on the hot
    /// path for every tracked token to serve nobody. This costs the loop one map
    /// lookup and one condition walk, and only while someone is looking.
    ReadRule {
        mint: String,
        rule_id: RuleId,
        ack: oneshot::Sender<Option<RuleReadout>>,
    },
}

/// Outcome of [`EngineCommand::ReseedFromDb`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineReseedReport {
    pub rules: usize,
    pub holdings_adopted: u32,
    pub buy_submitted_adopted: u32,
    pub episodes_seeded: u32,
}

/// Why [`EngineHandle::reload_rules`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineReloadError {
    /// Decision loop channel closed (shutting down).
    ChannelClosed,
    /// Reload ack timed out waiting on the serialized loop.
    TimedOut,
    /// PG load or rule parse failed inside the loop.
    Failed(String),
}

impl std::fmt::Display for EngineReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelClosed => write!(f, "channel closed"),
            Self::TimedOut => write!(f, "reload ack timed out"),
            Self::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

/// A cheap, cloneable handle the HTTP layer holds to talk to the running engine
/// loop. All it can do is enqueue [`EngineCommand`]s — the loop owns all state.
#[derive(Clone)]
pub struct EngineHandle {
    cmd_tx: mpsc::Sender<EngineCommand>,
    reload_scheduler: Arc<reload_scheduler::EngineReloadScheduler>,
}

impl std::fmt::Debug for EngineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineHandle").field("cmd_tx", &self.cmd_tx).finish_non_exhaustive()
    }
}

impl EngineHandle {
    pub fn new(cmd_tx: mpsc::Sender<EngineCommand>) -> Self {
        Self {
            cmd_tx,
            reload_scheduler: Arc::new(reload_scheduler::EngineReloadScheduler::new()),
        }
    }

    /// After a rule/fingerprint PG write, reload the engine without blocking HTTP.
    pub fn schedule_reload(&self, sse_tx: broadcast::Sender<SseEvent>) {
        Arc::clone(&self.reload_scheduler).schedule(self.clone(), sse_tx);
    }

    /// Ask the loop to reload rules from PG and wait until the fold completes.
    /// Recompute + swap the launch-build door map. Same ack shape as
    /// [`reload_rules`](Self::reload_rules); the caller is the daily refresh task.
    pub async fn reload_launch_build_stats(&self) -> Result<(), EngineReloadError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ReloadLaunchBuildStats { ack: tx })
            .await
            .map_err(|_| EngineReloadError::ChannelClosed)?;
        match tokio::time::timeout(RELOAD_ACK_TIMEOUT, rx).await {
            Ok(Ok(result)) => result.map_err(EngineReloadError::Failed),
            Ok(Err(_)) => Err(EngineReloadError::ChannelClosed),
            Err(_) => Err(EngineReloadError::TimedOut),
        }
    }

    /// Hand the loop the day's build-breadth table (already computed by the caller,
    /// the daily refresh task). Fire-and-forget: the loop only swaps a map.
    pub async fn set_build_breadth(
        &self,
        breadth: std::sync::Arc<[hunter_engine::event::BuildBreadth]>,
    ) -> Result<(), EngineReloadError> {
        self.cmd_tx
            .send(EngineCommand::SetBuildBreadth { breadth })
            .await
            .map_err(|_| EngineReloadError::ChannelClosed)
    }

    pub async fn reload_rules(&self) -> Result<(), EngineReloadError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ReloadRules { ack: tx })
            .await
            .map_err(|_| EngineReloadError::ChannelClosed)?;
        match tokio::time::timeout(RELOAD_ACK_TIMEOUT, rx).await {
            Ok(Ok(result)) => result.map_err(EngineReloadError::Failed),
            Ok(Err(_)) => Err(EngineReloadError::ChannelClosed),
            Err(_) => Err(EngineReloadError::TimedOut),
        }
    }

    /// Read what the fold currently sees for one (token, rule) arm.
    ///
    /// `Ok(None)` is a legitimate answer, not a failure: the token is untracked, the
    /// rule has no arm on it, or the arm carries no rule (a tracked-only manual
    /// position). Only a closed or wedged loop is an `Err`.
    pub async fn read_rule(
        &self,
        mint: &str,
        rule_id: RuleId,
    ) -> Result<Option<RuleReadout>, EngineReloadError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ReadRule {
                mint: mint.to_string(),
                rule_id,
                ack: tx,
            })
            .await
            .map_err(|_| EngineReloadError::ChannelClosed)?;
        match tokio::time::timeout(READ_RULE_ACK_TIMEOUT, rx).await {
            Ok(Ok(readout)) => Ok(readout),
            Ok(Err(_)) => Err(EngineReloadError::ChannelClosed),
            Err(_) => Err(EngineReloadError::TimedOut),
        }
    }

    /// Reload rules + adopt PG position rows into the engine (admin reseed).
    pub async fn reseed_from_db(&self) -> Result<EngineReseedReport, EngineReloadError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ReseedFromDb { ack: tx })
            .await
            .map_err(|_| EngineReloadError::ChannelClosed)?;
        match tokio::time::timeout(RESEED_ACK_TIMEOUT, rx).await {
            Ok(Ok(result)) => result.map_err(EngineReloadError::Failed),
            Ok(Err(_)) => Err(EngineReloadError::ChannelClosed),
            Err(_) => Err(EngineReloadError::TimedOut),
        }
    }

    /// Ask the loop to close a PG position id manually. `portion` selects Sell ALL
    /// vs Sell N% (`BpsOfInitial`). Returns `false` only if the loop channel is
    /// closed (shutting down).
    pub async fn manual_close(
        &self,
        pg_position_id: Uuid,
        portion: hunter_engine::event::Portion,
    ) -> bool {
        self.cmd_tx
            .send(EngineCommand::ManualClose {
                pg_position_id,
                portion,
            })
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

    /// Inject a Console manual buy (the PG row is born as `pg_id`). Returns `false`
    /// only if the loop channel is closed (shutting down).
    pub async fn manual_buy(
        &self,
        pg_id: Uuid,
        mint: String,
        lamports: u64,
        exit: Option<ManualExit>,
    ) -> bool {
        self.cmd_tx
            .send(EngineCommand::ManualBuy { pg_id, mint, lamports, exit })
            .await
            .is_ok()
    }

    /// Set / replace / clear a manual position's TP/SL config. Returns `false`
    /// only if the loop channel is closed (shutting down).
    pub async fn set_manual_exit(&self, pg_position_id: Uuid, exit: Option<ManualExit>) -> bool {
        self.cmd_tx
            .send(EngineCommand::SetManualExit { pg_position_id, exit })
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
    /// Confirmed sell-leg raw token units so far (scale-out). Remainder =
    /// `entry_token_amount - sold_token_amount`.
    pub sold_token_amount: u64,
    /// Next scale-out stage index (mirrors PG `scale_stage`; 0 = legacy / pre-first).
    pub scale_stage: u8,
    /// Persisted wallet token account for the mint (set after entry fill).
    pub token_account: Option<String>,
    pub entry_price: Option<f64>,
    /// Entry-fill SOL spent and fill instant — set from the same entry fill as
    /// `entry_price`, so every position SSE frame (not just `Holding`) can carry
    /// the full entry snapshot the UI charts and sizes from.
    pub entry_sol: Option<f64>,
    pub entry_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Network fees (SOL) of this position's buys that landed and reverted — the
    /// in-memory mirror of PG `extra.reverted_fee_lamports` before the entry books
    /// it. The entry fill adds it to `entry_sol` and clears it, as
    /// `record_entry_fill` does.
    pub reverted_buy_fee_sol: f64,
    /// Paper-only: trigger-trade snapshot for `target_*` (armed signal, distinct
    /// from the worst-case entry fill). Consumed by the sink on `Holding`.
    pub target_snapshot: Option<TargetSnapshot>,
    pub cashback_enabled: bool,
    /// The intent currently in flight for this position (entry or exit). The exit
    /// reaper uses it to emit `FillFailed` back into the engine when a sell task
    /// dies but the process is still up.
    pub inflight_intent: Option<IntentId>,
}

/// Trigger-trade snapshot the paper fill model arms from (→ DB `target_*`).
#[derive(Debug, Clone)]
pub struct TargetSnapshot {
    pub price: f64,
    pub token_amount: u64,
    /// The trigger print itself: a real print off the feed in both modes, so its
    /// slot IS knowable (mig 0004 `target_slot`), its block time is `target_time`,
    /// and the sink resolves `target_tx` from it.
    pub print: PrintKey,
}

/// Identity of one feed print: the `trades` order key plus its block time. The
/// token cache holds no signatures (a 64-byte string per cached row is RAM the EC2
/// box does not have), so a fill or trigger priced off a cached print carries this
/// instead and the sink resolves the signature from `trades`, off the decision
/// loop (`TradeRepo::print_signature`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrintKey {
    pub slot: u64,
    pub tx_index: u32,
    pub leg_index: u32,
    pub block_time: chrono::DateTime<chrono::Utc>,
}

impl PrintKey {
    pub fn of(t: &trading_core::state::token_cache::CachedTrade) -> Self {
        Self { slot: t.slot, tx_index: t.tx_index, leg_index: t.leg_index, block_time: t.block_time }
    }
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

    /// The token account a live position on this mint already holds, ignoring
    /// `exclude_pg` (the buying position itself).
    ///
    /// The in-memory twin of `StrategyRepo::find_reusable_token_account`: both
    /// answer "does a non-terminal position on this (wallet, mint) already have an
    /// account to re-buy into", and this registry holds exactly those positions
    /// (boot adoption re-populates it across a restart). Reading it here keeps a PG
    /// round trip off the send path — that query runs on a re-buy into a mint we
    /// already hold, ~a quarter of real entries. The PG read stays as the fallback
    /// for a row this process does not hold.
    pub fn sibling_token_account(&self, mint: &str, exclude_pg: Uuid) -> Option<String> {
        self.by_id
            .iter()
            .find(|e| {
                let m = e.value();
                m.mint == mint && m.pg_id != exclude_pg && m.token_account.is_some()
            })
            .and_then(|e| e.value().token_account.clone())
    }

    /// Drop a closed position (called on a terminal `PositionUpdate`).
    pub fn remove(&self, id: PositionId) {
        if let Some((_, meta)) = self.by_id.remove(&id) {
            self.by_pg.remove(&meta.pg_id);
        }
    }

    /// Distinct rule ids with a live engine-held position (drain-set input).
    pub fn open_rule_ids(&self) -> std::collections::HashSet<RuleId> {
        self.by_id.iter().map(|e| e.value().rule_id).collect()
    }
}

/// A live "armed" snapshot entry for the monitor endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArmedEntry {
    pub rule_id: Uuid,
    pub mint_address: String,
    /// `"armed"` while watching for entry; a disarm removes the entry.
    pub state: String,
    /// When this arming episode began — the ledger's episode key, and the age the
    /// Waiting lane shows. Server-stamped, so a reconnecting client no longer
    /// restarts every row's clock at its own mount.
    pub armed_at: chrono::DateTime<chrono::Utc>,
}

/// Shared snapshot of currently-armed (token, rule) pairs — the sink writes it on
/// every `ArmedChanged`, `GET /api/strategies/armed` reads it. Keyed by
/// `(rule_id, mint)`; a disarm removes the key (the SSE delta carries the reason).
///
/// This is the SSOT for **what is armed right now**, and only that. The durable
/// history of arming episodes is `strategy_arms` (see
/// `docs/plans/strategies/arm-ledger.md`) — the two overlap on live episodes on
/// purpose, because this one must patch per-event with no round trip.
#[derive(Debug, Clone, Default)]
pub struct ArmedRegistry(Arc<dashmap::DashMap<(Uuid, String), ArmedEntry>>);

impl ArmedRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an arm and hand back its `armed_at`. Re-arming a pair that is
    /// already armed keeps the ORIGINAL instant: it is the same episode, and a
    /// refreshed stamp would both reset the visible age and orphan the ledger row
    /// the end write has to find.
    pub fn set_armed(&self, rule_id: Uuid, mint: String) -> (chrono::DateTime<chrono::Utc>, bool) {
        let key = (rule_id, mint.clone());
        if let Some(existing) = self.0.get(&key) {
            return (existing.armed_at, false);
        }
        let armed_at = chrono::Utc::now();
        self.0.insert(
            key,
            ArmedEntry {
                rule_id,
                mint_address: mint,
                state: "armed".to_string(),
                armed_at,
            },
        );
        (armed_at, true)
    }

    /// Drop the arm and hand back when it started, so the caller can close the
    /// ledger episode without a read. `None` = nothing was armed for this pair.
    pub fn clear(&self, rule_id: Uuid, mint: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.0.remove(&(rule_id, mint.to_string())).map(|(_, e)| e.armed_at)
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
    /// Slot the fill landed in (mig 0004 `entry_slot` / `exit_slot`). Rides this
    /// side-channel rather than `Event::FillConfirmed`'s `Fill` on purpose: the
    /// engine is the pure decision fold and no decision reads a slot, so putting
    /// it on the event would widen the kernel's input for a bookkeeping value.
    /// `None` for a paper fill — simulated, so it never lands in a slot.
    pub slot: Option<u64>,
    /// The print a PAPER fill was priced against (`None` for real: its `sigs` are
    /// its own). The sink resolves its signature into the same columns a real fill
    /// writes, which is what lets the chart and trades table find the paper fill.
    /// It is the print's slot, not ours, so it never feeds `slot` above.
    pub print: Option<PrintKey>,
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

/// Process-local write-ahead journal of submitted buy signatures, keyed by PG
/// position id. Set **synchronously** in the `on_signed` hook (before network
/// submit / before the bounded PG persist) so:
/// - a slow `mark_buy_submitted` never blocks the nonce slot on the hot path
/// - `adopt_existing_fill` can skip a PG round-trip on the first attempt (empty)
///   and still see prior sigs on same-process entry retries
///
/// Crash recovery still reads `strategy_positions.submitted_buy_signatures` via
/// the reaper — this journal is live-process only.
#[derive(Debug, Clone, Default)]
pub struct SubmittedBuyJournal(Arc<dashmap::DashMap<Uuid, JournalEntry>>);

/// One position's journaled buys: every signature, and when the first was signed.
#[derive(Debug, Clone)]
struct JournalEntry {
    first_signed_at: chrono::DateTime<chrono::Utc>,
    sigs: Vec<String>,
}

impl SubmittedBuyJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `sig` for `pg_id` (idempotent if the same sig is recorded twice).
    pub fn append(&self, pg_id: Uuid, sig: String) {
        self.0
            .entry(pg_id)
            .and_modify(|e| {
                if !e.sigs.iter().any(|s| s == &sig) {
                    e.sigs.push(sig.clone());
                }
            })
            .or_insert_with(|| JournalEntry { first_signed_at: chrono::Utc::now(), sigs: vec![sig] });
    }

    /// Snapshot of sigs known for `pg_id` in this process (empty ⇒ first attempt).
    pub fn sigs(&self, pg_id: Uuid) -> Vec<String> {
        self.0.get(&pg_id).map(|e| e.sigs.clone()).unwrap_or_default()
    }

    /// The sigs plus when the first was signed. Every sig is recorded before its
    /// send, so that instant precedes all of them: the lower bound for finding
    /// their legs on the feed.
    pub fn signed(&self, pg_id: Uuid) -> Option<(chrono::DateTime<chrono::Utc>, Vec<String>)> {
        self.0.get(&pg_id).map(|e| (e.first_signed_at, e.sigs.clone()))
    }

    /// Drop the journal entry once the position is terminal / no longer needed.
    pub fn clear(&self, pg_id: Uuid) {
        self.0.remove(&pg_id);
    }
}

#[cfg(test)]
mod exit_mint_lock_tests {
    use super::*;

    /// A sibling's exit queued on the mint sends the moment the holder finishes,
    /// not on the next reaper tick.
    #[tokio::test]
    async fn a_queued_exit_takes_the_mint_when_the_holder_finishes() {
        let guards = InFlightGuards::new();
        let held = guards.try_begin_exit_mint("M").expect("free mint");
        assert!(guards.exit_mint_held("M"));
        assert!(guards.try_begin_exit_mint("M").is_none(), "one exit per mint");
        let waiter = guards.clone();
        let queued = tokio::spawn(async move { waiter.begin_exit_mint("M", Duration::from_secs(5)).await.is_some() });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let released = tokio::time::Instant::now();
        drop(held);
        assert!(queued.await.expect("task"), "the queued exit gets the mint");
        assert!(released.elapsed() < Duration::from_millis(500));
        assert!(guards.exit_mints.is_empty(), "released with no waiter: pruned");
    }

    /// The wait is bounded; the holder keeps the mint when a waiter gives up.
    #[tokio::test]
    async fn a_queued_exit_gives_up_after_its_wait() {
        let guards = InFlightGuards::new();
        let held = guards.try_begin_exit_mint("M").expect("free mint");
        assert!(guards.begin_exit_mint("M", Duration::from_millis(50)).await.is_none());
        assert!(guards.exit_mint_held("M"), "the holder keeps the mint");
        drop(held);
        assert!(guards.exit_mints.is_empty(), "pruned once the holder releases");
        assert!(!guards.exit_mint_held("M"));
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn meta(pg_id: Uuid, mint: &str, token_account: Option<&str>) -> PositionMeta {
        PositionMeta {
            pg_id,
            run_id: Uuid::nil(),
            rule_id: RuleId(Uuid::nil()),
            mint: mint.to_string(),
            trade_mode: TradeMode::Real,
            token_program_id: None,
            creator: None,
            entry_token_amount: None,
            sold_token_amount: 0,
            scale_stage: 0,
            token_account: token_account.map(str::to_string),
            entry_price: None,
            entry_sol: None,
            entry_time: None,
            reverted_buy_fee_sol: 0.0,
            target_snapshot: None,
            cashback_enabled: false,
            inflight_intent: None,
        }
    }

    /// The send path reads this instead of PG on a re-buy into a held mint, so it
    /// must answer exactly what the PG query answers: an account held by ANOTHER
    /// live position on THIS mint. Returning the buyer's own row would hand the buy
    /// back the account it is trying to resolve, and matching another mint would
    /// buy into the wrong token's account.
    #[test]
    fn a_sibling_account_is_another_live_position_on_the_same_mint() {
        let reg = PositionRegistry::new();
        let (me, sibling, other_mint) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        reg.upsert(PositionId(1), meta(me, "MINT", None));
        assert_eq!(reg.sibling_token_account("MINT", me), None, "nothing else holds it yet");

        reg.upsert(PositionId(2), meta(sibling, "MINT", Some("ACCT")));
        assert_eq!(reg.sibling_token_account("MINT", me).as_deref(), Some("ACCT"));
        // The buyer never resolves to itself.
        assert_eq!(reg.sibling_token_account("MINT", sibling), None);
        // Another mint's account is never reused.
        reg.upsert(PositionId(3), meta(other_mint, "OTHER", Some("OTHER_ACCT")));
        assert_eq!(reg.sibling_token_account("MINT", sibling), None);

        // A settled sibling leaves the registry, so its account stops being offered.
        reg.remove(PositionId(2));
        assert_eq!(reg.sibling_token_account("MINT", me), None);
    }
}
