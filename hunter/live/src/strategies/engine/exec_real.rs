//! Real execution (plan 4.3) — turns a `SubmitBuy`/`SubmitSell` effect into an
//! on-chain submit-and-return, then **synthesizes** a `FillConfirmed`/`FillFailed`
//! event from the **trades feed** (our wallet's own legs), with an RPC
//! confirmation **watchdog** as the timeout fallback.
//!
//! Double-fire safety (the crate's cardinal rule, preserved from `execution/real`):
//! the engine's `FillFailed` handling *resubmits*, so this adapter must only emit
//! `FillFailed` when re-submitting is safe — i.e. the buy was never signed/sent
//! (`Reverted`), or the sell definitively did not clear. When an outcome is
//! genuinely ambiguous (submitted, neither a feed fill nor an on-chain revert), it
//! emits **nothing** and leaves the durable `BuySubmitted`/`ExitPending` row for
//! the boot-recovery reaper — never a speculative resubmit.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use trading_core::models::trade::TradeType;
use trading_core::state::token_cache::TokenCache;
use trading_core::state::trade_signals::TradeSignals;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::{SigLegs, TradeRepo};

use crate::strategies::execution::real::snipe_reserves_from_cache;
use crate::trader::{PumpFunTrader, SigStatus};

use super::{FillSigStore, FillSigs};

/// How long to wait for the entry fill on the feed before the RPC watchdog fires.
/// Generous — it must buffer the feed's index lag (≈ several slots).
const ENTRY_FEED_WINDOW: Duration = Duration::from_secs(6);
/// Extended feed poll after the RPC says the buy *landed* but the feed hasn't
/// indexed it yet (mirrors the old sell "unconfirmed" extended poll).
const EXTENDED_FEED_WINDOW: Duration = Duration::from_secs(20);
/// Feed re-poll cadence while waiting.
const FEED_POLL: Duration = Duration::from_millis(500);
/// Sell attempts inside one `SubmitSell` (escalating Jito tip); the engine adds
/// its own bounded retries on top for the `Reverted` case.
const SELL_ATTEMPTS: u8 = 3;
/// Per-attempt sell confirm window before escalating the tip / giving up.
const SELL_CONFIRM_WINDOW: Duration = Duration::from_secs(4);
/// Dust threshold (raw units) below which the remaining balance counts as cleared.
const PARTIAL_FILL_THRESHOLD: u64 = 0;

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
    pub mint: String,
    pub token_amount: u64,
    pub token_account: Option<String>,
    pub creator: Option<String>,
    pub token_program_id: Option<String>,
    pub is_migrated: bool,
    pub cashback_enabled: bool,
    pub slippage_bps: Option<u64>,
}

