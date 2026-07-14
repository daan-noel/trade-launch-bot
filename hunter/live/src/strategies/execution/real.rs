//! Real (on-chain) execution: snipe-buy a matched token and resolve its entry
//! fill from the WS/DB feed; sell a position out and close it. The double-buy
//! safety rule — *re-send only on a confirmed on-chain revert* — is centralized
//! in [`classify_silent_send`] and unit-tested without a live chain.
//!
//! Strategy-agnostic: operates on the unified [`StrategyPosition`] / [`StrategyRepo`]
//! / [`StrategyRuntimeCache`]. The buy/sell/recovery flow is identical across
//! strategies; tpsl2's scalp-entry arming sits in the service layer ahead of the
//! buy, not here.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::json;
use tokio::time::{sleep, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

use trading_core::models::StrategyPosition;
use trading_core::storage::repositories::strategy_repo::StrategyRepo;
use trading_core::storage::repositories::trade_repo::{SigLegs, TradeRepo};
use trading_core::strategies::runtime_cache::StrategyRuntimeCache;

use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use crate::trader::{PumpFunTrader, SigStatus};
use pump_trader::{classify_swap_revert, SwapDirection, SwapRetryDecision, SwapRoute};

/// Decision for a snipe buy that was sent but whose fill never appeared in the
/// WS/DB feed within the poll window. Centralizing it keeps the **double-buy
/// safety rule** — *re-send only on a confirmed on-chain revert* — in one place
/// that is unit-tested without a live chain. The revert sub-classification funnels
/// through the SSOT `pump_trader::classify_swap_revert` (curve buy route — snipes
/// are curve-only), so a futile revert gives up instead of re-paying fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentSendOutcome {
    /// Slippage-floor revert (no tokens bought) — safe to re-send, re-quoted with
    /// fresh reserves.
    Resend,
    /// Stale-creator `ConstraintSeeds` (2006) revert — refresh the creator and
    /// re-send only if it actually changed; an unchanged creator (or a failed
    /// refresh) is a guaranteed-futile resend, so the caller gives up.
    RefreshCreatorThenResend,
    /// Tx landed but isn't indexed yet — wait for the row; never re-send (a
    /// second buy would double-fill).
    WaitThenSettle,
    /// Structural/unknown revert on the route the retry would reuse, pending/dropped,
    /// or the status check failed — give up rather than re-pay fees on a futile
    /// resend or risk double-buying from a nonce tx that could still land.
    GiveUp,
}

/// Map a one-shot `signature_state_detailed` result to the post-poll action. Pure;
/// generic over the error type so tests need no `anyhow` value. A confirmed revert
/// is sub-classified by its on-chain error code through the shared SSOT decision
/// (route=Curve, direction=Buy — snipes are curve-only): buy slippage → resend,
/// stale-creator 2006 → refresh-then-resend, structural/unknown → give up.
fn classify_silent_send<E>(status: &Result<SigStatus, E>) -> SilentSendOutcome {
    match status {
        Ok(SigStatus::Reverted { custom }) => {
            match classify_swap_revert(*custom, SwapRoute::Curve, SwapDirection::Buy) {
                SwapRetryDecision::Retry => SilentSendOutcome::Resend,
                SwapRetryDecision::RefreshCreator => SilentSendOutcome::RefreshCreatorThenResend,
                // StopFeeBurn (structural/unknown) or any decision not reachable on a
                // curve buy → a resend would just revert again, so give up.
                _ => SilentSendOutcome::GiveUp,
            }
        }
        Ok(SigStatus::Succeeded) => SilentSendOutcome::WaitThenSettle,
        Ok(SigStatus::Pending) | Err(_) => SilentSendOutcome::GiveUp,
    }
}

/// Recovery verdict for a `BuySubmitted` position whose submitted buy did **not**
/// turn up in the trade feed — used by the boot/periodic buy-recovery reaper. The
/// reaper **never re-sends** (a durable-nonce buy can still land after reboot, so
/// re-firing would double-buy), so the only actions are drop-or-wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuyRecoveryVerdict {
    /// Tx reverted on-chain → bought nothing → safe to delete the unentered row.
    Drop,
    /// Landed-but-unindexed, pending/dropped, or status unknown → tokens may exist
    /// or the tx may still land → leave `BuySubmitted` and re-check next tick.
    Wait,
}

/// Classify one submitted buy signature's on-chain status for recovery. Pure +
/// generic over the error type. Only a **proven revert** drops; everything else waits.
pub(crate) fn classify_submitted_buy<E>(status: &Result<Option<bool>, E>) -> BuyRecoveryVerdict {
    match status {
        Ok(Some(false)) => BuyRecoveryVerdict::Drop,
        Ok(Some(true)) | Ok(None) | Err(_) => BuyRecoveryVerdict::Wait,
    }
}

/// Convert the token cache's in-memory reserve pair — token side in raw units,
/// SOL side in SOL — into the `(virtual_token, virtual_quote=lamports)` pair the
/// snipe buy's slippage `min_out` expects (the trade-math arg names mirror pump.fun's
/// on-chain curve fields). `None` when either snapshot is missing or non-positive, so
/// the buy proceeds unprotected (`min_out=1`) rather than blocking on an inline
/// reserve RPC (the latency budget forbids it on this path).
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

/// Reclaim the token account's rent by closing it — but ONLY when no OTHER open
/// position shares this `(wallet, mint)` (M1). Two positions on one mint intentionally
/// land in ONE shared token account (`find_reusable_token_account`), so closing it on
/// the first position's exit would orphan the second's still-held bag. Off the hot
/// path (spawned by callers). On a query error we DEFER (don't close) — a shared
/// account might exist; the last position out reclaims the rent.
async fn reclaim_token_account_if_last(
    trader: &Arc<PumpFunTrader>,
    repo: &StrategyRepo,
    wallet: &str,
    mint: &str,
    mode: &str,
    exclude_position: Uuid,
) {
    match repo
        .has_other_open_position_on_mint(wallet, mint, mode, exclude_position)
        .await
    {
        Ok(true) => {
            debug!(mint = %mint,
                "rent-reclaim close skipped: another open position still holds the shared token account");
            return;
        }
        Ok(false) => {}
        Err(err) => {
            warn!(mint = %mint,
                "rent-reclaim other-open check failed; deferring close (a shared account may exist): {err}");
            return;
        }
    }
    if let Err(err) = trader.close_token_account(mint, None).await {
        debug!(mint = %mint, "rent-reclaim close skipped: {err}");
    }
}

/// Close a position whose token balance was already externally cleared (manual sell),
/// without sending any new on-chain sell. Records the close with `exit_price`/
/// `exit_time`; `exit_sol` is approximated as `exit_price × entry_token_amount`
/// (no actual strategy sell occurred). Shared by the proactive manual-sell path and
/// the reactive sell-revert fallback.
pub(crate) async fn close_externally_cleared_position(
    position: &mut StrategyPosition,
    repo: &StrategyRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    trader: &Arc<PumpFunTrader>,
    exit_price: f64,
    exit_time: DateTime<Utc>,
    reason: &str,
) {
    // The position is terminal from this point — release the SOL commitment
    // regardless of whether the DB write below succeeds.
    trader.release_sol_for_position(&position.id.to_string());
    let entry_amount = position.entry_token_amount.unwrap_or(0);
    let mint = position.mint_address.clone();
    // Rent reclaim: the token account is (for this position) empty — fire-and-forget
    // close, but only if no sibling position still shares the account (M1).
    {
        let trader = trader.clone();
        let repo = repo.clone();
        let wallet = position.wallet.clone();
        let mint = mint.clone();
        let mode = position.mode.clone();
        let id = position.id;
        tokio::spawn(async move {
            reclaim_token_account_if_last(&trader, &repo, &wallet, &mint, &mode, id).await;
        });
    }
    let prev = position.clone();
    // price is SOL per raw unit → exit_sol (human SOL) = price × raw tokens.
    position.close(exit_price, exit_price * entry_amount as f64, entry_amount, vec![], exit_time, reason);
    if let Err(err) = repo.update_position(position).await {
        warn!(
            position_id = %position.id, mint = %mint,
            "Failed to close externally-cleared position: {err}"
        );
    } else {
        runtime.sync_position(Some(&prev), position);
        let pnl_pct = position.pnl_pct().unwrap_or(0.0);
        info!(
            position_id = %position.id, mint = %mint, pnl_pct,
            "Position closed after external (manual) sell confirmed"
        );
    }
}

