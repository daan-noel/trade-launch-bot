//! Recovery reaper — safety backstop for `strategy_positions` rows the engine
//! loop can't resolve on its own:
//! - `BuySubmitted` with no live entry task → adopt / wait / drop (never re-send);
//!   unresolved past the review window → `needs_review` SSE flag (manual Verify)
//! - `ExitPending` with no live exit task → re-drive sell (or nudge the engine)
//! - externally-cleared `Holding` (bag gone via Trade sell) → book End (PG `trades` net)
//! - `ExitStuck` with remaining bag (PG net) → re-drive sell with backoff, then park
//! - `ExitStuck` / `ExitUnconfirmed` whose bag is gone → heal to End
//! - stale `ExitPending` → bag-check first: cleared heals to End, a held bag
//!   flips to `ExitStuck` (open, attention) — never a blind terminal stamp
//!
//! Fires once at boot (immediate tick) then every 60 s. Skips rows whose
//! [`InFlightGuards`] slot is held so it never races a live buy/sell task.
//!
//! Helius: no periodic account/balance RPC. Cleared/stranded detection is Postgres
//! `trades` net only; sell confirm stays feed-first inside `run_exit`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason};

use trading_core::config::constants::BUY_SUBMITTED_REVIEW_SECS;
use trading_core::models::ingest::SseEvent;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::trader::PumpFunTrader;

use super::exec_real::{self, BuyRecoveryVerdict};
use super::orphan_exit::{self, OrphanExitDeps, OrphanStart, BAG_CLEARED_THRESHOLD_RAW};
use super::{InFlightGuards, PositionRegistry};

const INTERVAL: Duration = Duration::from_secs(60);
const EXIT_PENDING_STALE: Duration = Duration::from_secs(300);
const UNENTERED_STALE: Duration = Duration::from_secs(600);
/// Flag unresolved `BuySubmitted` rows past this age for manual review (SSOT
/// window shared with the API's `needs_review` derivation).
const BUY_SUBMITTED_REVIEW: Duration = Duration::from_secs(BUY_SUBMITTED_REVIEW_SECS);
/// After a Fatal/unresolved ExitStuck redrive, skip this many reaper ticks
/// before trying again (no Helius spam).
const EXIT_STUCK_BACKOFF_TICKS: u8 = 5;
/// Bounded-then-park: total automatic redrives of one ExitStuck position before
/// it is parked (reaper stops auto-retrying, surfaced for a manual decision). Each
/// redrive can send reverting sells (tip+fees) on a dead/rugged pool, so this is
/// kept low. Reset by a manual Retry (`unpark_exit`) or a fresh entry.
const EXIT_REDRIVE_CAP: i32 = 2;

pub struct ReaperDeps {
    pub strategy_repo: StrategyRepo,
    pub trade_repo: TradeRepo,
    pub trader: Arc<PumpFunTrader>,
    pub token_cache: Arc<TokenCache>,
    pub trade_signals: Arc<TradeSignals>,
    pub inflight: InFlightGuards,
    pub registry: PositionRegistry,
    pub fill_tx: mpsc::Sender<Event>,
    pub settings: watch::Receiver<AppSettings>,
    /// SSE hub — used to push the `needs_review` flag on a stale BuySubmitted
    /// (B3: was log-only, invisible to the UI).
    pub sse_tx: broadcast::Sender<SseEvent>,
}

impl ReaperDeps {
    fn orphan_deps(&self) -> OrphanExitDeps {
        OrphanExitDeps {
            strategy_repo: self.strategy_repo.clone(),
            trade_repo: self.trade_repo.clone(),
            trader: self.trader.clone(),
            token_cache: self.token_cache.clone(),
            trade_signals: self.trade_signals.clone(),
            inflight: self.inflight.clone(),
            registry: self.registry.clone(),
            fill_tx: self.fill_tx.clone(),
            settings: self.settings.clone(),
        }
    }
}

