use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use sqlx::PgPool;
use tokio::sync::{broadcast, Semaphore};
use uuid::Uuid;

use super::exit::{CachedExitState, ExitWalkState};
use super::util::none_if_zero_u64;
use backend_core::models::ingest::SseEvent;
use crate::state::token_cache::CachedTrade;
use backend_core::models::{PaperRun, PaperRunStatus, Position, PositionStatus, Tpsl2Rule};
use backend_core::storage::repositories::{
    tpsl2_paper_trading_repo::Tpsl2PaperTradingRepo, tpsl2_position_repo::Tpsl2PositionRepo,
    tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo,
};

/// Pointer to a paper rule's current run — the run new paper positions are
/// stamped with and the run the result view surfaces.
#[derive(Clone, Copy)]
pub struct PaperRunRef {
    pub run_id: Uuid,
}

/// Per-rule realized-performance counters, accumulated live on each position
/// close (see [`Tpsl2RuntimeCache::sync_position`]) and warmed from the DB on
/// boot. All-time for real rules; current-run for paper rules (reset on
/// `start_paper_run`). Stores raw sums only — the API layer derives win rate
/// and average PnL % from these so the hot path never divides.
#[derive(Clone, Copy, Default)]
pub struct RuleClosedStats {
    /// Clean `End` exits sold above entry.
    pub wins: i64,
    /// Every other closed position (breakeven, loss, failed exit).
    pub losses: i64,
    /// Sum of realized SOL PnL across all closed positions.
    pub sum_pnl_sol: f64,
    /// Sum of realized PnL % across all closed positions (numerator for the avg).
    pub sum_pnl_pct: f64,
}

impl RuleClosedStats {
    /// Number of closed positions = the win/loss denominator.
    pub fn closed(&self) -> i64 {
        self.wins + self.losses
    }

    /// Fold one freshly-closed position into the counters. `sign = 1` adds it
    /// (a position just closed), `-1` backs it out (a closed position was
    /// removed). Classification matches the warm-up SQL exactly.
    fn apply(&mut self, p: &Position, sign: i64) {
        if p.is_win() {
            self.wins = (self.wins + sign).max(0);
        } else {
            self.losses = (self.losses + sign).max(0);
        }
        let s = sign as f64;
        if let Some(sol) = p.pnl_sol() {
            self.sum_pnl_sol += s * sol;
        }
        if let Some(pct) = p.pnl_percentage() {
            self.sum_pnl_pct += s * pct;
        }
    }
}

/// RAII claim on an in-flight exit. While held, `position_id` stays in the
/// `exiting` set (the no-double-sell guard); dropping it — on normal return,
/// early return, OR a panic that unwinds the holding task — frees the slot
/// automatically. Returned by [`Tpsl2RuntimeCache::try_begin_exit`]; move it into
/// the spawned sell/fill-poll task so the claim lives exactly as long as the exit.
pub struct ExitGuard {
    exiting: Arc<DashSet<Uuid>>,
    position_id: Uuid,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.exiting.remove(&self.position_id);
    }
}

/// RAII claim on an in-flight **entry** (a real snipe buy in progress). The exact
/// mirror of [`ExitGuard`] for the buy side: while held, `position_id` stays in
/// the `entering` set, so the buy-recovery reaper (`redrive_orphaned_buy_submitted`)
/// skips a position whose live buy task is still running. Dropping it — on normal
/// return, early return, OR a panic that unwinds the buy task — frees the slot, so
/// after a crash the set is empty and the reaper can claim and recover. Returned by
/// [`Tpsl2RuntimeCache::try_begin_entry`]; move it into the spawned buy task.
pub struct EntryGuard {
    entering: Arc<DashSet<Uuid>>,
    position_id: Uuid,
}

impl Drop for EntryGuard {
    fn drop(&mut self) {
        self.entering.remove(&self.position_id);
    }
}

/// RAII slot for an **until-dead** scalp armer — a real/paper entry watch with no
/// `p_entry_max_age_secs` ceiling, which would otherwise pin a never-dying token
/// (and its `token_cache` entry / `paper_poll_sem` permit) indefinitely. While
/// held, `position_id` occupies one of the [`MAX_UNTIL_DEAD_ARMERS`] slots; the
/// armer polls [`is_cancelled`](UntilDeadArmerGuard::is_cancelled) each tick and
/// bails if a newer armer evicted it. Dropping it — normal return, eviction, or a
/// panic — frees the slot. A max-age-bounded armer is self-limiting (its deadline)
/// and takes no slot.
pub struct UntilDeadArmerGuard {
    registry: Arc<DashMap<Uuid, UntilDeadArmerSlot>>,
    position_id: Uuid,
    cancelled: Arc<AtomicBool>,
}