/// Reconcile a single mint's open real positions after an external (manual) sell
/// cleared the wallet's bag: drive each entry-recorded `Holding` position to `End`
/// (reason `ManualSell`) without sending a new sell tx. `None` when no open position
/// exists (caller stops retrying); `Some(0)` when one exists but the feed hasn't yet
/// caught the sell (index lag — caller retries); `Some(n)` for `n` closed.
pub(crate) async fn reconcile_externally_cleared_mint(
    mint: &str,
    repo: &StrategyRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    trader: &Arc<PumpFunTrader>,
) -> Option<usize> {
    let positions = match repo.find_open_by_mint(mint, "real").await {
        Ok(p) if p.is_empty() => return None,
        Ok(p) => p,
        Err(err) => {
            warn!(mint = %mint, "manual-sell reconcile: open-position query failed: {err}");
            return None;
        }
    };
    let wallet = trader.wallet_pubkey();
    // Confirm via the trades feed that the bag is actually cleared before closing.
    match trade_repo.net_token_amount_by_wallet_and_mint(&wallet, mint).await {
        Ok(balance) if balance <= super::PARTIAL_FILL_THRESHOLD => {}
        Ok(_) => return Some(0),
        Err(err) => {
            warn!(mint = %mint, "manual-sell reconcile: balance query failed: {err}");
            return Some(0);
        }
    }
    // Exit price/time from the most recent sell; fall back to entry if not indexed.
    let last_sell = trade_repo
        .find_latest_by_wallet_mint_type(&wallet, mint, trading_core::models::trade::TradeType::Sell)
        .await
        .ok()
        .flatten();
    let mut closed = 0;
    for position in positions {
        // The atomic claim is the double-exit interlock: skip if a strategy sell is
        // already in flight for this position.
        let Some(guard) = runtime.try_begin_exit(position.id) else { continue };
        let _guard = guard;
        let (exit_price, exit_time) = match &last_sell {
            Some(s) => (s.price_per_token, s.block_time),
            None => (position.entry_price.unwrap_or(0.0), Utc::now()),
        };
        let mut position = position;
        close_externally_cleared_position(
            &mut position,
            repo,
            runtime,
            trader,
            exit_price,
            exit_time,
            "ManualSell",
        )
        .await;
        closed += 1;
    }
    Some(closed)
}

/// Off-chain dependency of the snipe buy flow: send a buy (no RPC confirm) and
/// classify a sent signature. A trait so `buy_until_filled_or_give_up` can be driven
/// by a scripted fake in tests instead of a live chain.
#[async_trait::async_trait]
pub(crate) trait SnipeExecutor: Send + Sync {
    fn wallet(&self) -> String;
    /// Send a snipe buy with **write-ahead** signature persistence: the durable-nonce
    /// signature is known the instant the tx is signed — before the network round-trip
    /// — so `on_signed` is invoked with it *before* submit, to durably record the
    /// `BuySubmitted` marker ahead of the on-chain side effect. Returns the submitted
    /// signature.
    #[allow(clippy::too_many_arguments)]
    async fn send_snipe_buy(
        &self,
        mint: &str,
        creator: &str,
        token_program_id: &str,
        amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
        on_signed: pump_trader::BuySignedHook,
        cashback_enabled: bool,
        // When `Some`, buy directly into this already-existing account (a prior
        // position on the same mint persisted it) — skip the template pool so both
        // buys land in ONE account. `None` = first buy: template mints a fresh one.
        user_token_account_override: Option<&str>,
    ) -> anyhow::Result<String>;
    /// Detailed status (unlike a bare landed-bool, preserves the on-chain custom
    /// error code) so a confirmed stale-creator revert can be told apart from any
    /// other confirmed revert — see [`buy_until_filled_or_give_up`]'s use of
    /// [`pump_trader::classify_swap_revert`].
    async fn check_signature(&self, signature: &str) -> anyhow::Result<SigStatus>;
    /// Re-read `mint`'s bonding-curve creator from chain and return it (also warms
    /// the trader's `TokenPDAs`/creator caches). Called only after a CONFIRMED
    /// stale-creator (2006) revert on this mint's snipe buy — pump.fun rotated
    /// `bonding_curve.creator` (`set_creator`) between the triggering event and
    /// the buy, so every resend with the original `creator` would revert
    /// identically. OFF the hot path.
    async fn refresh_creator(&self, mint: &str) -> anyhow::Result<String>;
    /// The trader's in-memory cached token account for `mint` (the account the last
    /// buy landed into), or `None`. Captured after a fill to persist on the row.
    fn cached_token_account(&self, mint: &str) -> Option<String>;
}

#[async_trait::async_trait]
impl SnipeExecutor for PumpFunTrader {
    fn wallet(&self) -> String {
        self.wallet_pubkey()
    }
    async fn send_snipe_buy(
        &self,
        mint: &str,
        creator: &str,
        token_program_id: &str,
        amount: f64,
        slippage_bps: Option<u64>,
        reserves: Option<(u128, u128)>,
        on_signed: pump_trader::BuySignedHook,
        cashback_enabled: bool,
        user_token_account_override: Option<&str>,
    ) -> anyhow::Result<String> {
        // Parse the persisted override (base58) into a Pubkey for the buy path. A
        // malformed stored value falls back to `None` (template path) rather than
        // failing the buy — the next fill re-persists a valid address.
        let override_pk = user_token_account_override
            .and_then(|s| std::str::FromStr::from_str(s).ok());
        // The trader returns `pump_trader::TradeError`; this trait method is
        // `anyhow::Result`, so lift it (the `?` would also work via `From`).
        self.buy_token_snipe_write_ahead(mint, creator, token_program_id, amount, slippage_bps, reserves, on_signed, cashback_enabled, override_pk)
            .await
            .map_err(anyhow::Error::from)
    }
    async fn check_signature(&self, signature: &str) -> anyhow::Result<SigStatus> {
        self.signature_state_detailed(signature).await.map_err(anyhow::Error::from)
    }
    async fn refresh_creator(&self, mint: &str) -> anyhow::Result<String> {
        self.get_creator_from_mint_pda(mint).await.map_err(anyhow::Error::from)
    }
    fn cached_token_account(&self, mint: &str) -> Option<String> {
        // Inherent method on PumpFunTrader (query.rs).
        PumpFunTrader::cached_token_account(self, mint)
    }
}

/// Retry/poll timing for [`buy_until_filled_or_give_up`]. `production()` mirrors the
/// `execution::BUY_*` constants; tests shrink it so the give-up and re-send paths
/// don't wait out real 12×1s poll windows.
#[derive(Clone, Copy)]
pub(crate) struct BuyRetryCfg {
    max_attempts: usize,
    backoff_ms: u64,
    poll_attempts: usize,
    poll_interval: Duration,
}

impl BuyRetryCfg {
    pub(crate) fn production() -> Self {
        Self {
            max_attempts: super::BUY_MAX_ATTEMPTS,
            backoff_ms: 500,
            poll_attempts: super::BUY_POLL_MAX_ATTEMPTS,
            poll_interval: Duration::from_millis(super::BUY_POLL_INTERVAL_MS),
        }
    }
}

