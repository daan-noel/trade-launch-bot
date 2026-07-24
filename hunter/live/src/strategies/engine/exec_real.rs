//! Real execution — turns a `SubmitBuy`/`SubmitSell` effect into an on-chain
//! submit-and-return, then **synthesizes** a `FillConfirmed`/`FillFailed` event
//! from the **trades feed** (our wallet's own legs), with an RPC confirmation
//! **watchdog** as the timeout fallback.
//!
//! Double-fire safety (the crate's cardinal rule):
//! the engine's `FillFailed` handling *resubmits*, so this adapter must only emit
//! `FillFailed::Reverted` when re-submitting is safe — i.e. the buy was never
//! signed/sent, or a confirmed on-chain revert classified as retryable by
//! `pump_trader::classify_swap_revert`. Structural reverts emit `Fatal`. When an
//! outcome is genuinely ambiguous (submitted, neither a feed fill nor an
//! on-chain revert), it emits **nothing** and leaves the durable
//! `BuySubmitted`/`ExitPending` row for the recovery reaper — never a speculative
//! resubmit.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use pump_trader::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute};

use trading_core::models::trade::TradeType;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::{SigLegs, TradeRepo};

use crate::trader::{PumpFunTrader, SigStatus};

use super::orphan_exit;
use super::{FillSigStore, FillSigs, InFlightGuards, PositionRegistry, SubmittedBuyJournal};

/// How long to wait for the entry fill on the feed before the RPC watchdog fires
/// (buffers the feed's index lag — mirrors the old 12 × 1 s window).
const ENTRY_FEED_WINDOW: Duration = Duration::from_secs(12);
/// Bound on the write-ahead `mark_buy_submitted` PG persist inside the nonce
/// slot (audit M2). The in-memory [`SubmittedBuyJournal`] is set synchronously
/// first, so a slow/wedged DB never holds the send — recovery for this process
/// still sees the sig; the reaper heals cross-process via PG when the write
/// eventually lands (or the row stays BuySubmitted without the sig and the
/// wallet-reconcile backstop flags it).
/// Background persist budget (send is never blocked). Sized for a few short
/// retries while Pass-1 `insert_position` lands.
const MARK_BUY_SUBMITTED_TIMEOUT: Duration = Duration::from_millis(400);
/// Extended feed poll after the RPC says the buy *landed* but the feed hasn't
/// indexed it yet.
const EXTENDED_FEED_WINDOW: Duration = Duration::from_secs(20);
/// Feed re-poll cadence while waiting.
const FEED_POLL: Duration = Duration::from_millis(500);
/// Min gap between sell-leg balance queries (dump can fire many feed wakeups).
const SELL_BALANCE_QUERY_MIN_INTERVAL: Duration = Duration::from_millis(250);
/// Sell attempts inside one `SubmitSell` (escalating Jito tip); classify/heal
/// runs between attempts. The engine adds bounded outer retries for safe Reverted.
const SELL_ATTEMPTS: u8 = 6;
/// Per-attempt sell confirm window before classifying the sent sig.
const SELL_CONFIRM_WINDOW: Duration = Duration::from_secs(5);
/// Hard cap on the RPC send itself (nonce/blockhash build + fan-out).
const SELL_SEND_TIMEOUT: Duration = Duration::from_secs(15);
/// Extended poll when sell status is Succeeded/Pending (never re-send).
const SELL_UNCONFIRMED_EXTENDED: Duration = Duration::from_secs(20);
/// Dust threshold (raw units) below which the remaining balance counts as cleared.
const PARTIAL_FILL_THRESHOLD: u64 = 0;

/// Decision for a snipe buy that was sent but whose fill never appeared in the
/// feed within the poll window. Funnel through `classify_swap_revert` (curve buy
/// — snipes are curve-only) so a futile revert gives up instead of re-paying fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentSendOutcome {
    Resend,
    RefreshCreatorThenResend,
    WaitThenSettle,
    GiveUp,
}