/// Spawn the reaper loop (immediate first tick, then every `INTERVAL`).
pub fn spawn_reaper(deps: ReaperDeps) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip counters for stuck BuySubmitted rows past the review window — still
        // try feed-adopt every tick, but don't burn signature_state RPC every 60s.
        let mut stuck_rpc_skip: HashMap<Uuid, u8> = HashMap::new();
        let mut exit_stuck_backoff: HashMap<Uuid, u8> = HashMap::new();
        // Do NOT consume the immediate first tick — boot sweep runs right away.
        loop {
            tick.tick().await;
            redrive_orphaned_buy_submitted(&deps, &mut stuck_rpc_skip).await;
            orphan_exit::reconcile_externally_cleared_holdings(&deps.orphan_deps()).await;
            // Book any ExitPending whose bag is already gone BEFORE re-driving the
            // sell — else the re-drive would submit a phantom sell of an empty bag.
            heal_exit_pending_cleared(&deps).await;
            redrive_orphaned_exit_pending(&deps).await;
            redrive_exit_stuck(&deps, &mut exit_stuck_backoff).await;
            // Bag-gone heals: ExitStuck (post-redrive clear) and ExitUnconfirmed
            // (the sell DID land, the feed just confirmed late) → book End.
            heal_cleared_by_status(&deps, "ExitStuck").await;
            heal_cleared_by_status(&deps, "ExitUnconfirmed").await;
            // Stale ExitPending (real): bag-check first (S4) — the cleared heal
            // above already booked landed sells; what's left with a bag flips to
            // ExitStuck (open, attention) for the redrive/park machinery.
            match deps
                .strategy_repo
                .mark_stale_exit_pending_stuck(EXIT_PENDING_STALE, BAG_CLEARED_THRESHOLD_RAW)
                .await
            {
                Ok(n) if n > 0 => info!(n, "reaper: stale ExitPending → ExitStuck"),
                Ok(_) => {}
                Err(e) => warn!("reaper: mark_stale_exit_pending_stuck: {e}"),
            }
            // Stale paper ExitPending (crash mid-simulated sell): no on-chain bag
            // exists — book End at the entry (breakeven) rather than fabricating
            // a loss or stranding an attention row with no legal action.
            close_stale_paper_exit_pending(&deps).await;
            // Paper `BuySubmitted` that never filled (crash mid-sim buy) owns no
            // on-chain bag to recover — drop it past the stale window so it can't
            // linger as a forever-`Open` paper row. Real `BuySubmitted` is left to
            // the buy-recovery reaper (it may own on-chain tokens).
            match deps.strategy_repo.delete_stale_paper_buy_submitted(UNENTERED_STALE).await {
                Ok(n) if n > 0 => info!(n, "reaper: deleted stale paper BuySubmitted rows"),
                Ok(_) => {}
                Err(e) => warn!("reaper: delete_stale_paper_buy_submitted: {e}"),
            }
        }
    })
}

/// How many reaper ticks to skip `signature_state` RPC for a stuck row past
/// [`BUY_SUBMITTED_REVIEW`] (feed-adopt still runs every tick). 10 × 60s ≈ 10 min.
const STUCK_RPC_SKIP_TICKS: u8 = 10;

/// Outcome of a one-shot `BuySubmitted` resolve — shared by the reaper tick and
/// the Console's on-demand Verify action (`close?action=verify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyResolution {
    /// A submitted sig was already indexed — entry fill recorded → Holding.
    Adopted,
    /// Every submitted sig is a confirmed revert — row deleted, SOL released.
    Dropped,
    /// Still ambiguous (a sig may yet land) — wait; NEVER re-send.
    Wait,
}

/// Resolve one real `BuySubmitted` row: adopt an indexed fill, or drop it when
/// every submitted sig confirmed-reverted, else wait. Never re-sends. The caller
/// must hold the entry in-flight guard.
pub async fn resolve_buy_submitted(
    deps: &ReaperDeps,
    position: &trading_core::models::StrategyPosition,
) -> BuyResolution {
    resolve_buy_submitted_inner(deps, position, true).await
}