/// Snipe-buy `mint` and record the entry fill on first sight in the WS feed,
/// retrying per `cfg`. Honors the double-buy invariant: only a confirmed on-chain
/// revert re-sends. `reserves_fn` re-quotes the slippage `min_out` from the live
/// token cache **per attempt** so a slippage-floor `Retry` resends with a fresh
/// floor instead of the same stale one (which would just revert again).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn buy_until_filled_or_give_up<E, F>(
    trader: Arc<E>,
    mint: String,
    mut creator: String,
    token_program_id: String,
    buy_amount_sol: f64,
    position_id: Uuid,
    repo: StrategyRepo,
    trade_repo: TradeRepo,
    runtime: Arc<StrategyRuntimeCache>,
    trade_signals: Arc<TradeSignals>,
    cfg: BuyRetryCfg,
    slippage_bps: Option<u64>,
    reserves_fn: F,
    cashback_enabled: bool,
) where
    E: SnipeExecutor + 'static,
    F: Fn() -> Option<(u128, u128)> + Send,
{
    let wallet = trader.wallet();
    let max_attempts = cfg.max_attempts;
    let mut backoff_ms = cfg.backoff_ms;
    // The submitted signatures of OUR buys so far. Per-signature attribution: the
    // entry is recovered by one of THESE signatures, never the latest buy on the
    // shared (wallet, mint) feed — so two concurrent positions on the same token
    // can't adopt each other's fill.
    let mut sent_sigs: Vec<String> = Vec::new();

    // Reuse a token account already persisted for this (wallet, mint) by a prior
    // position — so a second buy into the same mint lands in ONE account instead of
    // the template pool minting a second (which a later "sell all" could orphan).
    // `None` (the common case: first buy into this mint) → template path as before.
    let reuse_account: Option<String> = match repo
        .find_reusable_token_account(&wallet, &mint, "real")
        .await
    {
        Ok(a) => a,
        Err(err) => {
            warn!(mint = %mint, "failed to look up reusable token account (using template path): {err}");
            None
        }
    };
    if let Some(acct) = &reuse_account {
        info!(mint = %mint, account = %acct, "reusing persisted token account for re-buy");
    }
    // The account to persist on the fill: the reused one if any, else whatever the
    // template minted (read from the trader cache after the buy below).
    let persist_account = |trader: &Arc<E>| -> Option<String> {
        reuse_account.clone().or_else(|| trader.cached_token_account(&mint))
    };

    for attempt in 1..=max_attempts {
        // Never double-send: if a previous attempt's buy has since landed + indexed,
        // adopt that fill instead of firing another buy.
        if adopt_existing_fill_if_present(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &sent_sigs, persist_account(&trader).as_deref())
            .await
        {
            return;
        }

        // Per-attempt slot the write-ahead hook fills with the signed tx's signature
        // BEFORE the submit POST. The durable-nonce signature is fixed at signing, so
        // this is known pre-network; persisting it first makes a crash anywhere after
        // signing recoverable.
        let signed_slot: Arc<std::sync::Mutex<Option<String>>> =
            Arc::new(std::sync::Mutex::new(None));
        let on_signed: pump_trader::BuySignedHook = {
            let repo = repo.clone();
            let runtime = runtime.clone();
            let mint = mint.clone();
            let slot = signed_slot.clone();
            Box::new(move |sig: String| {
                Box::pin(async move {
                    // In-memory journal FIRST (synchronous): captures the signature for
                    // THIS process's recovery before any await, so the write-ahead
                    // guarantee doesn't depend on the DB round-trip below completing.
                    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(sig.clone());
                    // Write-ahead durable marker: record this signature + flip the row
                    // to `BuySubmitted` BEFORE the tx is submitted, so a crash in the
                    // send→record gap can recover the entry (`redrive_orphaned_buy_
                    // submitted`). Bounded (M2): this hook is awaited inside the caller's
                    // durable-nonce slot hold (pump-trader's `build_trade_tx` … send), so
                    // a stalled PG must never serialize buys behind a slow persist — cap
                    // it. On timeout the in-memory journal above still covers this-process
                    // recovery; only the cross-restart marker is (rarely) lost.
                    const MARK_SUBMITTED_TIMEOUT: Duration = Duration::from_millis(250);
                    match tokio::time::timeout(
                        MARK_SUBMITTED_TIMEOUT,
                        repo.mark_buy_submitted(position_id, &sig),
                    )
                    .await
                    {
                        Ok(Ok(Some(updated))) => {
                            // Sync off the PRIOR cached row (already in the holding
                            // index + counted `pending` since Arming), NOT `None`.
                            // Passing `None` makes `sync_position` treat this as a
                            // brand-new pending position and bump `pending_count` a
                            // SECOND time for a row already counted at Arming — the
                            // leak that shows `pending = 2` for a single real buy (and
                            // +1 more per resend attempt). Arming→BuySubmitted (and a
                            // BuySubmitted resend) is a zero-delta transition: both are
                            // in-index, both pending, neither entered.
                            let prev = runtime
                                .holding_by_mint(&mint)
                                .into_iter()
                                .find(|p| p.id == updated.id);
                            runtime.sync_position(prev.as_deref(), &updated);
                        }
                        Ok(Ok(None)) => {} // already advanced to Holding concurrently
                        Ok(Err(err)) => warn!(mint = %mint, sig = %sig,
                            "failed to persist BuySubmitted marker (continuing): {err}"),
                        Err(_) => warn!(mint = %mint, sig = %sig,
                            "BuySubmitted persist exceeded {MARK_SUBMITTED_TIMEOUT:?}; proceeding \
                             (in-memory journal holds the write-ahead guarantee for this process)"),
                    }
                })
            })
        };

        // Re-quote the slippage floor from the LIVE reserve cache each attempt: a
        // slippage `Retry` that resent the SAME stale `min_out` would just revert
        // again. `None` ⇒ min_out=1 (unprotected) rather than an inline reserve RPC
        // (the latency budget forbids one here).
        let reserves = reserves_fn();

        // Snipe send WITHOUT the blocking RPC confirm — the WS/DB trade feed below is
        // the sole confirmation and the entry-price source. `reuse_account` (if set)
        // routes the buy straight into the existing account, skipping the template.
        let send_result = trader
            .send_snipe_buy(&mint, &creator, &token_program_id, buy_amount_sol, slippage_bps, reserves, on_signed, cashback_enabled, reuse_account.as_deref())
            .await;

        // The signature is known the instant the tx was signed (captured by the hook
        // pre-submit). If the slot is empty the send failed BEFORE signing, so no tx
        // exists on-chain and a retry can't double-buy.
        let signed = signed_slot.lock().unwrap_or_else(|p| p.into_inner()).take();
        let signature = match signed {
            Some(sig) => sig,
            None => {
                match &send_result {
                    Err(err) => warn!(mint = %mint, attempt, "buy send failed before signing: {err}"),
                    Ok(sig) => warn!(mint = %mint, attempt, sig = %sig,
                        "buy reported success but the write-ahead hook never fired; retrying"),
                }
                if attempt < max_attempts {
                    sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).saturating_add(100);
                }
                continue;
            }
        };
        // Signed → the tx may land even if the submit POST reported an error. Attribute
        // the signature to THIS buy and confirm via the feed; from here we NEVER fire
        // another buy except on a proven on-chain revert (the double-buy invariant).
        sent_sigs.push(signature.clone());
        match &send_result {
            Ok(_) => info!(mint = %mint, attempt, sig = %signature,
                "buy submitted (no RPC confirm); polling WS feed for fill"),
            Err(err) => warn!(mint = %mint, attempt, sig = %signature,
                "buy submit reported an error after signing; treating the signed tx as in-flight \
                 (no re-send unless it reverts on-chain): {err}"),
        }

        // Poll the WS-fed trade feed for this wallet's buy row. The buy has been
        // sent, so the trader cache now holds the account it landed into — capture
        // it to persist on the fill.
        if poll_feed_until_entry_fill(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs, persist_account(&trader).as_deref())
            .await
        {
            return;
        }

        // The fill never showed within the window. One status check decides the next
        // move; the decision table lives in `classify_silent_send`.
        let status = trader.check_signature(&signature).await;
        match classify_silent_send(&status) {
            SilentSendOutcome::Resend => {
                // Buy slippage floor missed — resend; the per-attempt `reserves_fn`
                // re-quotes `min_out` from the live cache above so the retry uses a
                // fresh floor. Gated on a CONFIRMED revert, so this can't double-buy.
                warn!(mint = %mint, attempt, sig = %signature,
                    "buy reverted on a slippage floor; retrying with fresh reserves");
            }
            SilentSendOutcome::RefreshCreatorThenResend => {
                // A stale `bonding_curve.creator` (pump `set_creator` between the
                // triggering event and this buy) means every resend with the ORIGINAL
                // `creator` reverts identically (2006). Refresh once; resend only if it
                // ACTUALLY changed — an unchanged creator (or a failed refresh) is a
                // guaranteed-futile resend, so give up rather than re-pay fees. Gated on
                // a CONFIRMED revert, so this can't double-buy; shares the SSOT decision
                // with the sell path (`classify_sell_confirm`) and pump-trader's own
                // in-call heal.
                match trader.refresh_creator(&mint).await {
                    Ok(fresh) if fresh != creator => {
                        warn!(mint = %mint, attempt, sig = %signature,
                            old_creator = %creator, new_creator = %fresh,
                            "buy reverted on a stale creator (pump set_creator); \
                             refreshed creator, retrying");
                        creator = fresh;
                    }
                    Ok(_) => {
                        warn!(mint = %mint, attempt, sig = %signature,
                            "buy reverted 2006 but the creator is unchanged on re-read; \
                             giving up (a resend would only re-pay fees)");
                        return;
                    }
                    Err(err) => {
                        warn!(mint = %mint, attempt, sig = %signature,
                            "creator refresh failed after a 2006 revert: {err}; giving up \
                             (a resend with the original creator would revert identically)");
                        return;
                    }
                }
            }
            SilentSendOutcome::WaitThenSettle => {
                warn!(mint = %mint, sig = %signature,
                    "buy landed but not yet indexed; awaiting fill without re-send");
                if poll_feed_until_entry_fill(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs, persist_account(&trader).as_deref())
                    .await
                {
                    return;
                }
                warn!(mint = %mint, sig = %signature,
                    "buy confirmed on-chain but never indexed — leaving position unentered for review");
                return;
            }
            SilentSendOutcome::GiveUp => {
                match &status {
                    Ok(SigStatus::Reverted { custom }) => warn!(mint = %mint, sig = %signature,
                        raw_error_code = ?custom,
                        "buy reverted on-chain (structural/unknown/unchanged-2006) on the \
                         route the retry would reuse; giving up (a blind resend would only \
                         re-pay fees)"),
                    Err(err) => warn!(mint = %mint, sig = %signature,
                        "buy status check failed: {err}; not re-sending (double-buy risk)"),
                    _ => warn!(mint = %mint, sig = %signature,
                        "buy neither indexed nor on-chain; not re-sending (double-buy risk)"),
                }
                return;
            }
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, "buy failed after {max_attempts} attempts");
        }
    }
}

