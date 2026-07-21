//! Recovery reaper — safety backstop for `strategy_positions` rows the engine
//! loop can't resolve on its own:
//! - `BuySubmitted` with no live entry task → adopt / wait / drop (never re-send)
//! - `ExitPending` with no live exit task → re-drive sell (or nudge the engine)
//! - externally-cleared `Holding` (bag gone via Trade sell) → book End (PG `trades` net)
//! - `ExitFailed` with remaining bag (PG net) → re-drive sell with backoff
//! - stale `ExitPending` / `Arming` cleanup
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
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason};

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
/// Flag unresolved `BuySubmitted` rows past this age for manual review.
const BUY_SUBMITTED_REVIEW: Duration = Duration::from_secs(600);
/// After a Fatal/unresolved ExitFailed redrive, skip this many reaper ticks
/// before trying again (no Helius spam).
const EXIT_FAILED_BACKOFF_TICKS: u8 = 5;

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
        let mut exit_failed_backoff: HashMap<Uuid, u8> = HashMap::new();
        // Do NOT consume the immediate first tick — boot sweep runs right away.
        loop {
            tick.tick().await;
            redrive_orphaned_buy_submitted(&deps, &mut stuck_rpc_skip).await;
            orphan_exit::reconcile_externally_cleared_holdings(&deps.orphan_deps()).await;
            redrive_orphaned_exit_pending(&deps).await;
            redrive_exit_failed_bags(&deps, &mut exit_failed_backoff).await;
            for mode in ["real", "paper"] {
                match deps.strategy_repo.fail_stale_exit_pending(mode, EXIT_PENDING_STALE).await {
                    Ok(n) if n > 0 => info!(mode, n, "reaper: failed stale ExitPending rows"),
                    Ok(_) => {}
                    Err(e) => warn!(mode, "reaper: fail_stale_exit_pending: {e}"),
                }
                match deps.strategy_repo.delete_stale_unentered(mode, UNENTERED_STALE).await {
                    Ok(n) if n > 0 => info!(mode, n, "reaper: deleted stale Arming rows"),
                    Ok(_) => {}
                    Err(e) => warn!(mode, "reaper: delete_stale_unentered: {e}"),
                }
            }
        }
    })
}

/// How many reaper ticks to skip `signature_state` RPC for a stuck row past
/// [`BUY_SUBMITTED_REVIEW`] (feed-adopt still runs every tick). 10 × 60s ≈ 10 min.
const STUCK_RPC_SKIP_TICKS: u8 = 10;

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
    let wallet = deps.trader.wallet_pubkey();
    let live_ids: std::collections::HashSet<Uuid> = submitted.iter().map(|p| p.id).collect();
    stuck_rpc_skip.retain(|id, _| live_ids.contains(id));

    for position in submitted {
        if deps.inflight.entry_held(position.id) {
            continue;
        }
        let Some(_guard) = deps.inflight.try_begin_entry(position.id) else {
            continue;
        };

        // 1. Adopt: any submitted sig already indexed → record the entry.
        let mut adopted = false;
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
                        adopted = true;
                        stuck_rpc_skip.remove(&position.id);
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
                        break;
                    }
                    Err(e) => warn!(
                        position_id = %position.id,
                        "reaper: record_entry_fill failed: {e}"
                    ),
                }
            }
        }
        if adopted {
            continue;
        }

        if position.submitted_buy_signatures.is_empty() {
            warn!(
                position_id = %position.id,
                mint = %position.mint_address,
                "reaper: BuySubmitted has no signatures — leaving for review"
            );
            continue;
        }

        let age = Utc::now().signed_duration_since(position.updated_at);
        let past_review =
            age > chrono::Duration::from_std(BUY_SUBMITTED_REVIEW).unwrap_or_default();
        if past_review {
            match stuck_rpc_skip.get_mut(&position.id) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    continue;
                }
                _ => {
                    warn!(
                        position_id = %position.id,
                        mint = %position.mint_address,
                        "reaper: BuySubmitted unresolved past review window — manual review \
                         (throttling signature_state RPC)"
                    );
                    stuck_rpc_skip.insert(position.id, STUCK_RPC_SKIP_TICKS);
                }
            }
        }

        // 2. Drop ONLY if EVERY submitted sig is a confirmed revert.
        let mut all_reverted = true;
        for sig in &position.submitted_buy_signatures {
            let status = deps.trader.signature_state(sig).await;
            if exec_real::classify_submitted_buy(&status) == BuyRecoveryVerdict::Wait {
                all_reverted = false;
                break;
            }
        }

        if all_reverted {
            stuck_rpc_skip.remove(&position.id);
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
                }
                Err(err) => warn!(
                    position_id = %position.id,
                    "reaper: delete BuySubmitted failed: {err}"
                ),
            }
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

        match orphan_exit::spawn_orphan_sell(&orphan, position, "Recovery") {
            OrphanStart::Started => {}
            OrphanStart::Busy | OrphanStart::NothingToSell => {}
        }
    }
}

/// Re-drive real `ExitFailed` rows that still have a bag (PG net > dust).
async fn redrive_exit_failed_bags(deps: &ReaperDeps, backoff: &mut HashMap<Uuid, u8>) {
    let failed = match deps
        .strategy_repo
        .find_exit_failed_with_bag(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load ExitFailed-with-bag failed: {err}");
            return;
        }
    };
    let live_ids: std::collections::HashSet<Uuid> = failed.iter().map(|p| p.id).collect();
    backoff.retain(|id, _| live_ids.contains(id));

    // Also heal ExitFailed whose bag is already gone (book End, no sell / no RPC).
    // Queried inversely: load candidates with bag above; for any ExitFailed not in
    // that set we don't scan here — cleared-Holding reaper covers Holding-only.
    // ExitFailed with net<=0: find via a cheap pass on the failed list? The query
    // only returns with-bag. Separate heal:
    heal_exit_failed_cleared(deps).await;

    let orphan = deps.orphan_deps();
    for position in failed {
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
        match orphan_exit::spawn_orphan_sell(&orphan, position.clone(), "Recovery") {
            OrphanStart::Started => {
                // Back off regardless of outcome so we don't re-spam every 60s while
                // the sell task runs / after Fatal.
                backoff.insert(position.id, EXIT_FAILED_BACKOFF_TICKS);
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

/// ExitFailed rows whose PG net is already ≤ dust → book End (no sell, no RPC).
async fn heal_exit_failed_cleared(deps: &ReaperDeps) {
    let cleared = match deps
        .strategy_repo
        .find_exit_failed_cleared(BAG_CLEARED_THRESHOLD_RAW)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("reaper: find_exit_failed_cleared failed: {e}");
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
        let fill = orphan_exit::fill_from_latest_sell(&deps.trade_repo, &wallet, &pos).await;
        if orphan_exit::book_externally_cleared(&orphan, &pos, fill).await.is_ok() {
            n += 1;
        }
    }
    if n > 0 {
        info!(n, "reaper: booked cleared ExitFailed → End");
    }
}