fn classify_silent_send<E>(status: &Result<SigStatus, E>) -> SilentSendOutcome {
    match status {
        Ok(SigStatus::Reverted { custom }) => {
            match classify_swap_revert(*custom, SwapRoute::Curve, SwapDirection::Buy) {
                SwapRetryDecision::Retry => SilentSendOutcome::Resend,
                SwapRetryDecision::RefreshCreator => SilentSendOutcome::RefreshCreatorThenResend,
                _ => SilentSendOutcome::GiveUp,
            }
        }
        Ok(SigStatus::Succeeded) => SilentSendOutcome::WaitThenSettle,
        Ok(SigStatus::Pending) | Err(_) => SilentSendOutcome::GiveUp,
    }
}

/// Recovery verdict for a `BuySubmitted` row — reaper never re-sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuyRecoveryVerdict {
    Drop,
    Wait,
}

/// Classify one submitted buy signature's on-chain status for recovery.
pub(crate) fn classify_submitted_buy<E>(status: &Result<Option<bool>, E>) -> BuyRecoveryVerdict {
    match status {
        Ok(Some(false)) => BuyRecoveryVerdict::Drop,
        Ok(Some(true)) | Ok(None) | Err(_) => BuyRecoveryVerdict::Wait,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellConfirmAction {
    Reclassify(SwapRetryDecision),
    WaitConfirm,
}

fn classify_sell_confirm<E>(
    state: &Result<SigStatus, E>,
    used_migrated: bool,
    now_migrated: bool,
) -> SellConfirmAction {
    match state {
        Ok(SigStatus::Reverted { custom }) if used_migrated == now_migrated => {
            let route = if used_migrated { SwapRoute::Amm } else { SwapRoute::Curve };
            SellConfirmAction::Reclassify(classify_swap_revert(*custom, route, SwapDirection::Sell))
        }
        Ok(SigStatus::Reverted { .. }) => SellConfirmAction::Reclassify(SwapRetryDecision::Retry),
        Ok(SigStatus::Succeeded) | Ok(SigStatus::Pending) | Err(_) => SellConfirmAction::WaitConfirm,
    }
}

/// The dependencies a real submit needs — cheap Arc-backed clones.
#[derive(Clone)]
pub struct RealExecDeps {
    pub trader: Arc<PumpFunTrader>,
    pub token_cache: Arc<TokenCache>,
    pub trade_repo: TradeRepo,
    pub strategy_repo: StrategyRepo,
    pub trade_signals: Arc<TradeSignals>,
    pub fill_sigs: FillSigStore,
    pub fill_tx: mpsc::Sender<Event>,
    pub inflight: InFlightGuards,
    /// Process-local signed-buy journal (M2) — sync before send; PG is best-effort.
    pub buy_journal: SubmittedBuyJournal,
    /// Live position registry — sibling ExternallyCleared after a mint clears.
    pub registry: PositionRegistry,
    /// Engine event channel (same as the decision loop's fill_tx). When set,
    /// successful mint clears fold `ExternallyCleared` for siblings still in-engine.
    /// Orphan sells use a private fill_tx for their own outcome, so they pass the
    /// loop channel here separately.
    pub engine_fill_tx: Option<mpsc::Sender<Event>>,
}

/// Everything a buy submit needs, resolved by the loop from the cache + rule.
pub struct BuyOrder {
    pub intent: IntentId,
    pub pg_id: Uuid,
    pub mint: String,
    pub creator: String,
    pub token_program_id: String,
    pub lamports: u64,
    pub cashback_enabled: bool,
    pub slippage_bps: Option<u64>,
}

/// Everything a sell submit needs, resolved by the loop from the position meta.
pub struct SellOrder {
    pub intent: IntentId,
    pub pg_id: Uuid,
    pub mint: String,
    pub token_amount: u64,
    pub token_account: Option<String>,
    pub creator: Option<String>,
    pub token_program_id: Option<String>,
    pub cashback_enabled: bool,
    pub slippage_bps: Option<u64>,
}

/// Submit a real buy, then confirm the fill off the feed (RPC watchdog fallback),
/// emitting the definitive `FillConfirmed`/`FillFailed` back to the engine.
pub async fn run_entry(deps: RealExecDeps, order: BuyOrder) {
    let Some(_guard) = deps.inflight.try_begin_entry(order.pg_id) else {
        warn!(pg = %order.pg_id, mint = %order.mint, "real buy: entry guard held — skipping");
        return;
    };
    let wallet = deps.trader.wallet_pubkey();
    let sol_amount = order.lamports as f64 / 1_000_000_000.0;

    // SOL earmark — released on Fatal / sink terminal / sell-start / reaper drop.
    deps.trader.commit_sol_for_position(order.pg_id.to_string(), order.lamports);

    // Adopt a fill from signatures this position already submitted (engine retry
    // or crash between sign and confirm) before sending again. First attempt
    // (empty journal) skips the PG round-trip entirely.
    if let Some(legs) = adopt_existing_fill(&deps, &wallet, &order).await {
        emit_entry_filled(&deps, &order, legs.0, legs.1).await;
        return;
    }

    let guard_sig = deps.trade_signals.register(&wallet, &order.mint);
    let reserves = reserves_from_cache(&deps.token_cache, &order.mint);

    let signed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let on_signed = {
        let signed = signed.clone();
        let journal = deps.buy_journal.clone();
        let repo = deps.strategy_repo.clone();
        let pg = order.pg_id;
        let mint = order.mint.clone();
        Box::new(move |sig: String| {
            // Sync write-ahead (M2): process-local truth before any await so the
            // nonce slot is never held on PG. Durable persist is fire-and-forget
            // with a 250 ms bound (slow DB → warn + drop; journal + reaper cover).
            *signed.lock().unwrap() = Some(sig.clone());
            journal.append(pg, sig.clone());
            let repo = repo.clone();
            let mint = mint.clone();
            let sig_bg = sig.clone();
            tokio::spawn(async move {
                // Fire-and-forget: never blocks send. Retry briefly when the
                // background insert_position hasn't landed yet (Pass-1 async).
                let persist = async {
                    let mut attempt = 0u8;
                    loop {
                        match repo.mark_buy_submitted(pg, &sig_bg).await {
                            Ok(Some(_)) => return Ok(()),
                            Ok(None) => {
                                attempt = attempt.saturating_add(1);
                                if attempt >= 8 {
                                    return Err(
                                        "row missing or already Holding after retries".into(),
                                    );
                                }
                                tokio::time::sleep(Duration::from_millis(25)).await;
                            }
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                };
                match tokio::time::timeout(MARK_BUY_SUBMITTED_TIMEOUT, persist).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!(
                        mint = %mint, pg = %pg, sig = %sig_bg,
                        "mark_buy_submitted failed (journal kept): {e}"
                    ),
                    Err(_) => warn!(
                        mint = %mint, pg = %pg, sig = %sig_bg,
                        "mark_buy_submitted timed out after {}ms (journal kept)",
                        MARK_BUY_SUBMITTED_TIMEOUT.as_millis()
                    ),
                }
            });
            Box::pin(async {}) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        })
    };

    // Prior submitted sigs (engine Retry after a confirmed safe revert) climb
    // the tip ladder. Pending/ambiguous outcomes never resend here — that would
    // risk a double-buy if the first tx is still in flight.
    let tip_level = deps.buy_journal.sigs(order.pg_id).len().min(u8::MAX as usize) as u8;

    let submit = deps
        .trader
        .buy_token_snipe_write_ahead(
            &order.mint,
            &order.creator,
            &order.token_program_id,
            sol_amount,
            order.slippage_bps,
            reserves,
            on_signed,
            order.cashback_enabled,
            None,
            tip_level,
        )
        .await;

    let sig = signed.lock().unwrap().clone();
    match (submit, sig) {
        (Ok(sig), _) | (Err(_), Some(sig)) => {
            let outcome =
                confirm_entry(&deps, &guard_sig, &wallet, &order.mint, &sig, ENTRY_FEED_WINDOW)
                    .await;
            emit_entry_outcome(&deps, &order, sig, outcome).await;
        }
        (Err(e), None) => {
            warn!(mint = %order.mint, "real buy never signed ({e}); reporting revert for retry");
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent,
                    reason: FillFailReason::Reverted,
                })
                .await;
        }
    }
}

/// Try to adopt a fill already indexed for this position's submitted signatures.
/// Uses the process-local journal only — empty ⇒ first attempt, skip PG. Cross-
/// process orphans are the reaper's job (`find_all_buy_submitted`).
async fn adopt_existing_fill(
    deps: &RealExecDeps,
    wallet: &str,
    order: &BuyOrder,
) -> Option<(String, SigLegs)> {
    let sigs = deps.buy_journal.sigs(order.pg_id);
    if sigs.is_empty() {
        return None;
    }
    for sig in &sigs {
        if let Some(obs) = deps.trade_signals.observed_legs(sig) {
            if obs.token_amount > PARTIAL_FILL_THRESHOLD {
                info!(mint = %order.mint, sig = %sig, "adopted own-leg preview before re-send");
                return Some((
                    sig.clone(),
                    SigLegs {
                        token_amount: obs.token_amount,
                        amount_sol: obs.amount_sol,
                        first_block_time: obs.first_block_time,
                        last_block_time: obs.last_block_time,
                    },
                ));
            }
        }
        if let Ok(Some(legs)) = deps.trade_repo.find_fill_by_signature(wallet, &order.mint, sig).await
        {
            if legs.token_amount > PARTIAL_FILL_THRESHOLD {
                info!(mint = %order.mint, sig = %sig, "adopted existing buy fill before re-send");
                return Some((sig.clone(), legs));
            }
        }
    }
    None
}

async fn emit_entry_filled(deps: &RealExecDeps, order: &BuyOrder, sig: String, legs: SigLegs) {
    let token_account = deps.trader.cached_token_account(&order.mint);
    deps.fill_sigs
        .put(order.intent.clone(), FillSigs { sigs: vec![sig], token_account });
    let _ = deps
        .fill_tx
        .send(Event::FillConfirmed {
            intent: order.intent.clone(),
            fill: Fill {
                price: legs.price_per_token(),
                sol: legs.amount_sol,
                token_amount: legs.token_amount,
                at: legs.last_block_time,
            },
        })
        .await;
}

async fn emit_entry_outcome(
    deps: &RealExecDeps,
    order: &BuyOrder,
    sig: String,
    outcome: EntryOutcome,
) {
    match outcome {
        EntryOutcome::Filled(legs) => emit_entry_filled(deps, order, sig, legs).await,
        EntryOutcome::Retry => {
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent.clone(),
                    reason: FillFailReason::Reverted,
                })
                .await;
        }
        EntryOutcome::Fatal => {
            deps.trader.release_sol_for_position(&order.pg_id.to_string());
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent.clone(),
                    reason: FillFailReason::Fatal,
                })
                .await;
        }
        // Ambiguous: leave BuySubmitted for the reaper — never resend.
        EntryOutcome::Ambiguous => {
            warn!(mint = %order.mint, "real buy outcome ambiguous — left BuySubmitted for the reaper");
        }
    }
}