/// Wait for this wallet's buy on `mint` to land in the WS-fed trade feed; on first
/// sight, record the entry and sync the runtime cache. Returns true once an entry has
/// been recorded, false if the window elapses without a fill. Event-driven (wakes on
/// the DbWriter's persist signal) with a fallback tick + hard deadline equal to the
/// old polling window.
#[allow(clippy::too_many_arguments)]
async fn poll_feed_until_entry_fill(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    repo: &StrategyRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    trade_signals: &Arc<TradeSignals>,
    cfg: &BuyRetryCfg,
    sent_sigs: &[String],
    token_account: Option<&str>,
) -> bool {
    let guard = trade_signals.register(wallet, mint);
    let deadline = Instant::now() + cfg.poll_interval * cfg.poll_attempts as u32;

    loop {
        // Arm the wakeup BEFORE the DB check so a notify in the gap isn't lost.
        let notified = guard.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if adopt_existing_fill_if_present(mint, wallet, position_id, repo, trade_repo, runtime, sent_sigs, token_account).await {
            return true;
        }

        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let fallback = cfg.poll_interval.min(deadline - now);
        tokio::select! {
            _ = &mut notified => {}
            _ = sleep(fallback) => {}
        }
    }
}

/// If one of OUR sent buy signatures (`sent_sigs`) is already present in the trade
/// feed for `(wallet, mint)`, record it as the position entry (price/amount/sol/tx/
/// time summed from that signature's legs) and return true; otherwise return false
/// without sleeping. Per-signature attribution: we adopt only a fill the bot itself
/// submitted, so a concurrent same-token position never adopts the other's buy.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn adopt_existing_fill_if_present(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    repo: &StrategyRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    sent_sigs: &[String],
    // The wallet's token account for this mint (reused or template-minted), persisted
    // on the row so the sell / next buy reuse it across restarts. `None` keeps any
    // existing value (COALESCE) — e.g. the recovery reaper has nothing fresher.
    token_account: Option<&str>,
) -> bool {
    for sig in sent_sigs {
        let fill = match trade_repo.find_fill_by_signature(wallet, mint, sig).await {
            Ok(Some(fill)) => fill,
            Ok(None) => continue,
            Err(err) => {
                warn!(mint = %mint, "failed to query trades for buy confirmation: {err}");
                continue;
            }
        };
        if let Ok(Some(prev)) = repo.find_position(position_id).await {
            // `record_entry_fill` RETURNs the updated row, so we sync the cache off it
            // directly — no follow-up read of the row we just wrote.
            match repo
                .record_entry_fill(
                    position_id,
                    sig,
                    fill.token_amount,
                    fill.price_per_token(),
                    fill.amount_sol,
                    fill.first_block_time,
                    token_account,
                )
                .await
            {
                Ok(current) => {
                    runtime.sync_position(Some(&prev), &current);
                    info!(mint = %mint, tx = %sig, "position entry recorded from buy fill");
                }
                Err(err) => warn!(mint = %mint, "failed to update position entry after buy: {err}"),
            }
        }
        return true;
    }
    false
}