impl UntilDeadArmerGuard {
    /// True once a newer armer evicted this one because the cap was reached. The
    /// watch loop checks this each tick and returns (dropping its unentered
    /// position), so the freed slot is reclaimed promptly.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for UntilDeadArmerGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.position_id);
    }
}

/// Registry value behind an [`UntilDeadArmerGuard`]: a monotonic `seq` (the
/// eviction order — smallest = oldest) and the shared cancel flag.
#[derive(Clone)]
struct UntilDeadArmerSlot {
    seq: u64,
    cancelled: Arc<AtomicBool>,
}

/// In-memory TPSL state for the strategy hot path (rules + open positions + rule counters).
///
/// Counters are mode-aware: for real rules `total_count_by_rule` is all-time
/// (from `positions`); for paper rules it is scoped to the current run (reset on
/// each run start). `holding_*` track open positions of both modes (paper rule
/// ids and real rule ids are disjoint, so the shared maps never collide).
#[derive(Clone)]
pub struct Tpsl2RuntimeCache {
    active_rules: Arc<RwLock<Arc<Vec<Tpsl2Rule>>>>,
    rules_by_id: Arc<RwLock<HashMap<Uuid, Tpsl2Rule>>>,
    holding_by_mint: Arc<DashMap<String, Vec<Arc<Position>>>>,
    holding_count_by_rule: Arc<DashMap<Uuid, i64>>,
    total_count_by_rule: Arc<DashMap<Uuid, i64>>,
    /// Per-rule realized-performance counters (wins/losses + PnL sums). Same
    /// mode-aware scope as `total_count_by_rule`: all-time for real rules,
    /// current-run for paper rules. Read by the rule-list endpoint; mutated on
    /// each position close in `sync_position`.
    closed_stats_by_rule: Arc<DashMap<Uuid, RuleClosedStats>>,
    /// Current paper run per paper rule (stamping target + result pointer).
    paper_run_by_rule: Arc<DashMap<Uuid, PaperRunRef>>,
    /// Memoized clock-driven exit walk state per holding position, keyed by
    /// position id. Seeded once and advanced as trades print, so the per-second
    /// time-exit sweep never re-walks a token's full history. Lifecycle is tied
    /// to the holding index: an entry is dropped when its position leaves Holding.
    exit_state_by_position: Arc<DashMap<Uuid, CachedExitState>>,
    /// Secondary index over the holdings: only the Holding positions whose rule
    /// currently carries a time-based exit (TimeStop / Stall). The per-second
    /// wall-clock sweep iterates *this* set instead of cloning every holding
    /// `Arc<Position>` each tick — positions with only price exits never appear
    /// here. Kept in lockstep with the holding index on every add/remove, and
    /// rebuilt wholesale on a rule reload (a rule's time-exit config can change).
    time_exit_holding: Arc<DashMap<Uuid, Arc<Position>>>,
    /// Caps concurrent paper entry/exit fill-poll tasks. Each spawn acquires a
    /// permit before doing DB work, so a burst of fills can't spawn an unbounded
    /// number of feed-polling tasks all hammering the DB at once.
    paper_poll_sem: Arc<Semaphore>,
    /// Positions with an exit currently in flight — a real on-chain sell or a
    /// paper exit fill-poll. Lives on the runtime cache (not the service) so
    /// **every** exit path shares one guard: the trade/time ladder
    /// (`trigger_real_exit` / paper fill-poll) and the manual Stop&Close
    /// lifecycle. Claiming here closes the double-sell race where Stop&Close and
    /// a concurrent ladder exit could both submit a sell for the same position,
    /// and bounds the paper fill-poll to one task per position (no re-spawn storm
    /// when an ExitPending DB write fails and leaves the position Holding).
    exiting: Arc<DashSet<Uuid>>,
    /// Positions with a real snipe **buy** currently in flight — the buy-side twin
    /// of `exiting`. The live buy task claims its slot for the buy's lifetime; the
    /// buy-recovery reaper (`redrive_orphaned_buy_submitted`) claims via
    /// [`try_begin_entry`] to decide whether a `BuySubmitted` row is orphaned (no
    /// live task → claimable → recover) or genuinely in flight (claim refused →
    /// skip). After a crash the set is empty, so every reloaded `BuySubmitted` row
    /// is recoverable. The RAII guard frees the slot on drop incl. a panic.
    entering: Arc<DashSet<Uuid>>,
    /// Active **until-dead** scalp armers (entry watches with no `max_age`
    /// ceiling), keyed by position id. Bounded to [`MAX_UNTIL_DEAD_ARMERS`]: when
    /// full, the oldest un-entered armer is evicted so a never-dying token can't
    /// pin an unbounded number of watches (and their `token_cache` entries). A
    /// max-age-bounded armer is self-limiting and never registers here.
    until_dead_armers: Arc<DashMap<Uuid, UntilDeadArmerSlot>>,
    /// Monotonic counter assigning each until-dead armer its eviction-order `seq`.
    armer_seq: Arc<AtomicU64>,
    /// Cold-lane broadcast — every position transition emits a `TpslPositionsChanged`
    /// signal so SSE clients refetch the affected rule's positions in real time
    /// instead of polling.
    sse_tx: broadcast::Sender<SseEvent>,
}