enum EntryOutcome {
    Filled(SigLegs),
    Retry,
    Fatal,
    Ambiguous,
}

async fn confirm_entry(
    deps: &RealExecDeps,
    guard: &trading_core::state::trade_signals::WaitGuard,
    wallet: &str,
    mint: &str,
    sig: &str,
    window: Duration,
) -> EntryOutcome {
    if let Some(legs) = poll_feed_buy(deps, wallet, mint, sig, guard, window).await {
        return EntryOutcome::Filled(legs);
    }
    let status = deps.trader.signature_state_detailed(sig).await;
    match classify_silent_send(&status) {
        SilentSendOutcome::Resend => EntryOutcome::Retry,
        SilentSendOutcome::RefreshCreatorThenResend => {
            match deps.trader.refresh_curve_creator_vault(mint).await {
                Ok(Some(vault)) => {
                    warn!(mint = %mint, new_creator_vault = %vault,
                        "buy reverted 2006; refreshed creator — reporting retry");
                    EntryOutcome::Retry
                }
                Ok(None) => {
                    warn!(mint = %mint, "buy reverted 2006 but creator unchanged — giving up");
                    EntryOutcome::Fatal
                }
                Err(e) => {
                    warn!(mint = %mint, "buy 2006 creator refresh failed ({e}) — giving up");
                    EntryOutcome::Fatal
                }
            }
        }
        SilentSendOutcome::WaitThenSettle => {
            match poll_feed_buy(deps, wallet, mint, sig, guard, EXTENDED_FEED_WINDOW).await {
                Some(legs) => EntryOutcome::Filled(legs),
                None => EntryOutcome::Ambiguous,
            }
        }
        SilentSendOutcome::GiveUp => match &status {
            // Pending/unknown → ambiguous (nonce may still land); structural revert → Fatal.
            Ok(SigStatus::Pending) | Err(_) => EntryOutcome::Ambiguous,
            Ok(SigStatus::Reverted { .. }) => EntryOutcome::Fatal,
            Ok(SigStatus::Succeeded) => EntryOutcome::Ambiguous,
        },
    }
}

