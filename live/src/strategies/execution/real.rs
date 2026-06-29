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

/// Decision for a snipe buy that was sent but whose fill never appeared in the
/// WS/DB feed within the poll window. Centralizing it keeps the **double-buy
/// safety rule** — *re-send only on a confirmed on-chain revert* — in one place
/// that is unit-tested without a live chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentSendOutcome {
    /// Tx reverted on-chain (no tokens bought) — safe to re-send.
    Resend,
    /// Tx landed but isn't indexed yet — wait for the row; never re-send (a
    /// second buy would double-fill).
    WaitThenSettle,
    /// Pending/dropped, or the status check failed — ambiguous, so give up rather
    /// than risk double-buying from a nonce tx that could still land.
    GiveUp,
}

/// Map a one-shot `signature_state` result to the post-poll action. Pure; generic
/// over the error type so tests need no `anyhow` value.
fn classify_silent_send<E>(status: &Result<Option<bool>, E>) -> SilentSendOutcome {
    match status {
        Ok(Some(false)) => SilentSendOutcome::Resend,
        Ok(Some(true)) => SilentSendOutcome::WaitThenSettle,
        Ok(None) | Err(_) => SilentSendOutcome::GiveUp,
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

/// Convert the token cache's in-memory virtual reserves — token side in raw units,
/// SOL side in SOL — into the `(virtual_token, virtual_quote=lamports)` pair the
/// snipe buy's slippage `min_out` expects. `None` when either snapshot is missing or
/// non-positive, so the buy proceeds unprotected (`min_out=1`) rather than blocking
/// on an inline reserve RPC (the latency budget forbids it on this path).
pub(crate) fn snipe_reserves_from_cache(
    virtual_token_reserves: Option<f64>,
    virtual_sol_reserves: Option<f64>,
) -> Option<(u128, u128)> {
    let vt = virtual_token_reserves?;
    let vsol = virtual_sol_reserves?;
    if vt <= 0.0 || vsol <= 0.0 {
        return None;
    }
    let vq_lamports = vsol * pump_trader::constants::LAMPORTS_PER_SOL as f64;
    Some((vt as u128, vq_lamports as u128))
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
    let mint = position.mint.clone();
    // Rent reclaim: the token account is already empty — fire-and-forget close.
    {
        let trader = trader.clone();
        let mint = mint.clone();
        tokio::spawn(async move {
            if let Err(err) = trader.close_token_account(&mint, None).await {
                debug!(mint = %mint, "rent-reclaim close skipped: {err}");
            }
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
    ) -> anyhow::Result<String>;
    async fn check_signature(&self, signature: &str) -> anyhow::Result<Option<bool>>;
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
    ) -> anyhow::Result<String> {
        // The trader returns `pump_trader::TradeError`; this trait method is
        // `anyhow::Result`, so lift it (the `?` would also work via `From`).
        self.buy_token_snipe_write_ahead(mint, creator, token_program_id, amount, slippage_bps, reserves, on_signed, cashback_enabled)
            .await
            .map_err(anyhow::Error::from)
    }
    async fn check_signature(&self, signature: &str) -> anyhow::Result<Option<bool>> {
        self.signature_state(signature).await.map_err(anyhow::Error::from)
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
/// revert re-sends.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn buy_until_filled_or_give_up<E: SnipeExecutor + 'static>(
    trader: Arc<E>,
    mint: String,
    creator: String,
    token_program_id: String,
    buy_amount: f64,
    position_id: Uuid,
    repo: StrategyRepo,
    trade_repo: TradeRepo,
    runtime: Arc<StrategyRuntimeCache>,
    trade_signals: Arc<TradeSignals>,
    cfg: BuyRetryCfg,
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
    cashback_enabled: bool,
) {
    let wallet = trader.wallet();
    let max_attempts = cfg.max_attempts;
    let mut backoff_ms = cfg.backoff_ms;
    // The submitted signatures of OUR buys so far. Per-signature attribution: the
    // entry is recovered by one of THESE signatures, never the latest buy on the
    // shared (wallet, mint) feed — so two concurrent positions on the same token
    // can't adopt each other's fill.
    let mut sent_sigs: Vec<String> = Vec::new();

    for attempt in 1..=max_attempts {
        // Never double-send: if a previous attempt's buy has since landed + indexed,
        // adopt that fill instead of firing another buy.
        if adopt_existing_fill_if_present(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &sent_sigs)
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
                    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(sig.clone());
                    // Write-ahead durable marker: record this signature + flip the row
                    // to `BuySubmitted` BEFORE the tx is submitted, so a crash in the
                    // send→record gap can recover the entry (`redrive_orphaned_buy_
                    // submitted`). Best-effort: the sig is also captured in `signed_slot`
                    // for THIS process, so a failed persist only loses the cross-restart
                    // marker.
                    match repo.mark_buy_submitted(position_id, &sig).await {
                        Ok(Some(updated)) => runtime.sync_position(None, &updated),
                        Ok(None) => {} // already advanced to Holding concurrently
                        Err(err) => warn!(mint = %mint, sig = %sig,
                            "failed to persist BuySubmitted marker (continuing): {err}"),
                    }
                })
            })
        };

        // Snipe send WITHOUT the blocking RPC confirm — the WS/DB trade feed below is
        // the sole confirmation and the entry-price source.
        let send_result = trader
            .send_snipe_buy(&mint, &creator, &token_program_id, buy_amount, slippage_bps, reserves, on_signed, cashback_enabled)
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

        // Poll the WS-fed trade feed for this wallet's buy row.
        if poll_feed_until_entry_fill(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs)
            .await
        {
            return;
        }

        // The fill never showed within the window. One status check decides the next
        // move; the decision table lives in `classify_silent_send`.
        let status = trader.check_signature(&signature).await;
        match classify_silent_send(&status) {
            SilentSendOutcome::Resend => {
                warn!(mint = %mint, attempt, sig = %signature, "buy reverted on-chain; retrying");
            }
            SilentSendOutcome::WaitThenSettle => {
                warn!(mint = %mint, sig = %signature,
                    "buy landed but not yet indexed; awaiting fill without re-send");
                if poll_feed_until_entry_fill(&mint, &wallet, position_id, &repo, &trade_repo, &runtime, &trade_signals, &cfg, &sent_sigs)
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
) -> bool {
    let guard = trade_signals.register(wallet, mint);
    let deadline = Instant::now() + cfg.poll_interval * cfg.poll_attempts as u32;

    loop {
        // Arm the wakeup BEFORE the DB check so a notify in the gap isn't lost.
        let notified = guard.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if adopt_existing_fill_if_present(mint, wallet, position_id, repo, trade_repo, runtime, sent_sigs).await {
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
pub(crate) async fn adopt_existing_fill_if_present(
    mint: &str,
    wallet: &str,
    position_id: Uuid,
    repo: &StrategyRepo,
    trade_repo: &TradeRepo,
    runtime: &Arc<StrategyRuntimeCache>,
    sent_sigs: &[String],
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
                    fill.sol_amount,
                    fill.first_block_time,
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
    let mint = position.mint.clone();
    let target_tokens = position.entry_token_amount.unwrap_or(0);
    let amount = target_tokens;
    let base_token_program = position
        .token_program_id
        .clone()
        .unwrap_or_else(|| trading_core::config::constants::TOKEN_PROGRAM_ID.to_string());

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
    };

    // Rent reclaim (off the hot path): balance confirmed cleared, so close the now-
    // empty token account to recover its ~0.002 SOL rent. Fire-and-forget.
    {
        let trader = trader.clone();
        let mint = mint.clone();
        tokio::spawn(async move {
            if let Err(err) = trader.close_token_account(&mint, None).await {
                debug!(mint = %mint, "rent-reclaim close skipped: {err}");
            }
        });
    }

    if let Some(legs) = legs {
        let prev = position.clone();
        position.close(
            legs.price_per_token(),
            legs.sol_amount,
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
}

/// pump.fun bonding-curve `TooLittleSolReceived` (Anchor 6003) — the sell-side
/// slippage floor. Retryable (re-quote and resend).
const CURVE_TOO_LITTLE_SOL_RECEIVED: u32 = 6003;
/// PumpSwap AMM `ExceededSlippage` (Anchor 6004) — the AMM-side slippage floor.
const AMM_EXCEEDED_SLIPPAGE: u32 = 6004;
/// pump.fun `BondingCurveComplete` (6005): token already migrated to AMM but our
/// cache still had `is_migrated = false`. Re-read curve state and re-route.
const BONDING_CURVE_COMPLETE: u32 = 6005;
/// Anchor `ConstraintSeeds` (2006): on a curve sell it means pump.fun rotated
/// `bonding_curve.creator` after our buy cached the vault — refresh + retry.
const ANCHOR_CONSTRAINT_SEEDS: u32 = 2006;
/// pump.fun `MissingUserVolumeAccumulator` (6024): cashback UVA account not
/// included in the tx because our cache had `is_cashback = false`. Re-read the
/// curve's cashback flag and retry with the UVA included.
const CURVE_MISSING_USER_VOLUME_ACCUMULATOR: u32 = 6024;

/// After a feed-confirm sell's poll window elapses without clearing, one
/// `signature_state_detailed` check decides whether re-sending is worth the fee,
/// using the on-chain **program error code**: a slippage-floor revert retries; a
/// structural/unknown revert on a venue the retry would reuse stops; a curve 2006
/// refreshes the creator vault; 6024 refreshes the cashback flag; 6005 re-routes
/// to the AMM after confirming migration; a mid-exit migration always re-routes.
/// Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SellRetryDecision {
    Retry,
    StopFeeBurn,
    RefreshCreator,
    /// Curve 6024: cashback UVA was missing — re-read cashback flag, retry.
    RefreshCashback,
    /// Curve 6005: token already migrated — re-read migration state, re-route.
    RerouteMigrated,
}

/// Map a landed-and-reverted sell to a retry decision using the on-chain error code.
/// `used_migrated` is the venue the sent tx used (true = AMM); `now_migrated` is the
/// venue the next attempt would use, so a mid-exit migration always retries re-routed.
fn classify_sell_revert<E>(
    state: &Result<SigStatus, E>,
    used_migrated: bool,
    now_migrated: bool,
) -> SellRetryDecision {
    match state {
        Ok(SigStatus::Reverted { custom }) if used_migrated == now_migrated => {
            let slippage_code = if used_migrated {
                AMM_EXCEEDED_SLIPPAGE
            } else {
                CURVE_TOO_LITTLE_SOL_RECEIVED
            };
            if *custom == Some(slippage_code) {
                SellRetryDecision::Retry
            } else if *custom == Some(ANCHOR_CONSTRAINT_SEEDS) && !used_migrated {
                SellRetryDecision::RefreshCreator
            } else if *custom == Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR) && !used_migrated {
                SellRetryDecision::RefreshCashback
            } else if *custom == Some(BONDING_CURVE_COMPLETE) && !used_migrated {
                SellRetryDecision::RerouteMigrated
            } else {
                SellRetryDecision::StopFeeBurn
            }
        }
        _ => SellRetryDecision::Retry,
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

    // Resolve the token account once (cache-first) and reuse it across attempts.
    let token_account_override = trader
        .resolve_cached_token_account(&mint)
        .await
        .ok()
        .flatten()
        .map(|pk| pk.to_string());

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
                    match classify_sell_revert(&state, is_migrated, now_migrated) {
                        SellRetryDecision::StopFeeBurn => {
                            let raw_code = match &state {
                                Ok(SigStatus::Reverted { custom }) => *custom,
                                _ => None,
                            };
                            warn!(mint = %mint, attempt, sig = %sig, raw_error_code = ?raw_code,
                                "sell reverted on-chain (structural/unknown) on a route the retry \
                                 would reuse; stopping (a blind re-send would only re-pay fees)");
                            return SellOutcome::Failed { sigs: sell_sigs };
                        }
                        SellRetryDecision::RefreshCreator => {
                            match trader.refresh_curve_creator_vault(&mint).await {
                                Ok(vault) => warn!(mint = %mint, attempt, sig = %sig,
                                    new_creator_vault = %vault,
                                    "sell reverted on a stale creator_vault (pump set_creator); \
                                     refreshed creator, retrying with a higher tip"),
                                Err(err) => {
                                    warn!(mint = %mint, attempt, sig = %sig,
                                        "creator_vault refresh failed after a 2006 revert: {err}; \
                                         marking ExitFailed");
                                    return SellOutcome::Failed { sigs: sell_sigs };
                                }
                            }
                        }
                        SellRetryDecision::RefreshCashback => {
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
                        SellRetryDecision::RerouteMigrated => {
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
                        SellRetryDecision::Retry => {
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

    fn outcome(status: Result<Option<bool>, &'static str>) -> SilentSendOutcome {
        classify_silent_send(&status)
    }

    #[test]
    fn confirmed_revert_is_the_only_resend() {
        assert_eq!(outcome(Ok(Some(false))), SilentSendOutcome::Resend);
    }

    #[test]
    fn landed_but_unindexed_waits_never_resends() {
        assert_eq!(outcome(Ok(Some(true))), SilentSendOutcome::WaitThenSettle);
    }

    #[test]
    fn pending_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Ok(None)), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn status_error_gives_up_to_avoid_double_buy() {
        assert_eq!(outcome(Err("rpc down")), SilentSendOutcome::GiveUp);
    }

    #[test]
    fn nothing_but_a_confirmed_revert_ever_resends() {
        for status in [Ok(Some(true)), Ok(None), Err("x")] {
            assert_ne!(outcome(status), SilentSendOutcome::Resend);
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

    #[test]
    fn structural_revert_same_route_stops_fee_burn() {
        let structural = reverted(Some(6022));
        assert_eq!(
            classify_sell_revert(&structural, false, false),
            SellRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn slippage_revert_same_route_retries() {
        // Curve slippage floor on a curve route → retryable.
        assert_eq!(
            classify_sell_revert(&reverted(Some(CURVE_TOO_LITTLE_SOL_RECEIVED)), false, false),
            SellRetryDecision::Retry
        );
        // AMM slippage floor on an AMM route → retryable.
        assert_eq!(
            classify_sell_revert(&reverted(Some(AMM_EXCEEDED_SLIPPAGE)), true, true),
            SellRetryDecision::Retry
        );
    }

    #[test]
    fn curve_constraint_seeds_refreshes_creator() {
        assert_eq!(
            classify_sell_revert(&reverted(Some(ANCHOR_CONSTRAINT_SEEDS)), false, false),
            SellRetryDecision::RefreshCreator
        );
    }

    #[test]
    fn missing_uva_refreshes_cashback() {
        assert_eq!(
            classify_sell_revert(
                &reverted(Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR)),
                false,
                false
            ),
            SellRetryDecision::RefreshCashback
        );
    }

    #[test]
    fn bonding_curve_complete_reroutes_migrated() {
        assert_eq!(
            classify_sell_revert(&reverted(Some(BONDING_CURVE_COMPLETE)), false, false),
            SellRetryDecision::RerouteMigrated
        );
    }

    #[test]
    fn curve_only_codes_do_not_trigger_on_amm_route() {
        // 6024 and 6005 are pump.fun curve errors — they must fall through to
        // StopFeeBurn if we somehow get them on an AMM route (shouldn't happen,
        // but the guard must hold).
        assert_eq!(
            classify_sell_revert(
                &reverted(Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR)),
                true,
                true
            ),
            SellRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_sell_revert(&reverted(Some(BONDING_CURVE_COMPLETE)), true, true),
            SellRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn migration_mid_exit_always_retries() {
        // Route changed (curve→AMM) → re-route regardless of the code.
        assert_eq!(
            classify_sell_revert(&reverted(Some(6022)), false, true),
            SellRetryDecision::Retry
        );
    }
}