/// Max concurrent paper fill-poll tasks (entry + exit) for this strategy.
const PAPER_POLL_CONCURRENCY: usize = 64;

/// Max concurrent **until-dead** scalp armers (real + paper) — entry watches with
/// no `p_entry_max_age_secs` ceiling. A healthy pumping token never satisfies
/// `is_dead` (liquidity-depletion + quiet), so an until-dead watch on one could
/// pin its `token_cache` entry forever; this caps how many such watches run at
/// once. When full, [`begin_until_dead_armer`](Tpsl2RuntimeCache::begin_until_dead_armer)
/// evicts the oldest un-entered armer. Bounded (max-age) armers are self-limiting
/// and don't count against this.
const MAX_UNTIL_DEAD_ARMERS: usize = 32;

impl Tpsl2RuntimeCache {
    pub fn new(sse_tx: broadcast::Sender<SseEvent>) -> Self {
        Self {
            active_rules: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            rules_by_id: Arc::new(RwLock::new(HashMap::new())),
            holding_by_mint: Arc::new(DashMap::new()),
            holding_count_by_rule: Arc::new(DashMap::new()),
            total_count_by_rule: Arc::new(DashMap::new()),
            closed_stats_by_rule: Arc::new(DashMap::new()),
            paper_run_by_rule: Arc::new(DashMap::new()),
            exit_state_by_position: Arc::new(DashMap::new()),
            time_exit_holding: Arc::new(DashMap::new()),
            paper_poll_sem: Arc::new(Semaphore::new(PAPER_POLL_CONCURRENCY)),
            exiting: Arc::new(DashSet::new()),
            entering: Arc::new(DashSet::new()),
            until_dead_armers: Arc::new(DashMap::new()),
            armer_seq: Arc::new(AtomicU64::new(0)),
            sse_tx,
        }
    }

    /// Shared semaphore bounding concurrent paper fill-poll tasks.
    pub fn paper_poll_sem(&self) -> Arc<Semaphore> {
        self.paper_poll_sem.clone()
    }

    /// Claim `position_id` for an in-flight exit (real sell or paper fill-poll).
    /// Returns `Some(ExitGuard)` if newly claimed — hold it for the whole exit;
    /// returns `None` if an exit is already running for it, in which case the
    /// caller MUST skip (the no-double-sell invariant). The guard frees the slot
    /// on drop, so a spawned sell/fill-poll task that **panics** mid-exit can no
    /// longer wedge the `exiting` slot permanently (the old manual `end_exit`
    /// release never ran on a panic, stranding the position forever).
    pub fn try_begin_exit(&self, position_id: Uuid) -> Option<ExitGuard> {
        if self.exiting.insert(position_id) {
            Some(ExitGuard {
                exiting: self.exiting.clone(),
                position_id,
            })
        } else {
            None
        }
    }

    /// Whether an exit is currently in flight for `position_id` (its guard is
    /// held). The ExitPending reaper uses this only diagnostically — it claims via
    /// [`try_begin_exit`] (atomic) to decide whether to re-drive, never this.
    #[cfg(test)]
    pub fn is_exiting(&self, position_id: Uuid) -> bool {
        self.exiting.contains(&position_id)
    }

    /// Claim `position_id` for an in-flight entry (a real snipe buy). Returns
    /// `Some(EntryGuard)` if newly claimed — hold it for the whole buy; returns
    /// `None` if a buy is already running for it, in which case the caller MUST
    /// skip. The atomic claim doubles as the in-flight check for the buy-recovery
    /// reaper (a live buy task holds the guard → reaper's claim returns `None` →
    /// skip), exactly mirroring `try_begin_exit`. The guard frees the slot on
    /// drop, so a panicked buy task can't wedge the slot forever.
    pub fn try_begin_entry(&self, position_id: Uuid) -> Option<EntryGuard> {
        if self.entering.insert(position_id) {
            Some(EntryGuard {
                entering: self.entering.clone(),
                position_id,
            })
        } else {
            None
        }
    }

