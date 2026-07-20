//! Recovery reaper — safety backstop for `strategy_positions` rows the engine
//! loop can't resolve on its own:
//! - `BuySubmitted` with no live entry task → adopt / wait / drop (never re-send)
//! - `ExitPending` with no live exit task → re-drive sell (or nudge the engine)
//! - stale `ExitPending` / `Arming` cleanup
//!
//! Fires once at boot (immediate tick) then every 60 s. Skips rows whose
//! [`InFlightGuards`] slot is held so it never races a live buy/sell task.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use hunter_engine::event::{Event, Fill, FillFailReason};

use trading_core::config::constants::resolve_sell_slippage_bps;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::settings_repo::AppSettings;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use crate::trader::PumpFunTrader;

use super::exec_real::{self, BuyRecoveryVerdict, RealExecDeps, SellOrder};
use super::{FillSigStore, InFlightGuards, PositionRegistry};

const INTERVAL: Duration = Duration::from_secs(60);
const EXIT_PENDING_STALE: Duration = Duration::from_secs(300);
const UNENTERED_STALE: Duration = Duration::from_secs(600);
/// Flag unresolved `BuySubmitted` rows past this age for manual review.
const BUY_SUBMITTED_REVIEW: Duration = Duration::from_secs(600);

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

/// Spawn the reaper loop (immediate first tick, then every `INTERVAL`).
pub fn spawn_reaper(deps: ReaperDeps) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip counters for stuck BuySubmitted rows past the review window — still
        // try feed-adopt every tick, but don't burn signature_state RPC every 60s.
        let mut stuck_rpc_skip: std::collections::HashMap<uuid::Uuid, u8> =
            std::collections::HashMap::new();
        // Do NOT consume the immediate first tick — boot sweep runs right away.
        loop {
            tick.tick().await;
            redrive_orphaned_buy_submitted(&deps, &mut stuck_rpc_skip).await;
            redrive_orphaned_exit_pending(&deps).await;
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
    stuck_rpc_skip: &mut std::collections::HashMap<uuid::Uuid, u8>,
) {
    let submitted = match deps.strategy_repo.find_all_buy_submitted("real").await {
        Ok(p) => p,
        Err(err) => {
            warn!("reaper: load BuySubmitted failed: {err}");
            return;
        }
    };
    let wallet = deps.trader.wallet_pubkey();
    let live_ids: std::collections::HashSet<uuid::Uuid> =
        submitted.iter().map(|p| p.id).collect();
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
                        // If the engine still tracks this pg id, synthesize FillConfirmed
                        // so TP/SL monitoring resumes.
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
                    // Nudge engine if it still holds the arm.
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
    for position in pending {
        if position.entry_price.is_none() {
            continue;
        }
        if deps.inflight.exit_held(position.id) {
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

        // Post-restart orphan: engine has no intent — sell directly and close the row.
        // Claim the exit guard for the whole spawned task lifetime (moved into task).
        let Some(guard) = deps.inflight.try_begin_exit(position.id) else {
            continue;
        };
        info!(
            position_id = %position.id,
            mint = %position.mint_address,
            "reaper: re-driving orphaned ExitPending sell (direct)"
        );

        let fill_sigs = FillSigStore::new();
        let (fill_tx, mut fill_rx) = mpsc::channel::<Event>(4);
        let real_deps = RealExecDeps {
            trader: deps.trader.clone(),
            token_cache: deps.token_cache.clone(),
            trade_repo: deps.trade_repo.clone(),
            strategy_repo: deps.strategy_repo.clone(),
            trade_signals: deps.trade_signals.clone(),
            fill_sigs,
            fill_tx,
            // Nested run_exit claims on a fresh set so it doesn't deadlock on `guard`.
            inflight: InFlightGuards::new(),
            buy_journal: super::SubmittedBuyJournal::new(),
        };
        let slippage = {
            let s = deps.settings.borrow();
            resolve_sell_slippage_bps(s.sell_slippage_bps, None)
        };
        let intent = hunter_engine::event::IntentId {
            rule: hunter_engine::event::RuleId(position.rule_id.unwrap_or_default()),
            mint: hunter_engine::event::Mint::from(position.mint_address.as_str()),
            seq: 0,
        };
        let order = SellOrder {
            intent,
            pg_id: position.id,
            mint: position.mint_address.clone(),
            token_amount: position.entry_token_amount.unwrap_or(0),
            token_account: position.token_account.clone(),
            creator: None,
            token_program_id: None,
            cashback_enabled: false,
            slippage_bps: slippage,
        };
        let repo = deps.strategy_repo.clone();
        let token_cache = deps.token_cache.clone();
        let exit_reason = position.exit_reason.clone();
        let entry_price = position.entry_price;
        let mint = position.mint_address.clone();
        let pg_id = position.id;

        tokio::spawn(async move {
            let _guard = guard; // held until this task ends
            exec_real::run_exit(real_deps, order).await;
            match fill_rx.recv().await {
                Some(Event::FillConfirmed { fill, .. }) => {
                    if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                        let reason = exit_reason.unwrap_or_else(|| "Recovery".to_string());
                        pos.close(
                            fill.price,
                            fill.sol,
                            fill.token_amount,
                            vec![],
                            fill.at,
                            &reason,
                        );
                        if let Err(e) = repo.update_position(&pos).await {
                            warn!(position_id = %pg_id, "reaper: close after recovery sell failed: {e}");
                        } else {
                            info!(position_id = %pg_id, "reaper: ExitPending recovered → closed");
                        }
                    }
                }
                Some(Event::FillFailed { reason: FillFailReason::Unconfirmed, .. }) => {
                    if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                        let price = token_cache
                            .get(&mint)
                            .and_then(|e| e.value().current_price)
                            .or(entry_price)
                            .unwrap_or(0.0);
                        pos.mark_exit_unconfirmed(price, Utc::now());
                        let _ = repo.update_position(&pos).await;
                    }
                }
                Some(Event::FillFailed { reason: FillFailReason::Fatal, .. }) => {
                    if let Ok(Some(mut pos)) = repo.find_position(pg_id).await {
                        let price = token_cache
                            .get(&mint)
                            .and_then(|e| e.value().current_price)
                            .or(entry_price)
                            .unwrap_or(0.0);
                        pos.mark_exit_failed(price, Utc::now());
                        let _ = repo.update_position(&pos).await;
                    }
                }
                _ => {
                    warn!(
                        position_id = %pg_id,
                        "reaper: direct ExitPending sell unresolved — leaving for next tick"
                    );
                }
            }
        });
    }
}