/// Submit a real buy, then confirm the fill off the feed (RPC watchdog fallback),
/// emitting the definitive `FillConfirmed`/`FillFailed` back to the engine.
pub async fn run_entry(deps: RealExecDeps, order: BuyOrder) {
    let wallet = deps.trader.wallet_pubkey();
    let sol_amount = order.lamports as f64 / 1_000_000_000.0;
    // Register the feed wakeup BEFORE submit so we can't miss the fill signal.
    let guard = deps.trade_signals.register(&wallet, &order.mint);
    let reserves = reserves_from_cache(&deps.token_cache, &order.mint);

    // Write-ahead: capture the signed signature (fires before submit) and persist
    // the durable "buy in flight" marker so a crash mid-flight is recoverable.
    let signed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let on_signed = {
        let signed = signed.clone();
        let repo = deps.strategy_repo.clone();
        let pg = order.pg_id;
        Box::new(move |sig: String| {
            *signed.lock().unwrap() = Some(sig.clone());
            let repo = repo.clone();
            Box::pin(async move {
                let _ = repo.mark_buy_submitted(pg, &sig).await;
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        })
    };

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
        )
        .await;

    let sig = signed.lock().unwrap().clone();
    match (submit, sig) {
        // Signed + submitted (or submit errored but a signature exists) → the tx may
        // have landed; resolve it definitively off-chain, never a blind resubmit.
        (Ok(sig), _) | (Err(_), Some(sig)) => {
            let outcome =
                confirm_entry(&deps, &guard, &wallet, &order.mint, &sig, ENTRY_FEED_WINDOW).await;
            emit_entry_outcome(&deps, &order, sig, outcome).await;
        }
        // Never signed → nothing was sent → safe to resubmit.
        (Err(e), None) => {
            warn!(mint = %order.mint, "real buy never signed ({e}); reporting revert for retry");
            let _ = deps
                .fill_tx
                .send(Event::FillFailed { intent: order.intent, reason: FillFailReason::Reverted })
                .await;
        }
    }
}

async fn emit_entry_outcome(deps: &RealExecDeps, order: &BuyOrder, sig: String, outcome: EntryOutcome) {
    match outcome {
        EntryOutcome::Filled(legs) => {
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
        EntryOutcome::Reverted => {
            let _ = deps
                .fill_tx
                .send(Event::FillFailed {
                    intent: order.intent.clone(),
                    reason: FillFailReason::Reverted,
                })
                .await;
        }
        // Ambiguous (landed-but-unindexed / pending): emit nothing. The durable
        // BuySubmitted row is reconciled by the boot-recovery reaper — never resell.
        EntryOutcome::Ambiguous => {
            warn!(mint = %order.mint, "real buy outcome ambiguous — left BuySubmitted for the reaper");
        }
    }
}

enum EntryOutcome {
    Filled(SigLegs),
    Reverted,
    Ambiguous,
}

/// Confirm an entry fill: poll the feed for the wallet's own legs on `sig`, then
/// (on timeout) an RPC cross-check; `Succeeded` triggers an extended feed poll.
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
    // Watchdog: one RPC cross-check.
    match deps.trader.signature_state_detailed(sig).await {
        Ok(SigStatus::Succeeded) => {
            // Landed; wait for the feed to index the economics.
            match poll_feed_buy(deps, wallet, mint, sig, guard, EXTENDED_FEED_WINDOW).await {
                Some(legs) => EntryOutcome::Filled(legs),
                None => EntryOutcome::Ambiguous,
            }
        }
        Ok(SigStatus::Reverted { .. }) => EntryOutcome::Reverted,
        Ok(SigStatus::Pending) | Err(_) => EntryOutcome::Ambiguous,
    }
}

/// Poll the trades feed for the wallet's buy legs on `sig` up to `window`.
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
    let wallet = deps.trader.wallet_pubkey();
    let guard = deps.trade_signals.register(&wallet, &order.mint);
    let mut sell_sigs: Vec<String> = Vec::new();

    for attempt in 0..SELL_ATTEMPTS {
        let submit = submit_one_sell(&deps, &order, attempt).await;
        match submit {
            Ok(Some(sig)) => sell_sigs.push(sig),
            Ok(None) => {} // not submitted this attempt (e.g. nothing to sell)
            Err(e) => warn!(mint = %order.mint, "real sell attempt {attempt} failed: {e}"),
        }
        // Confirm: sum this position's own sell legs vs the held amount.
        if let Some(legs) = confirm_sell(&deps, &guard, &wallet, &order, &sell_sigs).await {
            let token_account = order.token_account.clone().or_else(|| deps.trader.cached_token_account(&order.mint));
            deps.fill_sigs
                .put(order.intent.clone(), FillSigs { sigs: sell_sigs.clone(), token_account });
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
            return;
        }
    }

    // Never cleared after every attempt. If we submitted anything, the outcome is
    // unconfirmed (a sell may still land → never resell). If we never submitted, the
    // sell is safe to retry (report Reverted so the engine re-drives it).
    let reason = if sell_sigs.is_empty() {
        FillFailReason::Reverted
    } else {
        FillFailReason::Unconfirmed
    };
    info!(mint = %order.mint, ?reason, "real sell unresolved after {SELL_ATTEMPTS} attempts");
    let _ = deps.fill_tx.send(Event::FillFailed { intent: order.intent, reason }).await;
}

/// One sell attempt — routed AMM vs curve by the position's migration state, tip
/// escalating with `attempt`. `confirm=false`: we confirm via the feed, not RPC.
async fn submit_one_sell(
    deps: &RealExecDeps,
    order: &SellOrder,
    attempt: u8,
) -> pump_trader::Result<Option<String>> {
    if order.is_migrated {
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
        deps.trader
            .sell_token_once(
                &order.mint,
                order.token_amount,
                order.creator.as_deref(),
                order.cashback_enabled,
                order.token_account.as_deref(),
                order.slippage_bps,
                attempt,
                false,
            )
            .await
    }
}

/// Confirm a sell cleared: the held amount minus the summed sell legs is ≤ dust.
async fn confirm_sell(
    deps: &RealExecDeps,
    guard: &trading_core::state::trade_signals::WaitGuard,
    wallet: &str,
    order: &SellOrder,
    sell_sigs: &[String],
) -> Option<SigLegs> {
    if sell_sigs.is_empty() {
        return None;
    }
    let deadline = tokio::time::Instant::now() + SELL_CONFIRM_WINDOW;
    loop {
        if let Ok(Some(legs)) = deps
            .trade_repo
            .sum_legs_by_signatures(wallet, &order.mint, sell_sigs, TradeType::Sell)
            .await
        {
            let sold = legs.token_amount;
            if sold.saturating_add(PARTIAL_FILL_THRESHOLD) >= order.token_amount {
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