async fn poll_feed_buy(
    deps: &RealExecDeps,
    wallet: &str,
    mint: &str,
    sig: &str,
    guard: &trading_core::state::trade_signals::WaitGuard,
    window: Duration,
) -> Option<SigLegs> {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        // Prefer process-local own-leg preview (ingest saw our buy before PG).
        if let Some(obs) = deps.trade_signals.observed_legs(sig) {
            if obs.token_amount > PARTIAL_FILL_THRESHOLD {
                return Some(SigLegs {
                    token_amount: obs.token_amount,
                    amount_sol: obs.amount_sol,
                    first_block_time: obs.first_block_time,
                    last_block_time: obs.last_block_time,
                });
            }
        }
        if let Ok(Some(legs)) = deps.trade_repo.find_fill_by_signature(wallet, mint, sig).await {
            if legs.token_amount > PARTIAL_FILL_THRESHOLD {
                return Some(legs);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::select! {
            _ = guard.notified() => {}
            _ = tokio::time::sleep(FEED_POLL) => {}
        }
    }
}

/// Submit a real sell (escalating tip across attempts), confirm by summing the
/// position's OWN sell legs vs the held amount, and emit the definitive outcome.
pub async fn run_exit(deps: RealExecDeps, order: SellOrder) {
    let Some(_guard) = deps.inflight.try_begin_exit(order.pg_id) else {
        warn!(pg = %order.pg_id, mint = %order.mint, "real sell: exit guard held — skipping");
        return;
    };
    // One sell per mint (shared ATA) — siblings wait for reaper/nudge rather than
    // fanning out parallel Helius sell sends.
    let Some(_mint_guard) = deps.inflight.try_begin_exit_mint(&order.mint) else {
        warn!(pg = %order.pg_id, mint = %order.mint, "real sell: mint exit lock held — skipping");
        return;
    };

    // Release FIRST — must fire even if the process crashes mid-exit.
    deps.trader.release_sol_for_position(&order.pg_id.to_string());

    if order.token_amount == 0 {
        // Nothing to sell — treat as cleared at zero.
        let _ = deps
            .fill_tx
            .send(Event::FillConfirmed {
                intent: order.intent.clone(),
                fill: Fill { price: 0.0, sol: 0.0, token_amount: 0, at: chrono::Utc::now() },
            })
            .await;
        return;
    }

    let wallet = deps.trader.wallet_pubkey();
    let feed_guard = deps.trade_signals.register(&wallet, &order.mint);
    let mut sell_sigs: Vec<String> = Vec::new();
    let mut cashback = order.cashback_enabled;
    let base_program = order.token_program_id.clone().unwrap_or_default();

    for attempt in 0..SELL_ATTEMPTS {
        let is_migrated =
            deps.token_cache.get(&order.mint).map(|e| e.value().is_migrated).unwrap_or(false);

        let submit = {
            let send = submit_one_sell(&deps, &order, attempt, is_migrated, cashback);
            match tokio::time::timeout(SELL_SEND_TIMEOUT, send).await {
                Ok(r) => r,
                Err(_) => {
                    warn!(mint = %order.mint, attempt, "sell send timed out");
                    Err(pump_trader::TradeError::Other("sell send timed out".into()))
                }
            }
        };

        let sig = match submit {
            Ok(Some(sig)) => {
                sell_sigs.push(sig.clone());
                Some(sig)
            }
            Ok(None) => None,
            Err(e) => {
                warn!(mint = %order.mint, attempt, "real sell attempt failed: {e}");
                None
            }
        };

        if let Some(legs) =
            confirm_sell(&deps, &feed_guard, &wallet, &order, &sell_sigs, SELL_CONFIRM_WINDOW).await
        {
            finish_cleared_sell(&deps, &order, &sell_sigs, legs).await;
            return;
        }

        let Some(sig) = sig else {
            continue;
        };

        let now_migrated =
            deps.token_cache.get(&order.mint).map(|e| e.value().is_migrated).unwrap_or(is_migrated);
        let state = deps.trader.signature_state_detailed(&sig).await;
        match classify_sell_confirm(&state, is_migrated, now_migrated) {
            SellConfirmAction::WaitConfirm => {
                if let Some(legs) = confirm_sell(
                    &deps,
                    &feed_guard,
                    &wallet,
                    &order,
                    &sell_sigs,
                    SELL_UNCONFIRMED_EXTENDED,
                )
                .await
                {
                    finish_cleared_sell(&deps, &order, &sell_sigs, legs).await;
                    return;
                }
                info!(mint = %order.mint, "sell unconfirmed after extended poll — not re-sending");
                let _ = deps
                    .fill_tx
                    .send(Event::FillFailed {
                        intent: order.intent.clone(),
                        reason: FillFailReason::Unconfirmed,
                    })
                    .await;
                return;
            }
            SellConfirmAction::Reclassify(decision) => {
                match decision {
                    SwapRetryDecision::StopFeeBurn => {
                        warn!(mint = %order.mint, attempt, "sell structural revert — Fatal");
                        let _ = deps
                            .fill_tx
                            .send(Event::FillFailed {
                                intent: order.intent.clone(),
                                reason: FillFailReason::Fatal,
                            })
                            .await;
                        return;
                    }
                    SwapRetryDecision::RefreshCreator => {
                        match deps.trader.refresh_curve_creator_vault(&order.mint).await {
                            Ok(Some(_)) => {}
                            Ok(None) | Err(_) => {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillFailed {
                                        intent: order.intent.clone(),
                                        reason: FillFailReason::Fatal,
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RefreshCoinCreator => {
                        match deps.trader.refresh_amm_pool_info(&order.mint, &base_program).await {
                            Ok(Some(_)) => {}
                            Ok(None) | Err(_) => {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillFailed {
                                        intent: order.intent.clone(),
                                        reason: FillFailReason::Fatal,
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RefreshCashback => {
                        match deps.trader.refresh_curve_facts(&order.mint).await {
                            Ok(facts) => {
                                cashback = facts.cashback_enabled;
                                if let Some(mut e) = deps.token_cache.get_mut(&order.mint) {
                                    e.token.is_cashback_enabled = facts.cashback_enabled;
                                }
                            }
                            Err(_) => {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillFailed {
                                        intent: order.intent.clone(),
                                        reason: FillFailReason::Fatal,
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::RerouteMigrated => {
                        match deps.trader.refresh_curve_facts(&order.mint).await {
                            Ok(facts) if facts.is_migrated => {
                                if let Some(mut e) = deps.token_cache.get_mut(&order.mint) {
                                    e.is_migrated = true;
                                }
                            }
                            _ => {
                                let _ = deps
                                    .fill_tx
                                    .send(Event::FillFailed {
                                        intent: order.intent.clone(),
                                        reason: FillFailReason::Fatal,
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                    SwapRetryDecision::Retry => {
                        // tip escalates on next attempt
                    }
                }
            }
        }
    }

    let reason = if sell_sigs.is_empty() {
        FillFailReason::Reverted
    } else {
        FillFailReason::Unconfirmed
    };
    info!(mint = %order.mint, ?reason, "real sell unresolved after {SELL_ATTEMPTS} attempts");
    let _ = deps.fill_tx.send(Event::FillFailed { intent: order.intent, reason }).await;
}

async fn finish_cleared_sell(
    deps: &RealExecDeps,
    order: &SellOrder,
    sell_sigs: &[String],
    legs: SigLegs,
) {
    let token_account =
        order.token_account.clone().or_else(|| deps.trader.cached_token_account(&order.mint));
    deps.fill_sigs.put(
        order.intent.clone(),
        FillSigs { sigs: sell_sigs.to_vec(), token_account },
    );
    let fill = Fill {
        price: legs.price_per_token(),
        sol: legs.amount_sol,
        token_amount: legs.token_amount,
        at: legs.last_block_time,
    };
    let _ = deps
        .fill_tx
        .send(Event::FillConfirmed {
            intent: order.intent.clone(),
            fill,
        })
        .await;

    // Sibling book-close when the wallet mint bag is gone (PG net — no RPC).
    let wallet = deps.trader.wallet_pubkey();
    let engine_tx = deps.engine_fill_tx.clone().unwrap_or_else(|| deps.fill_tx.clone());
    let _ = orphan_exit::close_siblings_if_mint_cleared(
        &deps.strategy_repo,
        &deps.trade_repo,
        &deps.registry,
        &engine_tx,
        &wallet,
        &order.mint,
        order.pg_id,
        &fill,
    )
    .await;

    // Fire-and-forget rent reclaim when no sibling still shares the account.
    let trader = deps.trader.clone();
    let repo = deps.strategy_repo.clone();
    let mint = order.mint.clone();
    let pg = order.pg_id;
    tokio::spawn(async move {
        reclaim_token_account_if_last(&trader, &repo, &wallet, &mint, "real", pg).await;
    });
}

/// Reclaim rent only when no OTHER open position shares `(wallet, mint)` (M1).
pub(crate) async fn reclaim_token_account_if_last(
    trader: &Arc<PumpFunTrader>,
    repo: &StrategyRepo,
    wallet: &str,
    mint: &str,
    mode: &str,
    exclude_position: Uuid,
) {
    match repo.has_other_open_position_on_mint(wallet, mint, mode, exclude_position).await {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            warn!(mint = %mint, "rent-reclaim other-open check failed; deferring: {err}");
            return;
        }
    }
    if let Err(err) = trader.close_token_account(mint, None).await {
        tracing::debug!(mint = %mint, "rent-reclaim close skipped: {err}");
    }
}

async fn submit_one_sell(
    deps: &RealExecDeps,
    order: &SellOrder,
    attempt: u8,
    is_migrated: bool,
    cashback: bool,
) -> pump_trader::Result<Option<String>> {
    if is_migrated {
        deps.trader
            .amm_sell(
                &order.mint,
                order.token_amount,
                order.token_program_id.as_deref().unwrap_or_default(),
                None,
                order.token_account.as_deref(),
                order.slippage_bps,
                attempt,
                false,
            )
            .await
    } else {
        // Cache reserves for min_out when sell slippage is configured — mirrors
        // snipe buy; avoids a cold `curve_reserves` RPC on the exit hot path.
        let reserves = reserves_from_cache(&deps.token_cache, &order.mint);
        deps.trader
            .sell_token_once(
                &order.mint,
                order.token_amount,
                order.creator.as_deref(),
                cashback,
                order.token_account.as_deref(),
                order.slippage_bps,
                attempt,
                false,
                reserves,
            )
            .await
    }
}

async fn confirm_sell(
    deps: &RealExecDeps,
    guard: &trading_core::state::trade_signals::WaitGuard,
    wallet: &str,
    order: &SellOrder,
    sell_sigs: &[String],
    window: Duration,
) -> Option<SigLegs> {
    if sell_sigs.is_empty() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + window;
    let mut last_query: Option<tokio::time::Instant> = None;
    loop {
        let now = tokio::time::Instant::now();
        let at_deadline = now >= deadline;
        let rate_ok =
            at_deadline || last_query.map_or(true, |t| now.duration_since(t) >= SELL_BALANCE_QUERY_MIN_INTERVAL);
        if rate_ok {
            last_query = Some(now);
            // Prefer process-local own-leg preview before SQL.
            if let Some(obs) = deps.trade_signals.sum_observed_legs(sell_sigs) {
                if obs.token_amount.saturating_add(PARTIAL_FILL_THRESHOLD) >= order.token_amount {
                    return Some(SigLegs {
                        token_amount: obs.token_amount,
                        amount_sol: obs.amount_sol,
                        first_block_time: obs.first_block_time,
                        last_block_time: obs.last_block_time,
                    });
                }
            }
            if let Ok(Some(legs)) = deps
                .trade_repo
                .sum_legs_by_signatures(wallet, &order.mint, sell_sigs, TradeType::Sell)
                .await
            {
                if legs.token_amount.saturating_add(PARTIAL_FILL_THRESHOLD) >= order.token_amount {
                    return Some(legs);
                }
            }
        }
        if at_deadline {
            return None;
        }
        tokio::select! {
            _ = guard.notified() => {}
            _ = tokio::time::sleep(FEED_POLL) => {}
        }
    }
}

/// Slippage `min_out` reserves for the snipe buy, read from the cache (no RPC).
fn reserves_from_cache(token_cache: &Arc<TokenCache>, mint: &str) -> Option<(u128, u128)> {
    let (reserve_token, reserve_sol) = token_cache
        .get(mint)
        .map(|e| {
            let s = e.value();
            (s.current_reserve_token, s.current_reserve_sol)
        })
        .unwrap_or((None, None));
    snipe_reserves_from_cache(reserve_token, reserve_sol)
}

/// Convert the token cache's in-memory reserve pair into the snipe buy's
/// `(virtual_token, virtual_quote=lamports)` pair.
pub(crate) fn snipe_reserves_from_cache(
    reserve_token: Option<f64>,
    reserve_sol: Option<f64>,
) -> Option<(u128, u128)> {
    let vt = reserve_token?;
    let vsol = reserve_sol?;
    if vt <= 0.0 || vsol <= 0.0 {
        return None;
    }
    let vq_lamports = vsol * pump_trader::constants::LAMPORTS_PER_SOL as f64;
    Some((vt as u128, vq_lamports as u128))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_silent_send_buy_slippage_resends() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6002) });
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::Resend);
    }

    #[test]
    fn classify_silent_send_2006_refreshes() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(2006) });
        assert_eq!(
            classify_silent_send(&status),
            SilentSendOutcome::RefreshCreatorThenResend
        );
    }

    #[test]
    fn classify_silent_send_structural_gives_up() {
        let status: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6022) });
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn classify_silent_send_succeeded_waits() {
        let status: Result<SigStatus, ()> = Ok(SigStatus::Succeeded);
        assert_eq!(classify_silent_send(&status), SilentSendOutcome::WaitThenSettle);
    }

    #[test]
    fn classify_submitted_buy_only_drops_proven_revert() {
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(Some(false))),
            BuyRecoveryVerdict::Drop
        );
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(Some(true))),
            BuyRecoveryVerdict::Wait
        );
        assert_eq!(
            classify_submitted_buy::<()>(&Ok(None)),
            BuyRecoveryVerdict::Wait
        );
    }

    #[test]
    fn classify_sell_confirm_route_change_retries_on_revert() {
        let state: Result<SigStatus, ()> =
            Ok(SigStatus::Reverted { custom: Some(6003) });
        assert_eq!(
            classify_sell_confirm(&state, false, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::Retry)
        );
    }

    #[test]
    fn classify_sell_confirm_succeeded_pending_rpc_error_wait_never_resend() {
        // C1: Succeeded / Pending / RPC-error must never trigger a second sell.
        let succeeded: Result<SigStatus, ()> = Ok(SigStatus::Succeeded);
        let pending: Result<SigStatus, ()> = Ok(SigStatus::Pending);
        let rpc_err: Result<SigStatus, ()> = Err(());
        assert_eq!(
            classify_sell_confirm(&succeeded, false, false),
            SellConfirmAction::WaitConfirm
        );
        assert_eq!(
            classify_sell_confirm(&pending, true, true),
            SellConfirmAction::WaitConfirm
        );
        assert_eq!(
            classify_sell_confirm(&rpc_err, false, false),
            SellConfirmAction::WaitConfirm
        );
    }
}