/// Sell a position's full balance out (retrying / re-routing across migration) and,
/// on a confirmed clear, close it; otherwise mark it ExitFailed at the trigger price.
/// `trigger_price`/`trigger_time` are the hypothetical exit recorded if the sell
/// never confirms; `exit_reason` is persisted either way.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sell_and_close_position(
    trader: Arc<PumpFunTrader>,
    mut position: StrategyPosition,
    repo: StrategyRepo,
    trade_repo: TradeRepo,
    runtime: Arc<StrategyRuntimeCache>,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    trigger_price: f64,
    trigger_time: DateTime<Utc>,
    exit_reason: String,
    slippage_bps: Option<u64>,
) {
    // Position is terminal from this point — release the SOL commitment. Idempotent.
    trader.release_sol_for_position(&position.id.to_string());
    let mint = position.mint_address.clone();
    // M1: serialize real exits on the same (wallet, mint). Two positions sharing ONE
    // token account must not race the shared balance (or the rent-reclaim close); held
    // for the whole sell+close. Different mints take different mutexes (no contention).
    let _mint_exit_guard = runtime.mint_exit_lock(&position.wallet, &mint).lock_owned().await;
    let target_tokens = position.entry_token_amount.unwrap_or(0);
    let amount = target_tokens;
    let base_token_program = position
        .token_program_id
        .clone()
        .unwrap_or_else(|| trading_core::config::constants::TOKEN_PROGRAM_ID.to_string());
    // The token account persisted at entry-fill time — the sell targets this exact
    // account (restart-safe), not whatever the in-memory cache happens to hold.
    let persisted_token_account = position.token_account.clone();

    if amount == 0 {
        warn!(
            position_id = %position.id, mint = %mint,
            "No entry token amount recorded — closing position without sell TX"
        );
        let prev = position.clone();
        position.close(trigger_price, 0.0, 0, Vec::new(), trigger_time, &exit_reason);
        if let Err(err) = repo.update_position(&position).await {
            warn!(
                position_id = %position.id, mint = %mint,
                "Failed to close zero-amount position: {err}"
            );
        } else {
            runtime.sync_position(Some(&prev), &position);
        }
        return;
    }

    info!(
        position_id = %position.id, mint = %mint, amount,
        "Executing sell for exited position"
    );

    // The retry loop confirms the clear by summing THIS position's own sell
    // signatures' token legs against its entry amount (not the shared net balance),
    // so on a confirmed clear it hands back those signatures + their rolled-up legs.
    let (sigs, legs) = match sell_until_balance_cleared(
        trader.clone(),
        mint.clone(),
        amount,
        trade_repo.clone(),
        cache,
        trade_signals,
        base_token_program,
        slippage_bps,
        persisted_token_account,
    )
    .await
    {
        SellOutcome::Cleared { sigs, legs } => (sigs, legs),
        SellOutcome::Failed { sigs } => {
            // Reactive fallback: before marking ExitFailed, check whether the balance
            // was already cleared by an external (manual) sell.
            let wallet = trader.wallet_pubkey();
            if let Ok(balance) =
                trade_repo.net_token_amount_by_wallet_and_mint(&wallet, &mint).await
            {
                if balance <= super::PARTIAL_FILL_THRESHOLD {
                    if let Ok(Some(last_sell)) = trade_repo
                        .find_latest_by_wallet_mint_type(
                            &wallet,
                            &mint,
                            trading_core::models::trade::TradeType::Sell,
                        )
                        .await
                    {
                        info!(
                            position_id = %position.id, mint = %mint,
                            "Sell failed but balance externally cleared; closing as ManualSell"
                        );
                        close_externally_cleared_position(
                            &mut position,
                            &repo,
                            &runtime,
                            &trader,
                            last_sell.price_per_token,
                            last_sell.block_time,
                            "ManualSell",
                        )
                        .await;
                        return;
                    }
                }
            }
            warn!(
                position_id = %position.id, mint = %mint,
                "Sell execution finished without clearing token balance; marking position ExitFailed"
            );
            let prev = position.clone();
            position.mark_exit_failed(trigger_price, trigger_time);
            position.exit_reason = Some(exit_reason.clone());
            // Record whatever legs DID sell so the failed row still attributes them.
            position.exit_tx_signatures = json!(sigs);
            if let Err(err) = repo.update_position(&position).await {
                warn!(
                    position_id = %position.id, mint = %mint,
                    "Failed to mark position {} ExitFailed: {err}", position.id
                );
            } else {
                runtime.sync_position(Some(&prev), &position);
            }
            return;
        }
        SellOutcome::Unconfirmed { sigs } => {
            // C1: the sell may have landed — or may still land — but the feed never
            // confirmed the clear and the tx did NOT revert, so re-selling would risk a
            // double-sell. Leave the position `ExitUnconfirmed` (NOT `ExitFailed`,
            // which asserts nothing sold) and ALARM for manual review. NEVER fire a
            // second sell, and NEVER close the (possibly still-funded) token account.
            warn!(
                position_id = %position.id, mint = %mint, exit_reason = %exit_reason,
                alarm = true,
                "SELL UNCONFIRMED — bag may or may not be sold; NOT re-selling and NOT \
                 closing the token account. Marking ExitUnconfirmed for manual review"
            );
            let prev = position.clone();
            position.mark_exit_unconfirmed(trigger_price, trigger_time);
            position.exit_reason = Some(exit_reason.clone());
            // Attribute whatever sell legs we did submit (for the manual review trail).
            position.exit_tx_signatures = json!(sigs);
            if let Err(err) = repo.update_position(&position).await {
                warn!(
                    position_id = %position.id, mint = %mint,
                    "Failed to mark position {} ExitUnconfirmed: {err}", position.id
                );
            } else {
                runtime.sync_position(Some(&prev), &position);
            }
            return;
        }
    };

    // Rent reclaim (off the hot path): balance confirmed cleared, so close the now-
    // empty token account to recover its ~0.002 SOL rent — but only if no sibling
    // position still shares it (M1). Fire-and-forget.
    {
        let trader = trader.clone();
        let repo = repo.clone();
        let wallet = position.wallet.clone();
        let mint = mint.clone();
        let mode = position.mode.clone();
        let id = position.id;
        tokio::spawn(async move {
            reclaim_token_account_if_last(&trader, &repo, &wallet, &mint, &mode, id).await;
        });
    }

    if let Some(legs) = legs {
        let prev = position.clone();
        position.close(
            legs.price_per_token(),
            legs.amount_sol,
            legs.token_amount,
            sigs,
            legs.last_block_time,
            &exit_reason,
        );
        if let Err(err) = repo.update_position(&position).await {
            warn!(
                position_id = %position.id, mint = %mint,
                "Failed to close position after confirmed sell: {err}"
            );
        } else {
            runtime.sync_position(Some(&prev), &position);
            let pnl_percent = position.pnl_pct().unwrap_or(0.0);
            info!(
                position_id = %position.id, mint = %mint, pnl_percent,
                "Position closed after confirmed sell"
            );
        }
        return;
    }

    warn!(
        position_id = %position.id, mint = %mint,
        "Sell completed but no confirmed sell record found"
    );
}

/// Outcome of [`sell_until_balance_cleared`]. Both variants carry the position's OWN
/// accumulated sell signatures (one per landed leg/attempt) so the close / ExitFailed
/// record attributes the exit per-signature. `Cleared` also carries the rolled-up
/// sell legs (`SigLegs`) so the close reuses the exit price/amount/time; `None` only
/// on the amount==0 no-op.
enum SellOutcome {
    Cleared { sigs: Vec<String>, legs: Option<SigLegs> },
    Failed { sigs: Vec<String> },
    /// The sell was sent but never confirmed the clear, and the sent tx did NOT revert
    /// (Succeeded/Pending/status-unknown) — so it may have sold or may still land, and
    /// re-sending would risk a double-sell (C1). The caller marks the position
    /// `ExitUnconfirmed` and alarms; no second sell is ever fired. Carries the sell
    /// signatures submitted so far for the manual-review trail.
    Unconfirmed { sigs: Vec<String> },
}

/// Action after a feed-confirm sell's poll window elapses without clearing, decided
/// from the sent tx's on-chain status. The double-sell safety rule — *re-send only on
/// a confirmed on-chain revert* — is the exact sell-side twin of the buy path's
/// [`classify_silent_send`] (C1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellConfirmAction {
    /// The sent tx **confirmed a revert** (so it sold nothing) — or the venue changed
    /// mid-exit (a route change means the sent route is moot), which only fires on a
    /// reverted status. Safe to act on the SSOT swap decision it carries (per-code
    /// retry / refresh / re-route / stop).
    Reclassify(SwapRetryDecision),
    /// The sent tx came back **Succeeded / Pending / status-unknown** — it may have
    /// sold or may still land, so re-sending would risk a double-sell. NEVER re-send;
    /// the caller extends the feed poll and, failing a clear, marks `ExitUnconfirmed`.
    WaitConfirm,
}

/// Map a sent sell's `signature_state_detailed` result to the post-poll action, using
/// the on-chain **program error code** for the revert sub-classification. Only a
/// **confirmed revert** (bought/sold nothing) — same route — is fed to the SSOT
/// `pump_trader::classify_swap_revert` (slippage retries, 2006 creator/coin_creator
/// refresh, 6024 cashback, 6005 reroute), shared with this crate's own trader-internal
/// `confirm=true` heal so the sync-error and feed-classify triggers apply the identical
/// rule. A **route change** mid-exit (`used_migrated != now_migrated`) only re-routes
/// on a reverted status — a Succeeded/Pending/unknown tx is never blindly re-sold just
/// because the token migrated. Every non-revert status → `WaitConfirm` (the C1 fix:
/// the old `_ => Retry` re-sent on Succeeded/Pending/RPC-error → double-sell). Pure;
/// generic over the error type so tests need no `anyhow` value.
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
        // Reverted (sold nothing) but the venue changed mid-exit → re-route the retry.
        Ok(SigStatus::Reverted { .. }) => SellConfirmAction::Reclassify(SwapRetryDecision::Retry),
        // Landed / pending / unknown → the sell may exist; never re-send (C1).
        Ok(SigStatus::Succeeded) | Ok(SigStatus::Pending) | Err(_) => SellConfirmAction::WaitConfirm,
    }
}