/// [`resolve_buy_submitted`] with the signature_state RPC pass optional — the
/// reaper throttles it for rows already flagged past the review window (feed
/// adopt still runs every tick).
async fn resolve_buy_submitted_inner(
    deps: &ReaperDeps,
    position: &trading_core::models::StrategyPosition,
    check_sigs: bool,
) -> BuyResolution {
    let wallet = deps.trader.wallet_pubkey();

    // 1. Adopt: any submitted sig already indexed → record the entry.
    for sig in &position.submitted_buy_signatures {
        if let Ok(Some(legs)) = deps
            .trade_repo
            .find_fill_by_signature(&wallet, &position.mint_address, sig)
            .await
        {
            if legs.token_amount == 0 {
                continue;
            }
            let token_account = position
                .token_account
                .clone()
                .or_else(|| deps.trader.cached_token_account(&position.mint_address));
            match deps
                .strategy_repo
                .record_entry_fill(
                    position.id,
                    sig,
                    legs.token_amount,
                    legs.price_per_token(),
                    legs.amount_sol,
                    legs.last_block_time,
                    token_account.as_deref(),
                )
                .await
            {
                Ok(_) => {
                    info!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: adopted BuySubmitted fill → Holding"
                    );
                    if let Some(engine_id) = deps.registry.engine_id(position.id) {
                        if let Some(meta) = deps.registry.get(engine_id) {
                            if let Some(intent) = meta.inflight_intent {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillConfirmed {
                                        intent,
                                        fill: Fill {
                                            price: legs.price_per_token(),
                                            sol: legs.amount_sol,
                                            token_amount: legs.token_amount,
                                            at: legs.last_block_time,
                                        },
                                    })
                                    .await;
                            }
                        }
                    }
                    return BuyResolution::Adopted;
                }
                Err(e) => warn!(
                    position_id = %position.id,
                    "reaper: record_entry_fill failed: {e}"
                ),
            }
        }
    }

    if position.submitted_buy_signatures.is_empty() {
        warn!(
            position_id = %position.id,
            mint = %position.mint_address,
            "reaper: BuySubmitted has no signatures — leaving for review"
        );
        return BuyResolution::Wait;
    }

    // 2. Drop ONLY if EVERY submitted sig is a confirmed revert.
    if !check_sigs {
        return BuyResolution::Wait;
    }
    let mut all_reverted = true;
    for sig in &position.submitted_buy_signatures {
        let status = deps.trader.signature_state(sig).await;
        if exec_real::classify_submitted_buy(&status) == BuyRecoveryVerdict::Wait {
            all_reverted = false;
            break;
        }
    }
    if !all_reverted {
        return BuyResolution::Wait;
    }

    match deps.strategy_repo.delete_position(position.id).await {
        Ok(()) => {
            deps.trader.release_sol_for_position(&position.id.to_string());
            info!(
                position_id = %position.id,
                mint = %position.mint_address,
                "reaper: dropped reverted BuySubmitted (no tokens)"
            );
            if let Some(engine_id) = deps.registry.engine_id(position.id) {
                if let Some(meta) = deps.registry.get(engine_id) {
                    if let Some(intent) = meta.inflight_intent {
                        let _ = deps
                            .fill_tx
                            .send(Event::FillFailed {
                                intent,
                                reason: FillFailReason::Fatal,
                            })
                            .await;
                    }
                }
            }
            BuyResolution::Dropped
        }
        Err(err) => {
            warn!(
                position_id = %position.id,
                "reaper: delete BuySubmitted failed: {err}"
            );
            BuyResolution::Wait
        }
    }
}