    /// Whether a buy is currently in flight for `position_id` (its entry guard is
    /// held). Diagnostic/test only — the reaper claims via [`try_begin_entry`].
    #[cfg(test)]
    pub fn is_entering(&self, position_id: Uuid) -> bool {
        self.entering.contains(&position_id)
    }

    /// Claim an **until-dead** scalp-armer slot for `position_id` (an entry watch
    /// with no `max_age` ceiling). Bounded to [`MAX_UNTIL_DEAD_ARMERS`]: when full,
    /// the OLDEST un-entered armer is evicted — its
    /// [`is_cancelled`](UntilDeadArmerGuard::is_cancelled) flips true so its watch
    /// loop bails and drops its unentered position — and the eviction is logged (no
    /// silent cap; data-scale guardrail). The returned guard frees this slot on
    /// drop (normal return, eviction, or panic). Hold it only for the duration of
    /// the watch: once armed (an entry is found), drop it so the slot frees while
    /// the buy proceeds.
    pub fn begin_until_dead_armer(&self, position_id: Uuid) -> UntilDeadArmerGuard {
        // A position re-arming reuses its own slot — clear any stale entry first so
        // it never evicts itself or double-counts against the cap.
        self.until_dead_armers.remove(&position_id);
        // Evict oldest while at capacity. `min_by_key` consumes the iterator and we
        // copy out the key + cancel flag before `remove`, so no DashMap ref is held
        // across the mutation (avoids the iter-while-remove deadlock).
        while self.until_dead_armers.len() >= MAX_UNTIL_DEAD_ARMERS {
            let oldest = self
                .until_dead_armers
                .iter()
                .min_by_key(|e| e.value().seq)
                .map(|e| (*e.key(), e.value().cancelled.clone()));
            match oldest {
                Some((id, cancelled)) => {
                    cancelled.store(true, Ordering::Release);
                    self.until_dead_armers.remove(&id);
                    tracing::warn!(
                        evicted_position = %id,
                        cap = MAX_UNTIL_DEAD_ARMERS,
                        "until-dead scalp-armer cap reached; evicting the oldest un-entered armer"
                    );
                }
                None => break,
            }
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let seq = self.armer_seq.fetch_add(1, Ordering::Relaxed);
        self.until_dead_armers
            .insert(position_id, UntilDeadArmerSlot { seq, cancelled: cancelled.clone() });
        UntilDeadArmerGuard {
            registry: self.until_dead_armers.clone(),
            position_id,
            cancelled,
        }
    }

    pub async fn load_from_db(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rule_repo = Tpsl2StrategyRuleRepo::new(pool.clone());
        let position_repo = Tpsl2PositionRepo::new(pool.clone());
        let paper_repo = Tpsl2PaperTradingRepo::new(pool.clone());

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

        // Realized-performance counters, warmed the same way: real rules all-time,
        // paper rules per current run (attributed below via run_id → rule_id).
        self.closed_stats_by_rule.clear();
        for (rule_id, wins, losses, sum_pnl_sol, sum_pnl_pct) in
            position_repo.closed_stats_by_rule().await?
        {
            if !paper_ids.contains(&rule_id) {
                self.closed_stats_by_rule.insert(
                    rule_id,
                    RuleClosedStats { wins, losses, sum_pnl_sol, sum_pnl_pct },
                );
            }
        }

        self.paper_run_by_rule.clear();
        // One GROUP BY for every run's position count instead of a per-run query.
        let counts_by_run = paper_repo.count_by_run_all().await?;
        let stats_by_run = paper_repo.closed_stats_by_run_all().await?;
        for run in paper_repo.find_all_runs().await? {
            // A rule is one mode at a time: once it's flipped to real, its old
            // (stopped-but-undeleted) paper runs must not paint stats onto it.
            // Only attribute paper-run results to rules still in paper mode.
            if !paper_ids.contains(&run.rule_id) {
                continue;
            }
            let count = counts_by_run.get(&run.id).copied().unwrap_or(0);
            if count > 0 {
                self.total_count_by_rule.insert(run.rule_id, count);
            }
            if let Some(&(wins, losses, sum_pnl_sol, sum_pnl_pct)) = stats_by_run.get(&run.id) {
                self.closed_stats_by_rule.insert(
                    run.rule_id,
                    RuleClosedStats { wins, losses, sum_pnl_sol, sum_pnl_pct },
                );
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
    /// `tpsl2_real_positions` table (excluding paper-rule rows) and `tpsl2_paper_positions`.
    async fn load_holdings(&self, pool: &PgPool) -> anyhow::Result<()> {
        let paper_ids = self.paper_rule_ids();
        let mut all: Vec<Position> = Tpsl2PositionRepo::new(pool.clone())
            .find_all_holding()
            .await?
            .into_iter()
            .filter(|p| !paper_ids.contains(&p.rule_id))
            .collect();
        all.extend(Tpsl2PaperTradingRepo::new(pool.clone()).find_all_holding().await?);
        self.set_holding_positions(all);
        Ok(())
    }

    pub async fn reload_rules(&self, pool: &PgPool) -> anyhow::Result<()> {
        let rules = Tpsl2StrategyRuleRepo::new(pool.clone()).find_all().await?;
        self.set_rules(rules);
        Ok(())
    }

    fn set_rules(&self, rules: Vec<Tpsl2Rule>) {
        let active: Vec<_> = rules.iter().filter(|r| r.is_active).cloned().collect();
        let by_id: HashMap<_, _> = rules.into_iter().map(|r| (r.id, r)).collect();
        if let Ok(mut a) = self.active_rules.write() {
            *a = Arc::new(active);
        }
        if let Ok(mut m) = self.rules_by_id.write() {
            *m = by_id;
        }
        // A rule's time-exit config may have changed, flipping membership for its
        // open positions — rebuild the time-exit index against the new rule set.
        self.rebuild_time_exit_index();
    }

    /// Whether the rule (by id) currently carries a time-based exit (TimeStop or
    /// Stall). Matches the gate `sweep_time_exits` applies per position.
    fn rule_has_time_exit(&self, rule_id: Uuid) -> bool {
        self.rules_by_id
            .read()
            .ok()
            .and_then(|m| {
                m.get(&rule_id).map(|r| {
                    none_if_zero_u64(r.p_exit_time_stop_secs).is_some()
                        || none_if_zero_u64(r.p_exit_stall_secs).is_some()
                })
            })
            .unwrap_or(false)
    }

    /// Rebuild the time-exit index from the current holdings + rules. Cheap
    /// (holdings are small); called on bulk holding loads and rule reloads.
    fn rebuild_time_exit_index(&self) {
        self.time_exit_holding.clear();
        for entry in self.holding_by_mint.iter() {
            for pos in entry.value() {
                if self.rule_has_time_exit(pos.rule_id) {
                    self.time_exit_holding.insert(pos.id, pos.clone());
                }
            }
        }
    }

    fn set_holding_positions(&self, positions: Vec<Position>) {
        self.holding_by_mint.clear();
        self.holding_count_by_rule.clear();

        let mut by_mint: HashMap<String, Vec<Arc<Position>>> = HashMap::new();
        let mut holding_by_rule: HashMap<Uuid, i64> = HashMap::new();

        for pos in positions {
            // holding_count tracks only entered positions (cap enforcement).
            if pos.entry_price.is_some() {
                *holding_by_rule.entry(pos.rule_id).or_insert(0) += 1;
            }
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
        // Rebuild the time-exit index to match the freshly-loaded holdings.
        self.rebuild_time_exit_index();
    }

    /// The active rule set, shared by `Arc` (callers clone the pointer, not the
    /// rules). A new handler is built per token creation, so this is hot.
    pub fn active_rules(&self) -> Arc<Vec<Tpsl2Rule>> {
        self.active_rules
            .read()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// O(1) lookup of a single rule by id (clones just that rule). The hot path
    /// uses this instead of cloning every rule per event.
    pub fn rule_by_id(&self, rule_id: Uuid) -> Option<Tpsl2Rule> {
        self.rules_by_id
            .read()
            .ok()
            .and_then(|m| m.get(&rule_id).cloned())
    }

    /// Hot-path gate accessor: resolve just the exit-ladder scalars for a rule
    /// under the read lock, without deep-cloning the whole `Tpsl2Rule` (which
    /// carries large fields incl. a `serde_json::Value`). The per-trade exit gate
    /// and the time sweep need only these; the full `rule_by_id` clone is reserved
    /// for the rare branch where a position actually exits.
    pub fn ladder_params_by_id(&self, rule_id: Uuid) -> Option<super::exit::LadderParams> {
        self.rules_by_id
            .read()
            .ok()
            .and_then(|m| m.get(&rule_id).map(super::exit::LadderParams::from_rule))
    }

    pub fn holding_by_mint(&self, mint: &str) -> Vec<Arc<Position>> {
        self.holding_by_mint
            .get(mint)
            .map(|e| e.value().clone())
            .unwrap_or_default()
    }

    /// True when this strategy currently holds at least one open position (paper
    /// or real) on `mint`. The token-cache eviction sweep consults this so a mint
    /// with a live exit is never dropped from the cache — its sell-confirm /
    /// exit-fill loops resolve fills against that cache. O(1) shard lookup; an
    /// empty list is never retained (see `remove_from_holding_index`), so key
    /// presence already implies at least one open position.
    pub fn is_mint_held(&self, mint: &str) -> bool {
        self.holding_by_mint.contains_key(mint)
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

    /// Snapshot of only the Holding positions whose rule carries a time-based
    /// exit. The per-second sweep iterates this (usually far smaller, often
    /// empty) set instead of every holding. Pointer-clones, no guard held.
    pub fn time_exit_holding_positions(&self) -> Vec<Arc<Position>> {
        self.time_exit_holding
            .iter()
            .map(|e| e.value().clone())
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
        trades: &[CachedTrade],
        trades_base: u64,
    ) -> ExitWalkState {
        let cached = CachedExitState::build(trades, trades_base, entry_price, entry_time);
        let state = cached.state;
        self.exit_state_by_position.insert(position_id, cached);
        state
    }

    /// Trade-gate variant: fold newly-printed trades into the memoized walk state +
    /// E5 cohort net (seeding both if unseen) **and** evaluate the exit ladder
    /// against only those new trades, returning the first
    /// [`ExitReason`](super::exit::ExitReason) that fires. Replaces the per-ping
    /// full re-walk (H3) and the per-ping cohort rebuild (H4) with one incremental
    /// pass (see [`CachedExitState::advance_and_find_exit`]).
    ///
    /// First sight seeds an unfolded state (cohort memo computed once from the
    /// retained window when E5 is configured) and folds+evaluates the whole window,
    /// reproducing the old first-ping full re-walk. If the clock sweep seeded the
    /// state first (no cohort memo), the cohort memo is lazily attached here so E5
    /// is never silently dropped.
    pub fn exit_state_advance_and_find_exit(
        &self,
        position_id: Uuid,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        trades: &[CachedTrade],
        trades_base: u64,
        params: &super::exit::LadderParams,
    ) -> Option<super::exit::ExitReason> {
        use dashmap::mapref::entry::Entry;
        match self.exit_state_by_position.entry(position_id) {
            Entry::Occupied(mut e) => {
                let cached = e.get_mut();
                cached.ensure_cohort_seeded(trades, trades_base, params);
                cached.advance_and_find_exit(trades, trades_base, params)
            }
            Entry::Vacant(v) => {
                let mut cached = CachedExitState::build_unfolded(
                    trades,
                    trades_base,
                    entry_price,
                    entry_time,
                    params,
                );
                let reason = cached.advance_and_find_exit(trades, trades_base, params);
                v.insert(cached);
                reason
            }
        }
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

    /// Realized-performance counters for a rule (wins/losses + PnL sums), or a
    /// zeroed default when the rule has no closed positions yet.
    pub fn closed_stats_by_rule(&self, rule_id: Uuid) -> RuleClosedStats {
        self.closed_stats_by_rule
            .get(&rule_id)
            .map(|e| *e.value())
            .unwrap_or_default()
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
        let run = Tpsl2PaperTradingRepo::new(pool.clone())
            .start_run(rule_id, max_total_tokens)
            .await?;
        // The prior run's positions were deleted in the DB; drop any that linger
        // in the in-memory holding index so counts start from zero.
        self.purge_rule_from_holding_index(rule_id);
        self.holding_count_by_rule.remove(&rule_id);
        self.total_count_by_rule.remove(&rule_id);
        // Fresh run ⇒ fresh stats (the prior run's positions were deleted).
        self.closed_stats_by_rule.remove(&rule_id);
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
        let repo = Tpsl2PaperTradingRepo::new(pool.clone());
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
        let repo = Tpsl2PaperTradingRepo::new(pool.clone());
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
        let repo = Tpsl2PaperTradingRepo::new(pool.clone());
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
            self.time_exit_holding.remove(&id);
        }
    }

    /// Call after DB writes that change position status or create/delete a position.
    pub fn sync_position(&self, prev: Option<&Position>, current: &Position) {
        let prev_in_holding_index = prev.map(|p| p.is_in_holding_index()).unwrap_or(false);
        let curr_in_holding_index = current.is_in_holding_index();

        // Holding index: Arming, BuySubmitted, and Holding all belong (exit-gating relies
        // on this; the fill-adopt path needs to find the pending row by mint).
        if prev_in_holding_index {
            self.remove_from_holding_index(prev.unwrap());
            if !curr_in_holding_index {
                // Position is leaving the holding index — memoized exit state is dead.
                self.exit_state_by_position.remove(&prev.unwrap().id);
            }
        }
        if curr_in_holding_index {
            self.upsert_in_holding_index(current);
        }

        // Cap counters: only positions that have a real entry (SOL deployed) count.
        let prev_entered = prev.map(|p| p.entry_price.is_some()).unwrap_or(false);
        let curr_entered = current.entry_price.is_some();

        // total_count: increment exactly once when a position first gets a real entry.
        if curr_entered && !prev_entered {
            self.adjust_total_count(current.rule_id, 1);
        }

        // holding_count: entered positions in Holding (not Arming/BuySubmitted — those
        // haven't deployed SOL yet and must not count toward the cap).
        let prev_holding_entered = prev
            .map(|p| p.entry_price.is_some() && p.status == PositionStatus::Holding)
            .unwrap_or(false);
        let curr_holding_entered = curr_entered && current.status == PositionStatus::Holding;
        if curr_holding_entered && !prev_holding_entered {
            self.adjust_holding_count(current.rule_id, 1);
        } else if prev_holding_entered && !curr_holding_entered {
            self.adjust_holding_count(current.rule_id, -1);
        }

        // Realized-performance counters: accumulate exactly on the transition
        // into a terminally-closed state (entered → End/ExitFailed). Terminal
        // states never transition again, so this fires once per position.
        let prev_closed = prev.map(|p| p.is_closed()).unwrap_or(false);
        if current.is_closed() && !prev_closed {
            self.closed_stats_by_rule
                .entry(current.rule_id)
                .or_default()
                .apply(current, 1);
        }

        // Cold-lane signal: ship the changed row + the rule's fresh cap counters
        // so clients patch one row + the badge in place (no list refetch).
        self.emit_position_changed(current, false);
    }

    /// Best-effort delta broadcast for a position change/removal. Skips building
    /// the (cloned) payload entirely when nothing is listening. Counters are read
    /// AFTER the caller has applied its cap adjustments.
    fn emit_position_changed(&self, position: &Position, removed: bool) {
        if self.sse_tx.receiver_count() == 0 {
            return;
        }
        let rule_snapshot = self
            .rules_by_id
            .read()
            .ok()
            .and_then(|m| m.get(&position.rule_id).cloned())
            .map(|r| {
                Box::new(backend_core::models::ingest::RuleNotifSnapshot {
                    rule_name: r.rule_name.clone(),
                    trade_mode: r.trade_mode.clone(),
                    p_token_initial_buy_sol: r.p_token_initial_buy_sol,
                    tolerance_pct: r.tolerance_pct,
                    p_token_cu_limit: r.p_token_cu_limit,
                    p_token_cu_price: r.p_token_cu_price,
                    p_token_max_sol_cost: r.p_token_max_sol_cost,
                    p_token_spendable_sol_in: r.p_token_spendable_sol_in,
                    p_token_ix_labels: r.p_token_ix_labels.as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                    p_exit_take_profit: r.p_exit_take_profit,
                    p_exit_stop_loss: r.p_exit_stop_loss,
                })
            });
        let _ = self.sse_tx.send(SseEvent::TpslPositionsChanged {
            strategy: "tpsl2".to_string(),
            rule_id: position.rule_id,
            rule_snapshot,
            position: Some(Box::new(position.clone())),
            removed,
            open_positions: self.holding_count_by_rule(position.rule_id),
            total_positions: self.total_count_by_rule(position.rule_id),
        });
    }

    pub fn remove_position(&self, position: &Position) {
        if position.is_in_holding_index() {
            self.remove_from_holding_index(position);
        }
        self.time_exit_holding.remove(&position.id);
        self.exit_state_by_position.remove(&position.id);
        // Only adjust cap counters if the position had a real entry.
        if position.entry_price.is_some() {
            if position.status == PositionStatus::Holding {
                self.adjust_holding_count(position.rule_id, -1);
            }
            self.adjust_total_count(position.rule_id, -1);
        }
        // Back the position out of the realized-performance counters if it had
        // already closed (keeps the warmed sums consistent when a closed row is
        // deleted, e.g. retention/cleanup), mirroring the +1 in `sync_position`.
        if position.is_closed() {
            if let Some(mut e) = self.closed_stats_by_rule.get_mut(&position.rule_id) {
                e.apply(position, -1);
            }
        }
        // Signal the removal so clients drop the row (and update the badge) without
        // waiting on the fallback poll. Counters are read after the adjustments above.
        self.emit_position_changed(position, true);
    }

    fn upsert_in_holding_index(&self, position: &Position) {
        let arc = Arc::new(position.clone());
        {
            let mut entry = self
                .holding_by_mint
                .entry(position.mint.clone())
                .or_insert_with(Vec::new);
            if let Some(slot) = entry.iter_mut().find(|p| p.id == position.id) {
                *slot = arc.clone();
            } else {
                entry.push(arc.clone());
            }
        }
        // Keep the time-exit index in lockstep with the holding entry.
        if self.rule_has_time_exit(position.rule_id) {
            self.time_exit_holding.insert(position.id, arc);
        } else {
            self.time_exit_holding.remove(&position.id);
        }
    }

    fn remove_from_holding_index(&self, position: &Position) {
        self.time_exit_holding.remove(&position.id);
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

    fn cache() -> Tpsl2RuntimeCache {
        let (tx, _rx) = broadcast::channel(8);
        Tpsl2RuntimeCache::new(tx)
    }

    fn holding_position(rule_id: Uuid) -> Position {
        Position::new("MintAAA".into(), "WalletBBB".into(), "TPSL2".into(), rule_id)
    }

    /// Closing an entered position accumulates the realized-performance counters
    /// exactly once: a profitable close lands in `wins` with positive SOL/%; a
    /// failed exit lands in `losses` with the lost bag booked as a SOL loss.
    #[test]
    fn sync_position_accumulates_closed_stats() {
        let cache = cache();
        let rule_id = Uuid::new_v4();

        // Win: enter, then close above entry.
        let mut win = holding_position(rule_id);
        win.mint = "MintWin".into();
        win.entry_price = Some(1.0);
        win.entry_token_amount = Some(100.0);
        win.mark_entry_filled();
        cache.sync_position(None, &win);
        let prev = win.clone();
        win.close(2.0, vec!["sig".into()], 100.0, Utc::now());
        cache.sync_position(Some(&prev), &win);

        let stats = cache.closed_stats_by_rule(rule_id);
        assert_eq!((stats.wins, stats.losses), (1, 0));
        assert_eq!(stats.sum_pnl_sol, 100.0);
        assert_eq!(stats.sum_pnl_pct, 100.0);

        // Re-syncing the already-closed position must NOT double-count.
        cache.sync_position(Some(&win), &win);
        assert_eq!(cache.closed_stats_by_rule(rule_id).wins, 1);

        // Loss via failed exit: closed, not a win, SOL loss = -entry cost.
        let mut fail = holding_position(rule_id);
        fail.mint = "MintFail".into();
        fail.entry_price = Some(1.0);
        fail.entry_token_amount = Some(100.0);
        fail.mark_entry_filled();
        cache.sync_position(None, &fail);
        let prev = fail.clone();
        fail.mark_exit_failed(0.0, Utc::now());
        cache.sync_position(Some(&prev), &fail);

        let stats = cache.closed_stats_by_rule(rule_id);
        assert_eq!((stats.wins, stats.losses), (1, 1));
        assert_eq!(stats.sum_pnl_sol, 0.0); // +100 win, -100 failed
    }

    /// The RAII exit guard frees the `exiting` slot when dropped — including the
    /// early-return / panic-unwind case the old manual `end_exit` never covered.
    #[test]
    fn exit_guard_frees_slot_on_drop() {
        let cache = cache();
        let id = Uuid::new_v4();

        let guard = cache.try_begin_exit(id).expect("first claim succeeds");
        assert!(cache.is_exiting(id));
        // A second claim while the first guard is held must be refused.
        assert!(cache.try_begin_exit(id).is_none(), "double-claim refused");

        drop(guard);
        assert!(!cache.is_exiting(id), "slot freed on drop");
        // Freed — claimable again.
        assert!(cache.try_begin_exit(id).is_some(), "re-claimable after release");
    }

    /// The entry guard mirrors the exit guard: it gates a second claim while held
    /// and frees the `entering` slot on drop (incl. a panic), so the buy-recovery
    /// reaper skips a live buy but can re-claim after a crash.
    #[test]
    fn entry_guard_frees_slot_on_drop() {
        let cache = cache();
        let id = Uuid::new_v4();

        let guard = cache.try_begin_entry(id).expect("first claim succeeds");
        assert!(cache.is_entering(id));
        assert!(cache.try_begin_entry(id).is_none(), "double-claim refused");

        drop(guard);
        assert!(!cache.is_entering(id), "slot freed on drop");
        assert!(cache.try_begin_entry(id).is_some(), "re-claimable after release");
    }

    /// A spawned task that panics while holding the guard still frees the slot
    /// (Drop runs on unwind), so a panicked sell can't wedge the position forever.
    #[tokio::test]
    async fn exit_guard_frees_slot_when_holding_task_panics() {
        let cache = cache();
        let id = Uuid::new_v4();
        let guard = cache.try_begin_exit(id).expect("claim");

        let handle = tokio::spawn(async move {
            let _g = guard; // moved in; dropped on unwind
            panic!("sell task blew up mid-exit");
        });
        assert!(handle.await.is_err(), "task panicked");
        assert!(!cache.is_exiting(id), "guard freed the slot on panic unwind");
    }
}