#[allow(clippy::too_many_arguments)]
async fn sell_until_balance_cleared(
    trader: Arc<PumpFunTrader>,
    mint: String,
    mut amount: u64,
    trade_repo: TradeRepo,
    cache: &TokenCache,
    trade_signals: Arc<TradeSignals>,
    base_token_program: String,
    slippage_bps: Option<u64>,
    // The token account persisted on the position row (Part 3). Preferred over the
    // cache-first resolve so the sell targets the SAME account the buy landed into —
    // no in-memory-cache dependency, correct across a process restart. `None` (legacy
    // rows / unrecorded) falls back to the cache-first lookup as before.
    persisted_token_account: Option<String>,
) -> SellOutcome {
    let mut attempt = 0usize;
    let max_attempts = super::SELL_MAX_ATTEMPTS;
    let mut backoff_ms = 300u64;
    let wallet = trader.wallet_pubkey();

    if amount == 0 {
        info!(mint = %mint, "sell skipped because amount is zero");
        return SellOutcome::Cleared { sigs: Vec::new(), legs: None };
    }
    // Total tokens this position is selling out (raw units). Confirmed by summing OUR
    // own sell signatures' token legs against this target.
    let target_tokens = amount;

    // Resolve the token account once and reuse it across attempts. Prefer the
    // address persisted on the position row (restart-safe, targets the exact account
    // the buy used); fall back to the cache-first on-chain resolve for legacy rows
    // that predate the persisted column.
    let token_account_override = match persisted_token_account {
        Some(acct) => Some(acct),
        None => trader
            .resolve_cached_token_account(&mint)
            .await
            .ok()
            .flatten()
            .map(|pk| pk.to_string()),
    };

    // Register interest in this (wallet, mint) once for the whole exit so the feed's
    // trade-sequence is observed continuously: the confirm loop only re-runs the
    // net-balance SQL aggregate when `seq` advanced.
    let guard = trade_signals.register(&wallet, &mint);
    let mut last_seq: Option<u64> = None;
    // Every sell signature THIS exit submitted (one per landed attempt/leg).
    let mut sell_sigs: Vec<String> = Vec::new();

    while attempt < max_attempts && amount > 0 {
        attempt += 1;
        // Re-read routing from the WS-fed cache each attempt: a held token can migrate
        // mid-exit (is_migrated flips), re-routing the next attempt to the AMM.
        let (is_cashback, is_migrated) = match cache.get(&mint) {
            Some(e) => (e.token.is_cashback_enabled, e.is_migrated),
            None => (false, false),
        };
        // Escalate the Jito tip each retry (1-based attempt → level 0 on the first).
        let tip_level = (attempt - 1) as u8;
        // confirm = false: this loop confirms by polling the LaserStream-fed `trades`
        // balance below, so the trader skips its redundant inner RPC poll. Bound the
        // send so a wedged RPC can't hang the position.
        const SELL_SEND_TIMEOUT: Duration = Duration::from_secs(15);
        let send = async {
            if is_migrated {
                trader
                    .amm_sell(
                        &mint,
                        amount,
                        &base_token_program,
                        None,
                        token_account_override.as_deref(),
                        slippage_bps,
                        tip_level,
                        false,
                    )
                    .await
            } else {
                trader
                    .sell_token_once(&mint, amount, None, is_cashback, token_account_override.as_deref(), slippage_bps, tip_level, false)
                    .await
            }
        };
        let sell_result = match tokio::time::timeout(SELL_SEND_TIMEOUT, send).await {
            // The trader returns `pump_trader::TradeError`; lift it into `anyhow`
            // so this arm unifies with the timeout arm below.
            Ok(res) => res.map_err(anyhow::Error::from),
            Err(_) => Err(anyhow::anyhow!("sell send timed out after {SELL_SEND_TIMEOUT:?}")),
        };
        match sell_result {
            Ok(Some(sig)) => {
                info!(mint = %mint, attempt, amount, "sell submitted (feed-confirm)");
                sell_sigs.push(sig.clone());
                let mut remaining_amount = amount;
                let mut cleared = false;
                let mut confirmed_legs: Option<SigLegs> = None;
                let interval = Duration::from_millis(super::SELL_POLL_INTERVAL_MS);
                let min_query_gap =
                    Duration::from_millis(super::SELL_BALANCE_QUERY_MIN_INTERVAL_MS);
                let deadline = Instant::now() + interval * super::SELL_POLL_MAX_ATTEMPTS as u32;
                let mut last_query_at: Option<Instant> = None;
                loop {
                    let notified = guard.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();

                    let now = Instant::now();
                    let at_deadline = now >= deadline;

                    let seq = guard.seq();
                    let seq_advanced = last_seq != Some(seq);
                    let rate_ok = at_deadline
                        || last_query_at.map_or(true, |t| now.duration_since(t) >= min_query_gap);
                    if seq_advanced && rate_ok {
                        last_query_at = Some(now);
                        // Sum only THIS position's own sell signatures' token legs and
                        // measure the remainder against its target.
                        match trade_repo
                            .sum_legs_by_signatures(
                                &wallet,
                                &mint,
                                &sell_sigs,
                                trading_core::models::trade::TradeType::Sell,
                            )
                            .await
                        {
                            Ok(legs) => {
                                let sold = legs.as_ref().map(|l| l.token_amount).unwrap_or(0);
                                let remaining = target_tokens.saturating_sub(sold);
                                if (remaining as i64) <= super::PARTIAL_FILL_THRESHOLD {
                                    info!(mint = %mint, attempt, "sell cleared the balance");
                                    cleared = true;
                                    confirmed_legs = legs;
                                    break;
                                }
                                remaining_amount = remaining as u64;
                                last_seq = Some(seq);
                            }
                            Err(err) => warn!("Failed to sum sell signature legs: {err}"),
                        }
                    }
                    if at_deadline {
                        break;
                    }
                    let now = Instant::now();
                    let fallback = interval.min(deadline - now);
                    tokio::select! {
                        _ = &mut notified => {}
                        _ = sleep(fallback) => {}
                    }
                }
                if cleared {
                    return SellOutcome::Cleared { sigs: sell_sigs, legs: confirmed_legs };
                }
                // Not cleared within the window. A partial fill leaves a smaller
                // remainder to chase; an unchanged balance means this send cleared
                // nothing — classify the sent tx before re-paying fees.
                if remaining_amount < amount {
                    warn!(mint = %mint, attempt, remaining = remaining_amount,
                        "sell partially filled; retrying the remainder with a higher tip");
                    amount = remaining_amount;
                } else {
                    let now_migrated =
                        cache.get(&mint).map(|e| e.is_migrated).unwrap_or(is_migrated);
                    let state = trader.signature_state_detailed(&sig).await;
                    let decision = match classify_sell_confirm(&state, is_migrated, now_migrated) {
                        SellConfirmAction::Reclassify(d) => d,
                        SellConfirmAction::WaitConfirm => {
                            // C1: the sent tx Succeeded / is Pending / status unknown — it
                            // may have sold or may still land, so we NEVER re-send (a
                            // second sell would double-sell). Extend the feed poll on a
                            // longer deadline; if the bag still never clears, give up as
                            // `ExitUnconfirmed` (alarmed for manual review) rather than
                            // firing another sell.
                            warn!(mint = %mint, attempt, sig = %sig,
                                "sell status inconclusive (landed/pending/unknown); NOT \
                                 re-sending — extending feed poll before giving up");
                            let ext_interval = Duration::from_millis(super::SELL_POLL_INTERVAL_MS);
                            let ext_min_gap =
                                Duration::from_millis(super::SELL_BALANCE_QUERY_MIN_INTERVAL_MS);
                            let ext_deadline = Instant::now()
                                + ext_interval * super::SELL_UNCONFIRMED_POLL_MAX_ATTEMPTS as u32;
                            let mut ext_last_seq: Option<u64> = None;
                            let mut ext_last_query_at: Option<Instant> = None;
                            loop {
                                let notified = guard.notified();
                                tokio::pin!(notified);
                                notified.as_mut().enable();

                                let now = Instant::now();
                                let at_deadline = now >= ext_deadline;
                                let seq = guard.seq();
                                let seq_advanced = ext_last_seq != Some(seq);
                                let rate_ok = at_deadline
                                    || ext_last_query_at
                                        .map_or(true, |t| now.duration_since(t) >= ext_min_gap);
                                if seq_advanced && rate_ok {
                                    ext_last_query_at = Some(now);
                                    // Same per-signature confirm as the main loop: sum only
                                    // THIS position's own sell legs against its target.
                                    match trade_repo
                                        .sum_legs_by_signatures(
                                            &wallet,
                                            &mint,
                                            &sell_sigs,
                                            trading_core::models::trade::TradeType::Sell,
                                        )
                                        .await
                                    {
                                        Ok(legs) => {
                                            let sold =
                                                legs.as_ref().map(|l| l.token_amount).unwrap_or(0);
                                            let remaining = target_tokens.saturating_sub(sold);
                                            if (remaining as i64) <= super::PARTIAL_FILL_THRESHOLD {
                                                info!(mint = %mint,
                                                    "sell cleared during the extended (unconfirmed) poll");
                                                return SellOutcome::Cleared {
                                                    sigs: sell_sigs,
                                                    legs,
                                                };
                                            }
                                            ext_last_seq = Some(seq);
                                        }
                                        Err(err) => warn!(
                                            "Failed to sum sell signature legs (extended poll): {err}"),
                                    }
                                }
                                if at_deadline {
                                    break;
                                }
                                let now = Instant::now();
                                let fallback = ext_interval.min(ext_deadline - now);
                                tokio::select! {
                                    _ = &mut notified => {}
                                    _ = sleep(fallback) => {}
                                }
                            }
                            warn!(mint = %mint, sig = %sig, alarm = true,
                                "SELL UNCONFIRMED — feed never confirmed the clear within the \
                                 extended window and the tx did not revert; giving up WITHOUT a \
                                 second sell (ExitUnconfirmed)");
                            return SellOutcome::Unconfirmed { sigs: sell_sigs };
                        }
                    };
                    match decision {
                        SwapRetryDecision::StopFeeBurn => {
                            let raw_code = match &state {
                                Ok(SigStatus::Reverted { custom }) => *custom,
                                _ => None,
                            };
                            // Error 6000 = pump.fun InvalidFeeRecipient — the
                            // PUMP_CURVE_FEE_RECIPIENT constant no longer matches what
                            // the program expects. If this appears across many distinct
                            // mints in a short window it signals a fee-recipient
                            // rotation: grep logs for `constant_rot_candidate=true` and
                            // verify the new account against a live swap before shipping
                            // an updated constant. Do NOT auto-discover on-chain.
                            if raw_code == Some(6000) {
                                warn!(
                                    mint = %mint,
                                    sig = %sig,
                                    is_migrated,
                                    constant_rot_candidate = true,
                                    "sell reverted with InvalidFeeRecipient (6000) — possible \
                                     pump.fun fee-recipient rotation; if this appears across many \
                                     mints, update PUMP_CURVE_FEE_RECIPIENT after verifying the \
                                     new account on-chain"
                                );
                            } else {
                                warn!(mint = %mint, attempt, sig = %sig, raw_error_code = ?raw_code,
                                    "sell reverted on-chain (structural/unknown) on a route the retry \
                                     would reuse; stopping (a blind re-send would only re-pay fees)");
                            }
                            return SellOutcome::Failed { sigs: sell_sigs };
                        }
                        SwapRetryDecision::RefreshCreator => {
                            match trader.refresh_curve_creator_vault(&mint).await {
                                Ok(Some(vault)) => warn!(mint = %mint, attempt, sig = %sig,
                                    new_creator_vault = %vault,
                                    "sell reverted on a stale creator_vault (pump set_creator); \
                                     refreshed creator, retrying with a higher tip"),
                                Ok(None) => {
                                    // The re-read yielded the SAME creator_vault, so a retry
                                    // would derive the identical vault and revert again — stop
                                    // instead of re-paying fees (mirrors the AMM branch below).
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "sell reverted 2006 but the creator_vault is unchanged \
                                         on re-read; not retrying (would only re-pay fees); \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                                Err(err) => {
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "creator_vault refresh failed after a 2006 revert: {err}; \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                            }
                        }
                        SwapRetryDecision::RefreshCoinCreator => {
                            match trader
                                .refresh_amm_pool_info(&mint, &base_token_program)
                                .await
                            {
                                Ok(Some(vault)) => warn!(mint = %mint, attempt, sig = %sig,
                                    new_coin_creator_vault_authority = %vault,
                                    "AMM sell reverted on a stale coin_creator (pump rotated the \
                                     pool's coin_creator after we cached it); refreshed the pool, \
                                     retrying with a higher tip"),
                                Ok(None) => {
                                    // The re-read yielded the SAME coin_creator, so a retry would
                                    // derive the identical authority and revert again — stop
                                    // instead of re-paying fees (bounds the refresh loop).
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "AMM sell reverted 2006 but the pool's coin_creator is \
                                         unchanged on re-read; not retrying (would only re-pay \
                                         fees); marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                                Err(err) => {
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "AMM pool refresh failed after a 2006 revert: {err}; \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                            }
                        }
                        SwapRetryDecision::RefreshCashback => {
                            match trader.refresh_curve_facts(&mint).await {
                                Ok(facts) => {
                                    // Update cache so next attempt's is_cashback read
                                    // picks up the toggled-on value (and toggle-OFF too).
                                    if let Some(mut e) = cache.get_mut(&mint) {
                                        e.token.is_cashback_enabled = facts.cashback_enabled;
                                    }
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        cashback_enabled = facts.cashback_enabled,
                                        "sell reverted 6024 (missing UVA); refreshed cashback \
                                         flag, retrying with a higher tip");
                                }
                                Err(err) => {
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "curve-facts refresh failed after 6024 revert: {err}; \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                            }
                        }
                        SwapRetryDecision::RerouteMigrated => {
                            match trader.refresh_curve_facts(&mint).await {
                                Ok(facts) => {
                                    if facts.is_migrated {
                                        // Update cache so the next loop iteration's
                                        // `is_migrated` read re-routes the sell to the AMM.
                                        if let Some(mut e) = cache.get_mut(&mint) {
                                            e.is_migrated = true;
                                        }
                                        warn!(mint = %mint, attempt, sig = %sig,
                                            "sell reverted 6005 (BondingCurveComplete); token \
                                             confirmed migrated — re-routing to AMM");
                                    } else {
                                        // 6005 but chain still says not migrated: structural.
                                        warn!(mint = %mint, attempt, sig = %sig,
                                            "sell reverted 6005 but chain reports not migrated; \
                                             marking ExitFailed");
                                        return SellOutcome::Failed { sigs: sell_sigs };
                                    }
                                }
                                Err(err) => {
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "curve-facts refresh failed after 6005 revert: {err}; \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                            }
                        }
                        SwapRetryDecision::Retry => {
                            warn!(mint = %mint, attempt, remaining = remaining_amount,
                                "sell not cleared within poll window; retrying with a higher tip");
                        }
                    }
                }
            }
            Ok(None) => warn!(mint = %mint, attempt, amount, "sell returned no signature (no-op)"),
            Err(err) => warn!(mint = %mint, attempt, amount, "sell error: {err}"),
        }

        if attempt < max_attempts {
            sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).saturating_add(100);
        } else {
            warn!(mint = %mint, amount, "sell failed after {max_attempts} attempts");
            return SellOutcome::Failed { sigs: sell_sigs };
        }
    }

    // Loop exited with `amount == 0` (fully chased): confirm the clear from OUR sell
    // signatures' summed legs. Anything else is a failure to clear.
    if amount == 0 {
        let legs = trade_repo
            .sum_legs_by_signatures(&wallet, &mint, &sell_sigs, trading_core::models::trade::TradeType::Sell)
            .await
            .ok()
            .flatten();
        SellOutcome::Cleared { sigs: sell_sigs, legs }
    } else {
        SellOutcome::Failed { sigs: sell_sigs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: Result<SigStatus, &'static str>) -> SilentSendOutcome {
        classify_silent_send(&status)
    }

    #[test]
    fn slippage_revert_resends_structural_gives_up() {
        // A buy slippage floor → resend (re-quoted with fresh reserves).
        assert_eq!(
            outcome(reverted(Some(pump_trader::CURVE_TOO_MUCH_SOL_REQUIRED))),
            SilentSendOutcome::Resend
        );
        // A structural/unknown revert (or a missing code) → give up, not resend.
        assert_eq!(outcome(reverted(Some(6022))), SilentSendOutcome::GiveUp);
        assert_eq!(outcome(reverted(None)), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn landed_but_unindexed_waits_never_resends() {
        assert_eq!(outcome(Ok(SigStatus::Succeeded)), SilentSendOutcome::WaitThenSettle);
    }

    #[test]
    fn pending_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Ok(SigStatus::Pending)), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn status_error_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Err("rpc down")), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn only_a_confirmed_revert_ever_resends() {
        // The double-buy invariant: a non-revert status NEVER resends (neither the
        // slippage resend nor the refresh-then-resend), so a nonce tx that could
        // still land can't be double-bought.
        for status in [Ok(SigStatus::Succeeded), Ok(SigStatus::Pending), Err("x")] {
            let o = outcome(status);
            assert_ne!(o, SilentSendOutcome::Resend);
            assert_ne!(o, SilentSendOutcome::RefreshCreatorThenResend);
        }
    }

    #[test]
    fn snipe_decision_covers_all_buy_codes() {
        use pump_trader::{
            AMM_BUY_SLIPPAGE_BELOW_MIN_BASE, ANCHOR_CONSTRAINT_SEEDS,
            CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT, CURVE_TOO_MUCH_SOL_REQUIRED,
        };
        // Curve buy slippage floors → resend (fresh reserves re-quoted per attempt).
        for code in [CURVE_TOO_MUCH_SOL_REQUIRED, CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT] {
            assert_eq!(outcome(reverted(Some(code))), SilentSendOutcome::Resend);
        }
        // Stale-creator 2006 → refresh creator, resend only if it changed (the
        // changed/unchanged decision itself lives at the call site).
        assert_eq!(
            outcome(reverted(Some(ANCHOR_CONSTRAINT_SEEDS))),
            SilentSendOutcome::RefreshCreatorThenResend
        );
        // Structural (6022), an AMM-only slippage code on the curve route (6040), and
        // a missing code are all futile on a curve snipe → give up.
        for code in [Some(6022u32), Some(AMM_BUY_SLIPPAGE_BELOW_MIN_BASE), None] {
            assert_eq!(outcome(reverted(code)), SilentSendOutcome::GiveUp);
        }
    }

    fn recover(status: Result<Option<bool>, &'static str>) -> BuyRecoveryVerdict {
        classify_submitted_buy(&status)
    }

    #[test]
    fn only_a_confirmed_revert_drops_a_buy_submitted_row() {
        assert_eq!(recover(Ok(Some(false))), BuyRecoveryVerdict::Drop);
    }

    #[test]
    fn landed_pending_or_unknown_buy_waits_never_drops() {
        for status in [Ok(Some(true)), Ok(None), Err("rpc down")] {
            assert_eq!(recover(status), BuyRecoveryVerdict::Wait);
        }
    }

    fn reverted(custom: Option<u32>) -> Result<SigStatus, &'static str> {
        Ok(SigStatus::Reverted { custom })
    }

    use pump_trader::{AMM_EXCEEDED_SLIPPAGE, ANCHOR_CONSTRAINT_SEEDS, BONDING_CURVE_COMPLETE,
        CURVE_MISSING_USER_VOLUME_ACCUMULATOR, CURVE_TOO_LITTLE_SOL_RECEIVED};

    /// A confirmed revert on the same route is fed to the SSOT per-code decision.
    fn reclass(
        state: Result<SigStatus, &'static str>,
        used_migrated: bool,
        now_migrated: bool,
    ) -> SellConfirmAction {
        classify_sell_confirm(&state, used_migrated, now_migrated)
    }

    #[test]
    fn structural_revert_same_route_stops_fee_burn() {
        assert_eq!(
            reclass(reverted(Some(6022)), false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::StopFeeBurn)
        );
    }

    #[test]
    fn slippage_revert_same_route_retries() {
        // Curve slippage floor on a curve route → retryable.
        assert_eq!(
            reclass(reverted(Some(CURVE_TOO_LITTLE_SOL_RECEIVED)), false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::Retry)
        );
        // AMM slippage floor on an AMM route → retryable.
        assert_eq!(
            reclass(reverted(Some(AMM_EXCEEDED_SLIPPAGE)), true, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::Retry)
        );
    }

    #[test]
    fn curve_constraint_seeds_refreshes_creator() {
        assert_eq!(
            reclass(reverted(Some(ANCHOR_CONSTRAINT_SEEDS)), false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::RefreshCreator)
        );
    }

    #[test]
    fn amm_constraint_seeds_refreshes_coin_creator() {
        // The same 2006 code on an AMM route means the pool's coin_creator rotated
        // — refresh the pool, not the curve creator.
        assert_eq!(
            reclass(reverted(Some(ANCHOR_CONSTRAINT_SEEDS)), true, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::RefreshCoinCreator)
        );
    }

    #[test]
    fn missing_uva_refreshes_cashback() {
        assert_eq!(
            reclass(reverted(Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR)), false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::RefreshCashback)
        );
    }

    #[test]
    fn bonding_curve_complete_reroutes_migrated() {
        assert_eq!(
            reclass(reverted(Some(BONDING_CURVE_COMPLETE)), false, false),
            SellConfirmAction::Reclassify(SwapRetryDecision::RerouteMigrated)
        );
    }

    #[test]
    fn curve_only_codes_do_not_trigger_on_amm_route() {
        // 6024 and 6005 are pump.fun curve errors — they must fall through to
        // StopFeeBurn if we somehow get them on an AMM route (shouldn't happen,
        // but the guard must hold).
        assert_eq!(
            reclass(reverted(Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR)), true, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::StopFeeBurn)
        );
        assert_eq!(
            reclass(reverted(Some(BONDING_CURVE_COMPLETE)), true, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::StopFeeBurn)
        );
    }

    #[test]
    fn migration_mid_exit_reroutes_only_on_a_reverted_status() {
        // Route changed (curve→AMM) on a REVERTED tx → re-route regardless of the code.
        assert_eq!(
            reclass(reverted(Some(6022)), false, true),
            SellConfirmAction::Reclassify(SwapRetryDecision::Retry)
        );
    }

    #[test]
    fn non_revert_status_never_resends_the_sell() {
        // The C1 fix: a sell whose sent tx Succeeded / is Pending / errored on the
        // status check must NEVER re-send (the old `_ => Retry` double-sold). Every
        // such status → WaitConfirm, on EITHER route and even across a mid-exit
        // migration (a landed sell isn't un-sold by the token migrating).
        for used in [false, true] {
            for now in [false, true] {
                assert_eq!(reclass(Ok(SigStatus::Succeeded), used, now), SellConfirmAction::WaitConfirm);
                assert_eq!(reclass(Ok(SigStatus::Pending), used, now), SellConfirmAction::WaitConfirm);
                assert_eq!(reclass(Err("rpc down"), used, now), SellConfirmAction::WaitConfirm);
            }
        }
        // And never a re-send decision for any of them.
        for state in [Ok(SigStatus::Succeeded), Ok(SigStatus::Pending), Err("x")] {
            assert_ne!(reclass(state, false, false), SellConfirmAction::Reclassify(SwapRetryDecision::Retry));
        }
    }
}