/// Recover `BuySubmitted` rows — adopt indexed fills, drop only when every
/// submitted sig confirmed-reverted, else wait. Never re-sends.
async fn redrive_orphaned_buy_submitted(
    deps: &ReaperDeps,
    stuck_rpc_skip: &mut HashMap<Uuid, u8>,
) {
    let submitted = match deps.strategy_repo.find_all_buy_submitted("real").await {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load BuySubmitted failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = submitted.iter().map(|p| p.id).collect();
    stuck_rpc_skip.retain(|id, _| live_ids.contains(id));

    for position in submitted {
        if deps.inflight.entry_held(position.id) {
            continue;
        }
        let Some(_guard) = deps.inflight.try_begin_entry(position.id) else {
            continue;
        };

        let age = Utc::now().signed_duration_since(position.updated_at);
        let past_review =
            age > chrono::Duration::from_std(BUY_SUBMITTED_REVIEW).unwrap_or_default();
        // Feed-adopt runs every tick; the signature_state RPC pass is throttled
        // for rows already flagged past the review window.
        let mut check_sigs = true;
        if past_review {
            match stuck_rpc_skip.get_mut(&position.id) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    check_sigs = false;
                }
                _ => {
                    warn!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: BuySubmitted unresolved past review window — manual review \
                         (throttling signature_state RPC)"
                    );
                    // Surface the flag to the UI (B3: was log-only). The API's
                    // `needs_review` derivation covers snapshots; this covers a
                    // session already hydrated before the row went stale.
                    let _ = deps.sse_tx.send(SseEvent::StrategyPositionUpdate {
                        rule_id: position.rule_id.unwrap_or_default(),
                        mint_address: position.mint_address.clone(),
                        position_id: position.id,
                        status: "BuySubmitted".to_string(),
                        exit_reason: None,
                        entry_price: position.entry_price,
                        exit_price: None,
                        trade_mode: Some("real".to_string()),
                        rule_name: None,
                        needs_review: Some(true),
                    });
                    stuck_rpc_skip.insert(position.id, STUCK_RPC_SKIP_TICKS);
                }
            }
        }

        match resolve_buy_submitted_inner(deps, &position, check_sigs).await {
            BuyResolution::Adopted | BuyResolution::Dropped => {
                stuck_rpc_skip.remove(&position.id);
            }
            BuyResolution::Wait => {}
        }
    }
}

/// Re-drive `ExitPending` rows whose exit guard is not held.
async fn redrive_orphaned_exit_pending(deps: &ReaperDeps) {
    let pending = match deps.strategy_repo.find_all_exit_pending("real").await {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitPending failed: {err}");
            return;
        }
    };
    let orphan = deps.orphan_deps();
    for position in pending {
        if position.entry_price.is_none() {
            continue;
        }
        if deps.inflight.exit_held(position.id) || deps.inflight.exit_mint_held(&position.mint_address)
        {
            continue;
        }

        // Prefer nudging the live engine when it still owns the position.
        if let Some(engine_id) = deps.registry.engine_id(position.id) {
            if let Some(meta) = deps.registry.get(engine_id) {
                if let Some(intent) = meta.inflight_intent.clone() {
                    info!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: nudging engine ExitPending via FillFailed::Reverted"
                    );
                    let _ = deps
                        .fill_tx
                        .send(Event::FillFailed {
                            intent,
                            reason: FillFailReason::Reverted,
                        })
                        .await;
                    continue;
                }
            }
        }

        match orphan_exit::spawn_orphan_sell(&orphan, position, "Recovery", false) {
            OrphanStart::Started => {}
            OrphanStart::Busy | OrphanStart::NothingToSell => {}
        }
    }
}

/// ExitPending rows whose PG net is already ≤ dust → book End (no sell, no RPC).
/// The in-flight sell that set `ExitPending` already cleared the bag; without this
/// the ExitPending re-drive would send a second sell that reverts for no tokens.
async fn heal_exit_pending_cleared(deps: &ReaperDeps) {
    let cleared = match deps
        .strategy_repo
        .find_exit_pending_cleared(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_exit_pending_cleared failed: {e}");
            return;
        }
    };
    if cleared.is_empty() {
        return;
    }
    let orphan = deps.orphan_deps();
    let wallet = deps.trader.wallet_pubkey();
    let mut n = 0u32;
    for pos in cleared {
        // Skip a position whose exit is actively in flight in THIS process — its
        // own confirm loop will book the close; don't race a book-close under it.
        if deps.inflight.exit_held(pos.id) || deps.inflight.exit_mint_held(&pos.mint_address) {
            continue;
        }
        let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
        if orphan_exit::book_externally_cleared(&orphan, &pos, fill).await.is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        info!(closed = n, "reaper: booked externally-cleared ExitPending rows");
    }
}

/// Re-drive real `ExitStuck` rows that still have a bag (PG net > dust).
async fn redrive_exit_stuck(deps: &ReaperDeps, backoff: &mut HashMap<Uuid, u8>) {
    let stuck = match deps
        .strategy_repo
        .find_exit_stuck_bags(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitStuck-with-bag failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = stuck.iter().map(|p| p.id).collect();
    backoff.retain(|id, _| live_ids.contains(id));

    let orphan = deps.orphan_deps();
    for position in stuck {
        if let Some(n) = backoff.get_mut(&position.id) {
            if *n > 0 {
                *n -= 1;
                continue;
            }
        }
        if deps.inflight.exit_held(position.id) || deps.inflight.exit_mint_held(&position.mint_address)
        {
            continue;
        }
        match orphan_exit::spawn_orphan_sell(&orphan, position.clone(), "Recovery", false) {
            OrphanStart::Started => {
                // Back off regardless of outcome so we don't re-spam every 60s while
                // the sell task runs / after Fatal.
                backoff.insert(position.id, EXIT_STUCK_BACKOFF_TICKS);
                // Bounded-then-park: count this redrive; at the cap, park it so the
                // reaper stops auto-retrying (a dead/rugged pool would otherwise burn
                // tip+fees forever) and it surfaces for a manual decision. Never
                // auto-written-off.
                match deps.strategy_repo.bump_exit_redrive(position.id).await {
                    Ok(n) if n >= EXIT_REDRIVE_CAP => {
                        match deps.strategy_repo.set_exit_parked(position.id, true).await {
                            Ok(()) => info!(
                                id = %position.id, mint = %position.mint_address, redrives = n,
                                "reaper: ExitStuck hit redrive cap → parked for manual resolution"
                            ),
                            Err(e) => {
                                warn!(id = %position.id, "reaper: park ExitStuck failed: {e}")
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => warn!(id = %position.id, "reaper: bump_exit_redrive failed: {e}"),
                }
            }
            OrphanStart::Busy => {}
            OrphanStart::NothingToSell => {
                let wallet = deps.trader.wallet_pubkey();
                let fill =
                    orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &position).await;
                let _ = orphan_exit::book_externally_cleared(&orphan, &position, fill).await;
            }
        }
    }
}

/// Rows in `status` (`ExitStuck` / `ExitUnconfirmed`) whose PG net is already
/// ≤ dust → book End (no sell, no RPC). For `ExitUnconfirmed` this is the heal
/// that never existed (S3): the sell DID land, the feed just confirmed late.
async fn heal_cleared_by_status(deps: &ReaperDeps, status: &str) {
    let cleared = match deps
        .strategy_repo
        .find_cleared_by_status(status, BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!(status, "reaper: find_cleared_by_status failed: {e}");
            return;
        }
    };
    if cleared.is_empty() {
        return;
    }
    let orphan = deps.orphan_deps();
    let wallet = deps.trader.wallet_pubkey();
    let mut n = 0u32;
    for pos in cleared {
        if deps.inflight.exit_held(pos.id) || deps.inflight.exit_mint_held(&pos.mint_address) {
            continue;
        }
        let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
        if orphan_exit::book_externally_cleared(&orphan, &pos, fill).await.is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        info!(n, status, "reaper: booked cleared rows → End");
    }
}

/// Stale **paper** `ExitPending` rows (crash mid-simulated sell): book End at the
/// entry price (breakeven) — paper has no on-chain bag to recover, and a
/// fabricated loss (or a forever-stuck attention row) would both lie.
async fn close_stale_paper_exit_pending(deps: &ReaperDeps) {
    let stale = match deps
        .strategy_repo
        .find_stale_paper_exit_pending(EXIT_PENDING_STALE)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_stale_paper_exit_pending failed: {e}");
            return;
        }
    };
    let mut n = 0u32;
    for pos in stale {
        let fill = Fill {
            price: pos.entry_price.unwrap_or(0.0),
            sol: pos.entry_sol.unwrap_or(0.0),
            token_amount: pos.entry_token_amount.unwrap_or(0),
            at: Utc::now(),
        };
        if orphan_exit::book_externally_cleared_pg(&deps.strategy_repo, pos.id, fill, "Manual")
            .await
            .is_ok()
        {
            n += 1;
        }
    }
    if n > 0 {
        info!(n, "reaper: closed stale paper ExitPending at breakeven");
    }
}
